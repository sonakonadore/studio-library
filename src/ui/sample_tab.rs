use super::*;
use std::collections::BTreeMap;
use studio_library::catalog::{Sample, SampleGrouping};

impl Studio {
    pub(super) fn replace_samples(&mut self, rows: Vec<Sample>) {
        self.samples.rows = rows;
        let paths: BTreeSet<_> = self
            .samples
            .rows
            .iter()
            .map(|row| &row.source.path)
            .collect();
        self.samples.selected.retain(|path| paths.contains(path));
        if self
            .samples
            .playing
            .as_ref()
            .is_some_and(|path| !paths.contains(path))
        {
            self.player.stop();
        }
        if self
            .samples
            .focus
            .as_ref()
            .is_some_and(|path| !paths.contains(path))
        {
            self.samples.focus = None;
        }
    }

    fn scan_samples(&mut self, ctx: &Context) {
        let folders: Vec<_> = self.settings.sample_folders().cloned().collect();
        if folders.is_empty() {
            return;
        }
        let db = self.db.clone();
        let outputs = self.settings.outputs.clone();
        self.job = Some(Job::start(
            ctx,
            "Scanning samples",
            move |cancel, update| {
                let scan = Store::open(&db)?.scan_sample_folders(
                    &folders,
                    &outputs,
                    cancel,
                    |count, decoded| update(format!("· {count} found · {decoded} analyzed")),
                )?;
                Ok(Outcome::Samples(scan))
            },
        ));
    }

    pub(super) fn sample_tab(&mut self, ctx: &Context) {
        left_panel("sample_sources", ctx, |ui| self.sample_sources(ctx, ui));
        right_panel("sample_detail", ctx, |ui| self.sample_detail(ui));
        center(ctx, |ui| self.sample_list(ctx, ui));
    }

    fn sample_sources(&mut self, ctx: &Context, ui: &mut Ui) {
        caption(ui, "Places");
        if ui
            .selectable_label(
                self.samples.place.is_none() && self.samples.active_folder.is_none(),
                "All samples",
            )
            .clicked()
        {
            self.samples.place = None;
            self.samples.active_folder = None;
            self.samples.filters.clear();
            self.samples.browse_path.clear();
        }
        if self.settings.sample_folders().next().is_none() {
            muted(ui, "Add a Splice or other local sample folder.");
        }
        if action(ui, self.ready(), "Add folder") {
            match folder("Choose a sample folder") {
                Ok(Some(scope)) => {
                    let place = scope.path.clone();
                    if self.save_settings(|settings| {
                        if settings
                            .samples
                            .as_ref()
                            .is_none_or(|first| first.path == scope.path)
                        {
                            settings.samples = Some(scope);
                        } else {
                            replace_scope(&mut settings.additional_sample_folders, scope);
                        }
                    }) {
                        self.samples.place = Some(place);
                        self.samples.active_folder = None;
                        self.samples.filters.clear();
                        self.samples.browse_path.clear();
                        self.scan_samples(ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        let folders: Vec<_> = self
            .settings
            .sample_folders()
            .map(|root| root.path.clone())
            .collect();
        for path in &folders {
            let mut remove = false;
            ui.horizontal(|ui| {
                remove = ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("Remove this location")
                    .clicked();
                if ui
                    .add(
                        egui::Button::selectable(
                            self.samples.place.as_ref() == Some(path),
                            labels::display_name(path),
                        )
                        .truncate(),
                    )
                    .on_hover_text(labels::display_path(path))
                    .clicked()
                {
                    self.samples.place = Some(path.clone());
                    self.samples.active_folder = None;
                    self.samples.filters.clear();
                    self.samples.browse_path.clear();
                }
            });
            if remove
                && self.save_settings(|settings| {
                    if settings
                        .samples
                        .as_ref()
                        .is_some_and(|first| &first.path == path)
                    {
                        settings.samples = None;
                    }
                    settings
                        .additional_sample_folders
                        .retain(|root| &root.path != path);
                    if settings.samples.is_none() && !settings.additional_sample_folders.is_empty()
                    {
                        settings.samples = Some(settings.additional_sample_folders.remove(0));
                    }
                })
            {
                if self.samples.place.as_ref() == Some(path) {
                    self.samples.place = None;
                    self.samples.browse_path.clear();
                }
                match self.store.samples_in(self.settings.sample_folders()) {
                    Ok(rows) => {
                        self.replace_samples(rows);
                        self.status = "Sample folder removed".into();
                    }
                    Err(error) => self.error(error),
                }
            }
        }
        if ui
            .add_enabled(
                self.ready() && self.settings.sample_folders().next().is_some(),
                egui::Button::new("Scan for changes"),
            )
            .clicked()
        {
            self.scan_samples(ctx);
        }
        ui.separator();
        caption(ui, "Collections");
        for (index, item) in self
            .settings
            .virtual_folders
            .clone()
            .into_iter()
            .enumerate()
        {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("Remove this collection")
                    .clicked()
                    && self.save_settings(|settings| {
                        settings.virtual_folders.remove(index);
                    })
                    && self.samples.active_folder.as_ref() == Some(&item.name)
                {
                    self.samples.active_folder = None;
                    self.samples.filters.clear();
                    self.samples.browse_path.clear();
                }
                if ui
                    .add(
                        egui::Button::selectable(
                            self.samples.active_folder.as_ref() == Some(&item.name),
                            labels::composed(item.name.as_str()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(display_tags(&item.tags))
                    .clicked()
                {
                    self.samples.active_folder = Some(item.name);
                    self.samples.place = None;
                    self.samples.filters = item.tags;
                    self.samples.browse_path.clear();
                }
            });
        }
        let save_enabled = self.ready() && !self.samples.filters.is_empty();
        if ui
            .add_enabled(
                save_enabled,
                egui::Button::new("Save filters as collection"),
            )
            .on_disabled_hover_text("Select a filter first")
            .clicked()
        {
            self.samples.show_folder_form = !self.samples.show_folder_form;
        }
        if self.samples.show_folder_form {
            ui.add(
                egui::TextEdit::singleline(&mut self.samples.folder_name)
                    .hint_text("Collection name")
                    .desired_width(175.0),
            );
            if ui
                .add_enabled(save_enabled, egui::Button::new("Save"))
                .clicked()
            {
                let name = self.samples.folder_name.trim().to_owned();
                let tags = self.samples.filters.clone();
                let duplicate = self
                    .settings
                    .virtual_folders
                    .iter()
                    .any(|folder| key(&folder.name) == key(&name));
                if name.is_empty() || duplicate {
                    self.error(anyhow::anyhow!("Enter a unique collection name."));
                } else if self.save_settings(|settings| {
                    settings.virtual_folders.push(VirtualFolder {
                        name: name.clone(),
                        tags,
                    })
                }) {
                    self.samples.active_folder = Some(name);
                    self.samples.place = None;
                    self.samples.folder_name.clear();
                    self.samples.show_folder_form = false;
                    self.status = "Collection saved".into();
                }
            }
        }
    }

    fn sample_list(&mut self, ctx: &Context, ui: &mut Ui) {
        ui.heading("Samples");
        let mut grouping = self.settings.sample_grouping;
        ui.horizontal_wrapped(|ui| {
            search_field(ui, &mut self.samples.query, "Search files or tags");
            if ui
                .selectable_label(self.samples.show_filters, "Filters")
                .clicked()
            {
                self.samples.show_filters = !self.samples.show_filters;
            }
            muted(ui, "Group by");
            egui::ComboBox::from_id_salt("sample_grouping")
                .selected_text(grouping.label())
                .show_ui(ui, |ui| {
                    for mode in SampleGrouping::ALL {
                        ui.selectable_value(&mut grouping, mode, mode.label());
                    }
                });
        });
        if grouping != self.settings.sample_grouping
            && self.save_settings(|settings| settings.sample_grouping = grouping)
        {
            self.samples.browse_path.clear();
        }
        if self.samples.show_filters {
            ScrollArea::vertical()
                .id_salt("sample_filters")
                .max_height(196.0)
                .show(ui, |ui| {
                    self.tag_filter_menu(ui);
                });
        }
        if !self.samples.filters.is_empty() {
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (index, filter) in self.samples.filters.iter().enumerate() {
                    if ui
                        .button(format!("{} ×", labels::composed(labels::tag(filter))))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                }
            });
            if let Some(index) = remove {
                self.samples.filters.remove(index);
                self.samples.active_folder = None;
            }
        }
        ui.separator();
        folder_breadcrumb(ui, &mut self.samples.browse_path);
        let query = key(&self.samples.query);
        let matching = self.samples.rows.iter().filter(|row| {
            self.samples
                .place
                .as_ref()
                .is_none_or(|place| row.source.path.starts_with(place))
                && matches_tags(&row.tags, &self.samples.filters)
                && sample_matches_query(row, &query)
        });
        let (folders, rows) = folder_contents(
            matching,
            self.settings.sample_grouping,
            &self.samples.browse_path,
        );
        let mut navigation = None;
        let text_editing = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if !rows.is_empty()
            && !text_editing
            && self.plan.is_none()
            && !self.show_warnings
            && !egui::Popup::is_any_open(ctx)
        {
            let (direction, replay, toggle) = ctx.input_mut(|input| {
                if !input.modifiers.is_none() {
                    return (0, false, false);
                }
                let down = input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                let up = input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                (
                    down as isize - up as isize,
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                )
            });
            let current = rows
                .iter()
                .position(|row| self.samples.focus.as_ref() == Some(&row.source.path));
            if direction != 0 {
                navigation = next_preview_index(current, rows.len(), direction);
            } else if let Some(index) = current {
                if toggle && self.samples.playing.is_some() {
                    self.player.stop();
                } else if replay || toggle {
                    self.player.play(rows[index].source.clone(), self.volume);
                }
            }
        }
        let mut choose = navigation.map(|index| rows[index]);
        let mut copy = false;
        let ready = self.ready();
        let selected = self.samples.selected.len();
        let summary = if folders.is_empty() {
            format!("{} shown · {selected} selected", rows.len())
        } else {
            let count = folders.len();
            let noun = if count == 1 { "folder" } else { "folders" };
            format!("{count} {noun} · {selected} selected")
        };
        let compact = ui.available_width() < 380.0;
        if compact {
            muted(ui, &summary);
        }
        ui.horizontal_wrapped(|ui| {
            if !compact {
                muted(ui, &summary);
            }
            copy = action(
                ui,
                ready && !self.samples.selected.is_empty(),
                "Copy selected",
            );
            if ui
                .add_enabled(
                    !self.samples.selected.is_empty(),
                    egui::Button::new("Clear selection"),
                )
                .clicked()
            {
                self.samples.selected.clear();
            }
        });
        if !folders.is_empty() {
            let id = ("sample_groups", grouping.label(), &self.samples.browse_path);
            if let Some(open) = folder_list(ui, id, &folders, "Samples") {
                self.samples.browse_path.push(open);
            }
            if copy {
                self.prepare_sample_copy(ctx);
            }
            return;
        }
        if rows.is_empty() {
            ui.add_space(24.0);
            muted(
                ui,
                if self.settings.sample_folders().next().is_some() {
                    "No matching samples."
                } else {
                    "Add a folder to find samples."
                },
            );
        } else {
            ui.separator();
            ui.horizontal(|ui| {
                caption(ui, "File");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    caption(ui, "Duration");
                });
            });
        }
        const ROW_HEIGHT: f32 = 46.0;
        ScrollArea::vertical()
            .id_salt(("sample_files", grouping.label(), &self.samples.browse_path))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, visible| {
                if let Some(index) = navigation.filter(|index| !visible.contains(index)) {
                    let offset = (index as f32 - visible.start as f32)
                        * (ROW_HEIGHT + ui.spacing().item_spacing.y);
                    let top = ui.max_rect().top() + offset;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), top),
                        egui::vec2(0.0, ROW_HEIGHT),
                    );
                    ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
                }
                for index in visible {
                    let row = rows[index];
                    let path = &row.source.path;
                    let focused = choose
                        .map(|row| &row.source.path)
                        .or(self.samples.focus.as_ref())
                        == Some(path);
                    ui.horizontal(|ui| {
                        let mut marked = self.samples.selected.contains(path);
                        if ui
                            .checkbox(&mut marked, "")
                            .on_hover_text("Select for copying")
                            .changed()
                        {
                            if marked {
                                self.samples.selected.insert(path.clone());
                            } else {
                                self.samples.selected.remove(path);
                            }
                        }
                        let filename = labels::display_name(path);
                        let button = egui::Button::selectable(focused, &filename)
                            .right_text(
                                RichText::new(duration(row.analysis.seconds))
                                    .font(theme::font(Face::Semibold, 14.0)),
                            )
                            .truncate();
                        let response = ui
                            .add_sized([ui.available_width(), ui.spacing().interact_size.y], button)
                            .on_hover_text(filename);
                        if navigation == Some(index) {
                            response
                                .scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                        }
                        if response.clicked() {
                            choose = Some(row);
                        }
                    });
                    ui.horizontal(|ui| {
                        if self.samples.playing.as_ref() == Some(path) {
                            ui.label(
                                RichText::new("Playing")
                                    .font(theme::font(Face::Italic, 12.5))
                                    .color(theme::colors(ui).accent),
                            );
                        }
                        let text = row
                            .tags
                            .iter()
                            .filter(|tag| !tag.starts_with("길이 · "))
                            .take(2)
                            .map(|tag| labels::composed(labels::tag(tag)))
                            .collect::<Vec<_>>()
                            .join("  /  ");
                        ui.add(
                            egui::Label::new(
                                RichText::new(&text).small().color(theme::colors(ui).muted),
                            )
                            .truncate(),
                        )
                        .on_hover_text(text);
                    });
                }
            });
        if let Some(row) = choose {
            let path = row.source.path.clone();
            if !self.samples.manual_preview {
                self.player.play(row.source.clone(), self.volume);
            }
            if self.samples.focus.as_ref() != Some(&path) {
                self.samples.editing_tags = false;
                self.samples.draft_tags = display_tags(&row.tags);
            }
            self.samples.focus = Some(path);
            ctx.request_repaint();
        }
        if copy {
            self.prepare_sample_copy(ctx);
        }
    }

    fn tag_filter_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(180.0);
        let mut groups = BTreeMap::<_, BTreeMap<_, BTreeSet<_>>>::new();
        for tag in self.samples.rows.iter().flat_map(|row| &row.tags) {
            let (category, label, subgroup) = match labels::tag_parts(tag) {
                Some((category, label)) => (category, label, labels::instrument_subgroup(tag)),
                None => ("Custom tags", tag.as_str(), None),
            };
            groups
                .entry(category)
                .or_default()
                .entry(subgroup)
                .or_default()
                .insert((label, tag));
        }
        if groups.is_empty() {
            muted(ui, "No tags available");
        }
        let filters = &mut self.samples.filters;
        let active_folder = &mut self.samples.active_folder;
        for (category, subgroups) in groups {
            egui::CollapsingHeader::new(category).show(ui, |ui| {
                for (subgroup, tags) in subgroups {
                    if let Some(subgroup) = subgroup {
                        egui::CollapsingHeader::new(subgroup).show(ui, |ui| {
                            Self::tag_choices(ui, tags, filters, active_folder);
                        });
                    } else {
                        Self::tag_choices(ui, tags, filters, active_folder);
                    }
                }
            });
        }
    }

    fn tag_choices(
        ui: &mut Ui,
        tags: BTreeSet<(&str, &String)>,
        filters: &mut Vec<String>,
        active_folder: &mut Option<String>,
    ) {
        for (label, tag) in tags {
            let mut selected = matches_tags(filters, std::slice::from_ref(tag));
            if ui
                .checkbox(&mut selected, labels::composed(label))
                .changed()
            {
                if selected {
                    filters.push(tag.clone());
                } else {
                    filters.retain(|filter| key(filter) != key(tag));
                }
                *active_folder = None;
            }
        }
    }

    fn sample_detail(&mut self, ui: &mut Ui) {
        caption(ui, "Preview");
        let mut auto_preview = !self.samples.manual_preview;
        if ui.checkbox(&mut auto_preview, "Auto-preview").changed() {
            self.samples.manual_preview = !auto_preview;
        }
        let playing = self.samples.playing.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(playing, egui::Button::new("Stop")).clicked() {
                self.player.stop();
            }
            if ui
                .add(
                    egui::Slider::new(&mut self.volume, 0.0..=1.0)
                        .show_value(false)
                        .text("Volume"),
                )
                .changed()
            {
                self.player.volume(self.volume);
            }
            ui.label(format!("{:.0}%", self.volume * 100.0));
        });
        if let Some(path) = &self.samples.playing {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("Playing")
                        .font(theme::font(Face::Italic, 12.5))
                        .color(theme::colors(ui).accent),
                );
                ui.label(RichText::new(labels::display_name(path)).small());
            });
        }
        ui.separator();
        let Some(row) = self
            .samples
            .rows
            .iter()
            .find(|row| Some(&row.source.path) == self.samples.focus.as_ref())
            .cloned()
        else {
            muted(ui, "Select a sample or use ↑ and ↓ to preview.");
            return;
        };
        ui.heading(labels::display_name(&row.source.path));
        path_label(ui, &row.source.path);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Duration")
                    .font(theme::font(Face::Light, 14.0))
                    .color(theme::colors(ui).muted),
            );
            ui.label(
                RichText::new(duration(row.analysis.seconds))
                    .font(theme::font(Face::Semibold, 14.0)),
            );
        });
        if let Some(error) = &row.analysis.error {
            ui.label(RichText::new("Sample analysis failed").color(theme::colors(ui).warn))
                .on_hover_text(labels::composed(error.as_str()));
        }
        ui.horizontal_wrapped(|ui| {
            if action(ui, true, "Play") {
                self.player.play(row.source.clone(), self.volume);
            }
            if ui.button("Show in Finder").clicked()
                && let Err(error) = reveal(&row.source)
            {
                self.error(error);
            }
        });
        ui.separator();
        if !self.samples.editing_tags {
            caption(
                ui,
                if row.customized {
                    "Your tags"
                } else {
                    "Suggested tags"
                },
            );
            egui::Grid::new("sample_tag_details")
                .num_columns(2)
                .spacing([16.0, 7.0])
                .min_col_width(84.0)
                .show(ui, |ui| {
                    for tag in &row.tags {
                        let (category, value) = labels::tag_parts(tag).unwrap_or(("Custom", tag));
                        ui.label(
                            RichText::new(category)
                                .font(theme::font(Face::Light, 14.0))
                                .color(theme::colors(ui).muted),
                        );
                        ui.label(
                            RichText::new(labels::composed(value))
                                .font(theme::font(Face::Semibold, 14.0)),
                        );
                        ui.end_row();
                    }
                });
            if ui.button("Edit tags").clicked() {
                self.samples.draft_tags = display_tags(&row.tags);
                self.samples.editing_tags = true;
            }
            return;
        }
        caption(ui, "Edit tags");
        muted(ui, "One tag per line. Press Return to add a line.");
        ui.add(
            egui::TextEdit::multiline(&mut self.samples.draft_tags)
                .desired_rows(row.tags.len().clamp(5, 10))
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            if action(ui, self.ready(), "Save tags") {
                let tags = edited_tags(&self.samples.draft_tags)
                    .into_iter()
                    .map(labels::stored_tag)
                    .collect::<Vec<_>>();
                match self.store.tag_sample(&row.source, &tags) {
                    Ok(()) => {
                        self.change_tags(&row.source, tags, true);
                        self.samples.editing_tags = false;
                        self.status = "Tags saved".into();
                    }
                    Err(error) => self.error(error),
                }
            }
            if ui
                .add_enabled(
                    self.ready() && row.customized,
                    egui::Button::new("Restore suggested tags"),
                )
                .clicked()
            {
                match self.store.reset_sample_tags(&row.source) {
                    Ok(()) => {
                        self.change_tags(&row.source, row.analysis.tags, false);
                        self.samples.editing_tags = false;
                        self.status = "Suggested tags restored".into();
                    }
                    Err(error) => self.error(error),
                }
            }
            if ui.button("Cancel").clicked() {
                self.samples.editing_tags = false;
            }
        });
    }

    fn change_tags(&mut self, source: &Source, tags: Vec<String>, customized: bool) {
        self.samples.draft_tags = display_tags(&tags);
        if let Some(row) = self
            .samples
            .rows
            .iter_mut()
            .find(|row| row.source.path == source.path)
        {
            row.tags = tags;
            row.customized = customized;
        }
    }

    fn prepare_sample_copy(&mut self, ctx: &Context) {
        let sources = self
            .samples
            .rows
            .iter()
            .filter(|row| self.samples.selected.contains(&row.source.path))
            .map(|row| row.source.clone())
            .collect::<Vec<_>>();
        match folder("Choose a destination for sample copies") {
            Ok(Some(destination)) => {
                if self
                    .settings
                    .sample_folders()
                    .any(|root| root.path == destination.path)
                {
                    self.error(anyhow::anyhow!(
                        "Choose a destination outside the sample folders."
                    ));
                    return;
                }
                self.job = Some(Job::start(ctx, "Preparing copy plan", move |cancel, _| {
                    Ok(Outcome::Plan(sample_plan(&sources, destination, cancel)?))
                }));
            }
            Ok(None) => {}
            Err(error) => self.error(error),
        }
    }
}

fn next_preview_index(current: Option<usize>, count: usize, direction: isize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    match current {
        Some(index) => {
            let next = index.saturating_add_signed(direction).min(last);
            (next != index).then_some(next)
        }
        None if direction > 0 => Some(0),
        None if direction < 0 => Some(last),
        None => None,
    }
}

fn folder_contents<'a>(
    rows: impl Iterator<Item = &'a Sample>,
    mode: SampleGrouping,
    folder: &[String],
) -> (Vec<(&'a str, usize)>, Vec<&'a Sample>) {
    let mut folders = BTreeMap::new();
    let mut samples = Vec::new();
    let mut child_names = Vec::new();
    for row in rows {
        child_names.clear();
        let mut in_folder = false;
        sample_folders(mode, &row.tags, |route| {
            if route.len() >= folder.len()
                && route.iter().zip(folder).all(|(part, name)| *part == name)
            {
                if let Some(&child) = route.get(folder.len()) {
                    if !child_names.contains(&child) {
                        child_names.push(child);
                    }
                } else {
                    in_folder = true;
                }
            }
        });
        for &child in &child_names {
            *folders.entry(child).or_insert(0) += 1;
        }
        if in_folder {
            samples.push(row);
        }
    }
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort_by_key(|&(name, _)| (folder_rank(mode, name), name));
    (folders, samples)
}

fn sample_folders<'a>(mode: SampleGrouping, tags: &'a [String], mut visit: impl FnMut(&[&'a str])) {
    let stored_category = match mode {
        SampleGrouping::Instrument => "악기",
        SampleGrouping::Genre => "장르",
        SampleGrouping::Length => "길이",
        SampleGrouping::Brightness => "밝기",
    };
    let mut grouped = false;
    for tag in tags {
        let Some((category, value)) = tag.split_once(" · ") else {
            continue;
        };
        if category != stored_category && !category.eq_ignore_ascii_case(mode.label()) {
            continue;
        }
        let label = labels::tag_parts(tag).map_or(value, |(_, label)| label);
        if label.is_empty() || label == "미분류" || label.eq_ignore_ascii_case("Unclassified") {
            continue;
        }
        if mode == SampleGrouping::Instrument {
            let section = labels::instrument_subgroup(tag).unwrap_or_else(|| {
                labels::instrument_subgroup(&labels::stored_tag(tag.clone()))
                    .unwrap_or("Other instruments")
            });
            visit(&[section, label]);
        } else {
            visit(&[label]);
        }
        grouped = true;
    }
    if !grouped {
        visit(&["Unclassified"]);
    }
}

fn folder_rank(mode: SampleGrouping, name: &str) -> usize {
    let order: &[_] = match mode {
        SampleGrouping::Instrument => &["Drums", "Percussion", "Other instruments"],
        SampleGrouping::Length => &["Under 1 s", "1–8 s", "8 s and up"],
        SampleGrouping::Brightness => &["Dark", "Medium", "Bright"],
        SampleGrouping::Genre => &[],
    };
    if name == "Unclassified" {
        usize::MAX
    } else {
        order
            .iter()
            .position(|item| *item == name)
            .unwrap_or(order.len())
    }
}

fn display_tags(tags: &[String]) -> String {
    tags.iter()
        .map(|tag| labels::composed(labels::tag(tag)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sample_matches_query(sample: &Sample, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let file = key(&name(&sample.source.path));
    let tags: Vec<_> = sample
        .tags
        .iter()
        .map(|tag| key(&labels::tag(tag)))
        .collect();
    query
        .split_whitespace()
        .all(|word| file.contains(word) || tags.iter().any(|tag| tag.contains(word)))
}

fn duration(seconds: Option<f64>) -> String {
    match seconds {
        Some(seconds) if seconds < 1.0 => format!("{seconds:.3} s"),
        Some(seconds) => format!("{seconds:.2} s"),
        None => "Unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_open_into_instrument_subfolders_or_selected_tag_categories() {
        let tags = [
            "악기 · 콩가·봉고".into(),
            "악기 · 킥".into(),
            "장르 · 하우스".into(),
            "길이 · 1–8초".into(),
            "밝기 · 밝음".into(),
        ];
        let routes = |mode, tags| {
            let mut found = Vec::new();
            sample_folders(mode, tags, |path| found.push(path.to_vec()));
            found
        };
        assert_eq!(
            routes(SampleGrouping::Instrument, &tags),
            [vec!["Percussion", "Conga / bongo"], vec!["Drums", "Kick"],]
        );
        assert_eq!(routes(SampleGrouping::Genre, &tags), [vec!["House"]]);
        assert_eq!(routes(SampleGrouping::Length, &tags), [vec!["1–8 s"]]);
        assert_eq!(routes(SampleGrouping::Brightness, &tags), [vec!["Bright"]]);
        assert_eq!(
            routes(SampleGrouping::Instrument, &[]),
            [vec!["Unclassified"]]
        );
    }

    #[test]
    fn arrow_navigation_stays_within_the_visible_sample_list() {
        assert_eq!(next_preview_index(None, 0, 1), None);
        assert_eq!(next_preview_index(None, 3, 1), Some(0));
        assert_eq!(next_preview_index(None, 3, -1), Some(2));
        assert_eq!(next_preview_index(Some(0), 3, 1), Some(1));
        assert_eq!(next_preview_index(Some(1), 3, -1), Some(0));
        assert_eq!(next_preview_index(Some(0), 3, -1), None);
        assert_eq!(next_preview_index(Some(2), 3, 1), None);
        assert_eq!(next_preview_index(Some(0), 3, 12), Some(2));
    }
}
