use super::*;
use regex::Regex;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct SongKey {
    title: String,
    location: Option<PathBuf>,
}

type ProjectRow<'a> = (&'a Project, &'a ProjectEdit, bool);

struct SongGroup<'a> {
    title: String,
    rows: Vec<ProjectRow<'a>>,
}

enum ProjectEntry<'a> {
    Song(&'a SongKey, &'a SongGroup<'a>),
    File(ProjectRow<'a>),
}

impl Studio {
    pub(super) fn check_project_drives(&mut self, ctx: &Context) {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        if let Some(pending) = &self.projects.drive_check {
            match pending.try_recv() {
                Ok(unavailable) => {
                    for row in &mut self.projects.rows {
                        row.offline |= unavailable.contains(&row.source.scope);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            self.projects.drive_check = None;
        }
        let now = Instant::now();
        if let Some(delay) = self
            .projects
            .next_drive_check
            .and_then(|next| next.checked_duration_since(now))
        {
            ctx.request_repaint_after(delay);
            return;
        }

        let mut scopes = Vec::new();
        for row in &self.projects.rows {
            if !row.offline
                && row.source.scope.path.starts_with("/Volumes")
                && !scopes.contains(&row.source.scope)
            {
                scopes.push(row.source.scope.clone());
            }
        }
        if scopes.is_empty() {
            return;
        }
        let (send, recv) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            scopes.retain(|scope| scope.check(&scope.path).is_err());
            let _ = send.send(scopes);
            ctx.request_repaint();
        });
        self.projects.drive_check = Some(recv);
        self.projects.next_drive_check = Some(now + Duration::from_secs(4));
    }

    pub(super) fn scan_projects(&mut self, ctx: &Context) {
        self.projects.drive_check = None;
        self.projects.next_drive_check = None;
        let settings = self.settings.clone();
        let db = self.db.clone();
        self.job = Some(Job::start(
            ctx,
            "Scanning projects",
            move |cancel, update| {
                let scan = Store::open(&db)?.scan_projects(&settings, cancel, |count| {
                    update(format!("· {count} projects found"))
                })?;
                Ok(Outcome::Projects(scan))
            },
        ));
    }

    pub(super) fn project_tab(&mut self, ctx: &Context) {
        left_panel("project_sources", ctx, |ui| self.project_sources(ctx, ui));
        right_panel("project_detail", ctx, |ui| self.project_detail(ctx, ui));
        center(ctx, |ui| self.project_list(ui));
        self.rule_dialog(ctx);
    }

    fn project_sources(&mut self, ctx: &Context, ui: &mut Ui) {
        caption(ui, "Places");
        if ui
            .selectable_label(self.projects.place.is_none(), "All projects")
            .clicked()
        {
            self.projects.place = None;
            self.projects.browse_path.clear();
            self.projects.opened_song = None;
        }
        if action(ui, self.ready(), "Add location") {
            match folder("Choose a project folder") {
                Ok(Some(root)) => {
                    let path = root.path.clone();
                    if self.save_settings(|settings| replace_scope(&mut settings.roots, root)) {
                        self.projects.place = Some(path);
                        self.projects.browse_path.clear();
                        self.projects.opened_song = None;
                        self.scan_projects(ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        for root in self.settings.roots.clone() {
            let offline = self
                .projects
                .rows
                .iter()
                .any(|row| row.offline && row.source.scope == root);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("Remove this location")
                    .clicked()
                    && self.save_settings(|settings| {
                        settings.roots.retain(|item| item.path != root.path)
                    })
                {
                    if self.projects.place.as_ref() == Some(&root.path) {
                        self.projects.place = None;
                        self.projects.browse_path.clear();
                        self.projects.opened_song = None;
                    }
                    self.scan_projects(ctx);
                }
                let mut label = RichText::new(labels::display_name(&root.path));
                if offline {
                    label = label.color(theme::colors(ui).warn);
                }
                if ui
                    .add(
                        egui::Button::selectable(
                            self.projects.place.as_ref() == Some(&root.path),
                            label,
                        )
                        .truncate(),
                    )
                    .on_hover_text(if offline {
                        format!("Offline\n{}", labels::display_path(&root.path))
                    } else {
                        labels::display_path(&root.path).into_owned()
                    })
                    .clicked()
                {
                    self.projects.place = Some(root.path.clone());
                    self.projects.browse_path.clear();
                    self.projects.opened_song = None;
                }
            });
        }
        if ui
            .add_enabled(
                self.ready()
                    && (!self.settings.roots.is_empty() || !self.settings.manual.is_empty()),
                egui::Button::new("Rescan"),
            )
            .clicked()
        {
            self.scan_projects(ctx);
        }
        ui.separator();
        caption(ui, "Add manually");
        ui.horizontal_wrapped(|ui| {
            for (is_folder, label) in [(false, "Add file"), (true, "Add folder")] {
                if ui
                    .add_enabled(self.ready(), egui::Button::new(label))
                    .clicked()
                {
                    let pick = if is_folder {
                        folder("Choose a project folder to register")
                    } else {
                        file("Choose a project file or package to register")
                    };
                    match pick {
                        Ok(Some(scope)) => {
                            let path = scope.path.clone();
                            if self.save_settings(|settings| {
                                replace_scope(&mut settings.manual, scope)
                            }) {
                                self.projects.selected = Some(path);
                                self.projects.draft = ProjectEdit::default();
                                self.scan_projects(ctx);
                            }
                        }
                        Ok(None) => {}
                        Err(error) => self.error(error),
                    }
                }
            }
        });
        for entry in self.settings.manual.clone() {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("Remove this registered project")
                    .clicked()
                    && self.save_settings(|settings| {
                        settings.manual.retain(|item| item.path != entry.path)
                    })
                {
                    self.scan_projects(ctx);
                }
                path_label(ui, &entry.path);
            });
        }
        ui.separator();
        caption(ui, "DAW settings");
        if let Some(daw) = &self.settings.primary {
            muted(ui, format!("Primary DAW · {daw}"));
        } else {
            muted(ui, "Primary DAW and detection rules");
        }
        if ui
            .add_enabled(self.ready(), egui::Button::new("DAW and detection rules"))
            .clicked()
        {
            self.projects.primary_draft =
                labels::composed(self.settings.primary.as_deref().unwrap_or_default()).into_owned();
            self.projects.show_rules = true;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Supported formats")
            .default_open(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                for (daw, ext) in FORMATS {
                    muted(ui, format!("{daw}  .{ext}"));
                }
            });
    }

    fn rule_dialog(&mut self, ctx: &Context) {
        let mut open = self.projects.show_rules;
        if !open {
            return;
        }
        egui::Window::new("DAW and detection rules")
            .open(&mut open)
            .default_width(470.0)
            .show(ctx, |ui| {
                caption(ui, "Primary DAW");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("daw_preset")
                        .selected_text("Choose a format")
                        .show_ui(ui, |ui| {
                            for (daw, ext) in FORMATS {
                                if ui.button(format!("{daw}  .{ext}")).clicked() {
                                    self.projects.primary_draft = (*daw).into();
                                    self.projects.rule_name = (*daw).into();
                                    self.projects.rule_pattern = (*ext).into();
                                    self.projects.rule_folder = false;
                                }
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.projects.primary_draft)
                            .hint_text("DAW name")
                            .desired_width(180.0),
                    );
                });
                ui.add_space(5.0);
                caption(ui, "Add a detection rule");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.projects.rule_name)
                            .hint_text("DAW name")
                            .desired_width(140.0),
                    );
                    ui.selectable_value(&mut self.projects.rule_folder, false, "Extension");
                    ui.selectable_value(&mut self.projects.rule_folder, true, "Folder name");
                    let hint = if self.projects.rule_folder {
                        "Exact folder name"
                    } else {
                        "e.g. .als"
                    };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.projects.rule_pattern)
                            .hint_text(hint)
                            .desired_width(155.0),
                    );
                });
                if action(ui, self.ready(), "Save settings") {
                    let primary = self.projects.primary_draft.trim().to_owned();
                    let kind = if self.projects.rule_folder {
                        RuleKind::FolderName
                    } else {
                        RuleKind::Extension
                    };
                    let name = if self.projects.rule_name.trim().is_empty() {
                        &primary
                    } else {
                        &self.projects.rule_name
                    };
                    let rule = if self.projects.rule_pattern.trim().is_empty() {
                        Ok(None)
                    } else {
                        Rule::new(name, kind, &self.projects.rule_pattern).map(Some)
                    };
                    match rule {
                        Ok(rule) => {
                            if self.save_settings(|settings| {
                                settings.primary = (!primary.is_empty()).then_some(primary);
                                if let Some(rule) = rule {
                                    settings.rules.retain(|existing| {
                                        existing.kind != rule.kind
                                            || existing.pattern != rule.pattern
                                    });
                                    settings.rules.push(rule);
                                }
                            }) {
                                self.projects.rule_pattern.clear();
                                self.projects.rule_name.clear();
                                self.status = "DAW settings saved".into();
                                if !self.settings.roots.is_empty()
                                    || !self.settings.manual.is_empty()
                                {
                                    self.scan_projects(ctx);
                                }
                            }
                        }
                        Err(error) => self.error(error),
                    }
                }
                ui.separator();
                caption(ui, "Custom rules");
                if self.settings.rules.is_empty() {
                    muted(ui, "No custom rules");
                }
                for (index, rule) in self.settings.rules.clone().into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let pattern = if rule.kind == RuleKind::Extension {
                            format!(".{}", rule.pattern)
                        } else {
                            format!("Folder {}", rule.pattern)
                        };
                        ui.label(labels::composed(format!("{}  ·  {pattern}", rule.daw)));
                        if ui
                            .add_enabled(self.ready(), egui::Button::new("Remove").small())
                            .clicked()
                            && self.save_settings(|settings| {
                                settings.rules.remove(index);
                            })
                        {
                            self.scan_projects(ctx);
                        }
                    });
                }
            });
        self.projects.show_rules = open;
    }

    fn project_list(&mut self, ui: &mut Ui) {
        ui.heading("Projects");
        let mut grouping = self.settings.project_grouping;
        ui.horizontal_wrapped(|ui| {
            search_field(ui, &mut self.projects.query, "Search projects")
                .on_hover_text("Search by title, group, DAW, location, or modified date");
            ui.checkbox(&mut self.projects.review_only, "Needs review");
            muted(ui, "Group by");
            egui::ComboBox::from_id_salt("project_grouping")
                .selected_text(grouping.label())
                .show_ui(ui, |ui| {
                    for mode in ProjectGrouping::ALL {
                        ui.selectable_value(&mut grouping, mode, mode.label());
                    }
                });
        });
        if grouping != self.settings.project_grouping
            && self.save_settings(|settings| settings.project_grouping = grouping)
        {
            self.projects.browse_path.clear();
            self.projects.opened_song = None;
        }
        let grouping = self.settings.project_grouping;
        ui.separator();
        let previous_depth = self.projects.browse_path.len();
        folder_breadcrumb(ui, &mut self.projects.browse_path);
        if self.projects.browse_path.len() != previous_depth {
            self.projects.opened_song = None;
        }
        let query = key(self.projects.query.trim());
        let rows: Vec<_> = self
            .projects
            .rows
            .iter()
            .filter_map(|row| {
                let existing = self.settings.edits.get(&row.source.path);
                let edit = existing.unwrap_or(&row.suggested);
                let review = row.review && existing.is_none();
                let matches = query.is_empty()
                    || [
                        edit.title.as_str(),
                        edit.group.as_str(),
                        &row.source.path.to_string_lossy(),
                        row.daw.as_str(),
                        row.modified_day.as_deref().unwrap_or_default(),
                    ]
                    .into_iter()
                    .any(|field| key(field).contains(&query));
                let in_place = self
                    .projects
                    .place
                    .as_ref()
                    .is_none_or(|place| row.source.path.starts_with(place));
                let in_folder = self
                    .projects
                    .browse_path
                    .first()
                    .is_none_or(|folder| project_folder(row, grouping) == folder);
                (matches && in_place && in_folder && (!self.projects.review_only || review))
                    .then_some((row, edit, review))
            })
            .collect();
        let total_review = self
            .projects
            .rows
            .iter()
            .filter(|item| item.review && !self.settings.edits.contains_key(&item.source.path))
            .count();
        if grouping != ProjectGrouping::Title
            && self.projects.browse_path.is_empty()
            && !rows.is_empty()
        {
            muted(
                ui,
                format!("{} files · {total_review} to review", rows.len()),
            );
            let mut counts = BTreeMap::new();
            for (row, _, _) in rows {
                *counts.entry(project_folder(row, grouping)).or_insert(0) += 1;
            }
            let mut folders: Vec<_> = counts.into_iter().collect();
            if grouping == ProjectGrouping::ModifiedDate {
                folders.sort_by_key(|&(name, _)| (name == "Unknown date", Reverse(name)));
            }
            if let Some(open) = folder_list(
                ui,
                ("project_folders", grouping.label()),
                &folders,
                "Projects",
            ) {
                self.projects.browse_path.push(open);
                self.projects.opened_song = None;
            }
            return;
        }
        let file_count = rows.len();
        let mut songs = group_projects(rows);
        if self.projects.opened_song.is_some() {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Songs").clicked() {
                    self.projects.opened_song = None;
                }
                if let Some(opened) = self.projects.opened_song.as_ref() {
                    let title = songs
                        .get(opened)
                        .map_or(opened.title.as_str(), |song| &song.title);
                    muted(ui, format!("›  {title}"));
                }
            });
        }
        let entries: Vec<_> = if let Some(opened) = &self.projects.opened_song {
            songs
                .get_mut(opened)
                .map(|song| {
                    sort_versions(&mut song.rows);
                    song.rows.iter().copied().map(ProjectEntry::File).collect()
                })
                .unwrap_or_default()
        } else {
            songs
                .iter()
                .map(|(key, song)| {
                    if song.rows.len() == 1 {
                        ProjectEntry::File(song.rows[0])
                    } else {
                        ProjectEntry::Song(key, song)
                    }
                })
                .collect()
        };
        let summary = if self.projects.opened_song.is_some() {
            format!("{} files", entries.len())
        } else {
            format!(
                "{} songs · {file_count} files · {total_review} to review",
                songs.len()
            )
        };
        muted(ui, summary);
        if entries.is_empty() {
            ui.add_space(24.0);
            if self.projects.rows.is_empty()
                && self.settings.roots.is_empty()
                && self.settings.manual.is_empty()
            {
                ui.label("Add a folder to find projects.");
            } else {
                muted(ui, "No matching projects.");
            }
        } else {
            ui.separator();
            ui.horizontal(|ui| {
                caption(
                    ui,
                    if self.projects.opened_song.is_some() {
                        "Versions and backups"
                    } else {
                        "Title"
                    },
                );
            });
        }
        let mut select = None;
        let mut open_song = None;
        ScrollArea::vertical()
            .id_salt((
                "project_files",
                grouping.label(),
                &self.projects.browse_path,
                &self.projects.opened_song,
            ))
            .auto_shrink([false, false])
            .show_rows(ui, 46.0, entries.len(), |ui, visible| {
                for entry in &entries[visible] {
                    match entry {
                        ProjectEntry::Song(key, song) => {
                            if song_row(ui, key, song) {
                                open_song = Some((*key).clone());
                            }
                        }
                        ProjectEntry::File((row, edit, review)) => {
                            let selected =
                                self.projects.selected.as_ref() == Some(&row.source.path);
                            if project_row(ui, row, edit, *review, selected) {
                                select = Some((
                                    row.source.path.clone(),
                                    display_project_edit(row, edit),
                                ));
                            }
                        }
                    }
                }
            });
        if let Some(open) = open_song {
            self.projects.opened_song = Some(open);
        }
        if let Some((path, edit)) = select {
            self.projects.selected = Some(path);
            self.projects.draft = edit;
        }
    }

    fn project_detail(&mut self, ctx: &Context, ui: &mut Ui) {
        let Some(row) = self
            .projects
            .rows
            .iter()
            .find(|row| Some(&row.source.path) == self.projects.selected.as_ref())
            .cloned()
        else {
            caption(ui, "Project details");
            muted(ui, "Select a project to see its location and details.");
            return;
        };
        let path = row.source.path.clone();
        let saved = self
            .settings
            .edits
            .get(&path)
            .unwrap_or(&row.suggested)
            .clone();
        let shown = display_project_edit(&row, &saved);
        if self.projects.draft.title.is_empty() {
            self.projects.draft = shown.clone();
        }
        caption(ui, &row.daw);
        ui.heading(&shown.title);
        if let Some(drive) = row.drive_name() {
            muted(ui, format!("Drive · {drive}"));
        }
        if let Some(day) = &row.modified_day {
            ui.label(
                RichText::new(format!("Modified · {day}"))
                    .small()
                    .color(theme::colors(ui).muted),
            );
        }
        ui.label(
            RichText::new(labels::display_path(&path))
                .small()
                .color(theme::colors(ui).muted),
        );
        if row.offline {
            ui.label(
                RichText::new("Offline · Last known location")
                    .font(theme::font(Face::Italic, 14.0))
                    .color(theme::colors(ui).warn),
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Copy path").clicked() {
                ui.ctx().copy_text(path.to_string_lossy().into_owned());
            }
            if ui
                .add_enabled(!row.offline, egui::Button::new("Show in Finder"))
                .clicked()
                && let Err(error) = reveal(&row.source)
            {
                self.error(error);
            }
        });
        let review = row.review && !self.settings.edits.contains_key(&path);
        if review {
            ui.label(
                RichText::new(
                    "Multiple projects share this name. Check the song group and version.",
                )
                .color(theme::colors(ui).warn),
            );
        }
        if row.offline {
            return;
        }
        ui.separator();
        for (label, value) in [
            ("Song title", &mut self.projects.draft.title),
            ("Version", &mut self.projects.draft.version),
            ("Song group", &mut self.projects.draft.group),
        ] {
            ui.label(label);
            if label == "Version"
                && labels::version(value) == "Unversioned"
                && value != "Unversioned"
            {
                let mut edit = String::new();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut edit)
                            .hint_text("Unversioned")
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    *value = edit;
                }
            } else {
                ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
            }
        }
        let editing = self.projects.draft != shown;
        if ui
            .add_enabled(
                self.ready() && (editing || review),
                egui::Button::new(if review {
                    "Confirm and save"
                } else {
                    "Save project details"
                }),
            )
            .clicked()
        {
            match validate_project_edit(&self.projects.draft, &shown, &saved) {
                Ok(edit) => {
                    self.projects.draft = display_project_edit(&row, &edit);
                    if self.save_settings(|settings| {
                        settings.edits.insert(path.clone(), edit);
                    }) {
                        self.status = "Project details saved".into();
                    }
                }
                Err(error) => self.error(error),
            }
        }
        ui.separator();
        caption(ui, "Copy project");
        muted(ui, projects::media_help(&extension(&path)));
        if row.directory {
            muted(ui, "The copy preserves the folder structure.");
        }
        if !row.companions.is_empty() {
            muted(
                ui,
                format!(
                    "Included folders · {}",
                    row.companions
                        .iter()
                        .map(|entry| labels::display_name(&entry.path))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        let missing_fender_folder = extension(&path) == "song" && !row.source.scope.directory;
        if missing_fender_folder {
            ui.label(
                RichText::new(
                    "Add the folder containing the .song file to include Media and History.",
                )
                .color(theme::colors(ui).warn),
            );
        }
        let can_copy = self.ready() && !review && !editing && !missing_fender_folder;
        if action(ui, can_copy, "Plan a copy") {
            match folder("Choose a destination for the project copy") {
                Ok(Some(destination)) => {
                    let related = self
                        .settings
                        .related
                        .get(&path)
                        .cloned()
                        .unwrap_or_default();
                    let project = row.clone();
                    self.job = Some(Job::start(ctx, "Preparing copy plan", move |cancel, _| {
                        Ok(Outcome::Plan(project_plan(
                            &project,
                            &saved,
                            &related,
                            destination,
                            cancel,
                        )?))
                    }));
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        ui.separator();
        self.project_audio(ui, &row);
    }

    fn project_audio(&mut self, ui: &mut Ui, project: &Project) {
        let path = &project.source.path;
        caption(ui, "Related files");
        muted(ui, "References unverified. Check media links in your DAW.");
        let related = self.settings.related.get(path).cloned().unwrap_or_default();
        if ui
            .add_enabled(self.ready(), egui::Button::new("Add related file"))
            .clicked()
        {
            match file("Choose a related file") {
                Ok(Some(pick)) if pick.directory => {
                    self.error(anyhow::anyhow!("Select one related file."))
                }
                Ok(Some(pick)) => {
                    let source = if project.source.scope.accepts(&pick.path) {
                        project.source.scope.source(pick.path)
                    } else {
                        pick.source(pick.path.clone())
                    };
                    self.add_related(path, source);
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        let selected: BTreeSet<_> = related.iter().map(|item| item.path.clone()).collect();
        for source in related {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("Remove this related file")
                    .clicked()
                {
                    self.save_settings(|settings| {
                        if let Some(sources) = settings.related.get_mut(path) {
                            sources.retain(|item| item.path != source.path);
                        }
                    });
                }
                path_label(ui, &source.path);
            });
        }
        ui.add_space(4.0);
        egui::CollapsingHeader::new(format!("Nearby audio ({})", project.nearby.len())).show(
            ui,
            |ui| {
                for source in &project.nearby {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.ready() && !selected.contains(&source.path),
                                egui::Button::new("+").small(),
                            )
                            .on_hover_text("Add to related files")
                            .clicked()
                        {
                            self.add_related(path, source.clone());
                        }
                        path_label(ui, &source.path);
                    });
                }
            },
        );
    }

    fn add_related(&mut self, path: &Path, source: Source) {
        if self.save_settings(|settings| {
            let related = settings.related.entry(path.to_owned()).or_default();
            if related.iter().all(|item| item.path != source.path) {
                related.push(source);
            }
        }) {
            self.status = "Related file saved. Check media references in your DAW.".into();
        }
    }
}

fn project_folder(project: &Project, grouping: ProjectGrouping) -> &str {
    match grouping {
        ProjectGrouping::Title => "All projects",
        ProjectGrouping::Daw => &project.daw,
        ProjectGrouping::ModifiedDate => project.modified_day.as_deref().unwrap_or("Unknown date"),
    }
}

fn ableton_backup(path: &Path) -> Option<(&str, &str)> {
    static BACKUP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(.+) \[([0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{6})\]$").unwrap());
    if !path.extension()?.eq_ignore_ascii_case("als")
        || !path.parent()?.file_name()?.eq_ignore_ascii_case("Backup")
    {
        return None;
    }
    let captures = BACKUP.captures(path.file_stem()?.to_str()?)?;
    Some((captures.get(1)?.as_str(), captures.get(2)?.as_str()))
}

fn group_projects(rows: Vec<ProjectRow<'_>>) -> BTreeMap<SongKey, SongGroup<'_>> {
    let mut songs = BTreeMap::new();
    for entry @ (row, edit, _) in rows {
        let (title, location) = if edit.group != row.suggested.group {
            (edit.group.clone(), None)
        } else if let Some((title, _)) = ableton_backup(&row.source.path) {
            (
                projects::title_version(title).0,
                row.source.path.parent().and_then(Path::parent),
            )
        } else {
            (edit.group.clone(), Some(row.grouping_location()))
        };
        let title = labels::composed(title).into_owned();
        let id = SongKey {
            title: key(&title),
            location: location.map(Path::to_owned),
        };
        songs
            .entry(id)
            .or_insert_with(|| SongGroup {
                title,
                rows: Vec::new(),
            })
            .rows
            .push(entry);
    }
    songs
}

fn sort_versions(rows: &mut [ProjectRow<'_>]) {
    rows.sort_by_key(|&(row, edit, _)| {
        let backup = ableton_backup(&row.source.path).map(|(_, stamp)| stamp);
        let version = edit
            .version
            .strip_prefix('v')
            .and_then(|value| value.parse::<u32>().ok());
        (
            backup.is_some(),
            Reverse(backup),
            Reverse(version),
            Reverse(row.modified_day.as_deref()),
            &row.source.path,
        )
    });
}

fn song_row(ui: &mut Ui, key: &SongKey, song: &SongGroup<'_>) -> bool {
    let colors = theme::colors(ui);
    let button = egui::Button::new(format!("›  {}", song.title))
        .frame_when_inactive(false)
        .right_text(RichText::new(format!("{} files", song.rows.len())).small())
        .truncate();
    let response = ui.add_sized([ui.available_width(), 24.0], button);
    ui.horizontal(|ui| {
        if song.rows.iter().any(|&(_, _, review)| review) {
            project_badge(ui, "Review", Face::Regular, colors.warn);
        }
        if song.rows.iter().all(|&(row, _, _)| row.offline) {
            project_badge(ui, "Offline", Face::Italic, colors.warn);
        }
        if let Some(location) = &key.location {
            project_badge(
                ui,
                labels::display_name(location),
                Face::Regular,
                colors.muted,
            )
            .on_hover_text(labels::display_path(location));
        } else {
            project_badge(ui, "Song group", Face::Regular, colors.muted);
        }
    });
    response.clicked()
}

fn project_row(
    ui: &mut Ui,
    row: &Project,
    edit: &ProjectEdit,
    review: bool,
    selected: bool,
) -> bool {
    let shown = display_project_edit(row, edit);
    let version = labels::version(&shown.version);
    let version = if version == "Unversioned" {
        ""
    } else {
        version
    };
    let colors = theme::colors(ui);
    let button = egui::Button::selectable(selected, shown.title)
        .right_text(RichText::new(version).font(theme::font(Face::Semibold, 14.0)))
        .truncate();
    let path = &row.source.path;
    let response = ui
        .add_sized([ui.available_width(), 24.0], button)
        .on_hover_text(labels::display_path(path));
    ui.horizontal(|ui| {
        if row.offline {
            project_badge(ui, "Offline", Face::Italic, colors.warn);
        }
        if review {
            project_badge(ui, "Review", Face::Regular, colors.warn);
        }
        if row.manual {
            project_badge(ui, "Manual", Face::Regular, colors.accent);
        }
        let description = if let Some((_, stamp)) = ableton_backup(path) {
            format!(
                "Backup · {} {}:{}:{}",
                &stamp[..10],
                &stamp[11..13],
                &stamp[13..15],
                &stamp[15..]
            )
        } else if let Some(parent) = path.parent() {
            format!("{} · {}", row.daw, labels::display_name(parent))
        } else {
            row.daw.clone()
        };
        project_badge(ui, description, Face::Regular, colors.muted)
            .on_hover_text(labels::display_path(path));
    });
    response.clicked()
}

fn project_badge(
    ui: &mut Ui,
    text: impl Into<String>,
    face: Face,
    color: egui::Color32,
) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(labels::composed(text.into()))
                .font(theme::font(face, 12.5))
                .color(color),
        )
        .truncate(),
    )
}

fn display_project_edit(project: &Project, edit: &ProjectEdit) -> ProjectEdit {
    let mut display = display_edit(edit);
    if let Some((title, _)) = ableton_backup(&project.source.path) {
        let title = labels::composed(projects::title_version(title).0).into_owned();
        if edit.title == project.suggested.title {
            display.title = title.clone();
        }
        if edit.group == project.suggested.group {
            display.group = title;
        }
    }
    display
}

fn display_edit(edit: &ProjectEdit) -> ProjectEdit {
    ProjectEdit {
        title: labels::composed(edit.title.as_str()).into_owned(),
        version: labels::composed(edit.version.as_str()).into_owned(),
        group: labels::composed(edit.group.as_str()).into_owned(),
    }
}

fn validate_project_edit(
    draft: &ProjectEdit,
    shown: &ProjectEdit,
    saved: &ProjectEdit,
) -> Result<ProjectEdit> {
    fn component(draft: &str, shown: &str, saved: &str) -> Result<String> {
        // Keep the copy destination spelling when the user only changes another field.
        safe_component(if draft.trim() == shown { saved } else { draft }).map(str::to_owned)
    }
    Ok(ProjectEdit {
        title: component(&draft.title, &shown.title, &saved.title)?,
        version: component(&draft.version, &shown.version, &saved.version)?,
        group: component(&draft.group, &shown.group, &saved.group)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_ableton_backups_with_their_song_and_keeps_other_locations_separate() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let scope = Scope::select(temp.path())?;
        for file in [
            "Sessions/0714 Project/0714.als",
            "Sessions/0714 Project/Backup/0714 [2026-07-14 031824].als",
            "Sessions/0714 Project/Backup/0714 [2026-07-14 032445].als",
            "Other sessions/0714.als",
            "Other sessions/0714 [2026-07-14 031824].als",
        ] {
            let path = scope.path.join(file);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, b"mock project")?;
        }
        let scan = projects::scan(
            &[scope],
            &[],
            &[],
            &[],
            &std::sync::atomic::AtomicBool::new(false),
            |_| {},
        )?;
        let mut songs = group_projects(
            scan.projects
                .iter()
                .map(|row| (row, &row.suggested, row.review))
                .collect(),
        );
        assert_eq!(songs.len(), 3);
        let song = songs
            .values_mut()
            .find(|song| song.rows.len() == 3)
            .unwrap();
        assert_eq!(song.title, "0714");
        sort_versions(&mut song.rows);
        assert!(ableton_backup(&song.rows[0].0.source.path).is_none());
        assert_eq!(
            ableton_backup(&song.rows[1].0.source.path),
            Some(("0714", "2026-07-14 032445"))
        );
        let backup = song.rows[1].0;
        assert_eq!(
            display_project_edit(backup, &backup.suggested).title,
            "0714"
        );
        assert!(backup.suggested.title.ends_with("[2026-07-14 032445]"));
        Ok(())
    }

    #[test]
    fn saving_one_field_preserves_other_fields_used_for_copy_paths() -> Result<()> {
        let saved = ProjectEdit {
            title: "\u{1100}\u{1169}\u{11BC}\u{1107}\u{116E}".into(),
            group: "\u{1111}\u{1169}\u{11AF}\u{1103}\u{1165}".into(),
            version: "v01".into(),
        };
        let shown = display_edit(&saved);
        assert_eq!(shown.title, "공부");
        assert_eq!(shown.group, "폴더");
        let mut draft = shown.clone();
        draft.version = "v02".into();
        let updated = validate_project_edit(&draft, &shown, &saved)?;
        assert_eq!(updated.title, saved.title);
        assert_eq!(updated.group, saved.group);
        assert_eq!(updated.version, "v02");
        Ok(())
    }
}
