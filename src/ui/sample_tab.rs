use super::*;
use studio_library::catalog::Sample;

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
            .focus
            .as_ref()
            .is_some_and(|path| !paths.contains(path))
        {
            self.samples.focus = None;
        }
    }

    fn scan_samples(&mut self, ctx: &Context) {
        let Some(root) = self.settings.samples.clone() else {
            return;
        };
        let db = self.db.clone();
        let outputs = self.settings.outputs.clone();
        self.job = Some(Job::start(
            ctx,
            "Scanning samples",
            move |cancel, update| {
                let scan =
                    Store::open(&db)?.scan_samples(&root, &outputs, cancel, |count, decoded| {
                        update(format!("· {count} found · {decoded} analyzed"))
                    })?;
                Ok(Outcome::Samples(scan))
            },
        ));
    }

    pub(super) fn sample_tab(&mut self, ctx: &Context) {
        let grain = self.paper_grain.id();
        left_panel("sample_sources", ctx, grain, |ui| {
            self.sample_sources(ctx, ui)
        });
        right_panel("sample_detail", ctx, grain, |ui| self.sample_detail(ui));
        center(ctx, grain, |ui| self.sample_list(ctx, ui));
    }

    fn sample_sources(&mut self, ctx: &Context, ui: &mut Ui) {
        caption(ui, "Splice download folder");
        if let Some(root) = &self.settings.samples {
            path_label(ui, &root.path);
        } else {
            muted(ui, "Choose the folder with your downloaded samples.");
        }
        if action(
            ui,
            self.ready(),
            if self.settings.samples.is_some() {
                "Change folder"
            } else {
                "Choose folder"
            },
        ) {
            match folder("Choose a local Splice folder") {
                Ok(Some(scope)) => {
                    if self.save_settings(|settings| settings.samples = Some(scope)) {
                        self.player.stop();
                        self.samples.selected.clear();
                        self.samples.focus = None;
                        self.samples.rows.clear();
                        self.scan_samples(ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        if ui
            .add_enabled(
                self.ready() && self.settings.samples.is_some(),
                egui::Button::new("Scan for changes"),
            )
            .clicked()
        {
            self.scan_samples(ctx);
        }
        if self.settings.samples.is_some()
            && ui
                .add_enabled(self.ready(), egui::Button::new("Remove folder"))
                .clicked()
            && self.save_settings(|settings| settings.samples = None)
        {
            self.player.stop();
            self.replace_samples(Vec::new());
            self.status = "Sample folder removed".into();
        }
        ui.separator();
        caption(ui, "Virtual folders");
        if ui
            .selectable_label(self.samples.active_folder.is_none(), "All samples")
            .clicked()
        {
            self.samples.active_folder = None;
            self.samples.filters.clear();
        }
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
                    .on_hover_text("Remove virtual folder")
                    .clicked()
                    && self.save_settings(|settings| {
                        settings.virtual_folders.remove(index);
                    })
                    && self.samples.active_folder.as_ref() == Some(&item.name)
                {
                    self.samples.active_folder = None;
                    self.samples.filters.clear();
                }
                if ui
                    .add(
                        egui::Button::selectable(
                            self.samples.active_folder.as_ref() == Some(&item.name),
                            &item.name,
                        )
                        .truncate(),
                    )
                    .on_hover_text(display_tags(&item.tags))
                    .clicked()
                {
                    self.samples.active_folder = Some(item.name);
                    self.samples.filters = item.tags;
                }
            });
        }
        let save_enabled = self.ready() && !self.samples.filters.is_empty();
        if ui
            .add_enabled(save_enabled, egui::Button::new("Save filters as folder"))
            .on_disabled_hover_text("Add a tag filter first.")
            .clicked()
        {
            self.samples.show_folder_form = !self.samples.show_folder_form;
        }
        if self.samples.show_folder_form {
            ui.add(
                egui::TextEdit::singleline(&mut self.samples.folder_name)
                    .hint_text("Folder name")
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
                    self.error(anyhow::anyhow!("Enter a unique folder name."));
                } else if self.save_settings(|settings| {
                    settings.virtual_folders.push(VirtualFolder {
                        name: name.clone(),
                        tags,
                    })
                }) {
                    self.samples.active_folder = Some(name);
                    self.samples.folder_name.clear();
                    self.samples.show_folder_form = false;
                    self.status = "Virtual folder saved".into();
                }
            }
        }
    }

    fn sample_list(&mut self, ctx: &Context, ui: &mut Ui) {
        ui.heading("Browse samples");
        muted(ui, "WAV · AIFF · MP3");
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.samples.query)
                    .hint_text("Search filenames")
                    .desired_width(210.0),
            );
            let available: BTreeSet<_> = self
                .samples
                .rows
                .iter()
                .flat_map(|row| row.tags.iter())
                .collect();
            egui::ComboBox::from_id_salt("filter_add")
                .selected_text("Filter by tag")
                .height(290.0)
                .show_ui(ui, |ui| {
                    if available.is_empty() {
                        muted(ui, "No tags available");
                    }
                    for tag in available {
                        if !matches_tags(&self.samples.filters, std::slice::from_ref(tag))
                            && ui.button(labels::tag(tag).into_owned()).clicked()
                        {
                            self.samples.filters.push(tag.clone());
                            self.samples.active_folder = None;
                        }
                    }
                });
        });
        if !self.samples.filters.is_empty() {
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (index, filter) in self.samples.filters.iter().enumerate() {
                    if ui.button(format!("{} ×", labels::tag(filter))).clicked() {
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
        let query = key(self.samples.query.trim());
        let rows: Vec<_> = self
            .samples
            .rows
            .iter()
            .filter(|row| {
                key(&name(&row.source.path)).contains(&query)
                    && matches_tags(&row.tags, &self.samples.filters)
            })
            .collect();
        let mut choose = None;
        let mut copy = false;
        let ready = self.ready();
        ui.horizontal_wrapped(|ui| {
            muted(
                ui,
                format!(
                    "{} shown · {} selected",
                    rows.len(),
                    self.samples.selected.len()
                ),
            );
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
        if rows.is_empty() {
            ui.add_space(24.0);
            muted(
                ui,
                if self.settings.samples.is_some() {
                    "No matching samples."
                } else {
                    "Choose your Splice download folder."
                },
            );
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 55.0, rows.len(), |ui, visible| {
                for row in &rows[visible] {
                    let path = &row.source.path;
                    let focused = self.samples.focus.as_ref() == Some(path);
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
                        let name_width = (ui.available_width() - 84.0).max(40.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(name_width, ui.spacing().interact_size.y),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add(
                                        egui::Button::selectable(
                                            focused,
                                            RichText::new(name(path)).strong(),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(name(path))
                                    .clicked()
                                {
                                    choose = Some((path.clone(), display_tags(&row.tags)));
                                }
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(duration(row.analysis.seconds))
                                    .size(14.0)
                                    .color(TEXT),
                            )
                            .on_hover_text("Duration");
                        });
                    });
                    ui.horizontal(|ui| {
                        if self.samples.playing.as_ref() == Some(path) {
                            ui.label(RichText::new("Playing").small().color(ACCENT));
                        }
                        let text = row
                            .tags
                            .iter()
                            .filter(|tag| !tag.starts_with("길이 · "))
                            .take(2)
                            .map(|tag| labels::tag(tag))
                            .collect::<Vec<_>>()
                            .join("  /  ");
                        ui.add(
                            egui::Label::new(RichText::new(&text).small().color(MUTED)).truncate(),
                        )
                        .on_hover_text(text);
                    });
                }
            });
        if let Some((path, tags)) = choose {
            self.samples.focus = Some(path);
            self.samples.draft_tags = tags;
        }
        if copy {
            self.prepare_sample_copy(ctx);
        }
    }

    fn sample_detail(&mut self, ui: &mut Ui) {
        caption(ui, "Preview");
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
            ui.label(
                RichText::new(format!("Playing · {}", name(path)))
                    .small()
                    .color(ACCENT),
            );
        }
        ui.separator();
        let Some(row) = self
            .samples
            .rows
            .iter()
            .find(|row| Some(&row.source.path) == self.samples.focus.as_ref())
            .cloned()
        else {
            muted(ui, "Select a sample to preview it.");
            return;
        };
        ui.heading(name(&row.source.path));
        path_label(ui, &row.source.path);
        muted(ui, format!("Duration  {}", duration(row.analysis.seconds)));
        if let Some(error) = &row.analysis.error {
            ui.label(RichText::new("Audio analysis failed").color(WARN))
                .on_hover_text(error);
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
        caption(
            ui,
            if row.customized {
                "Your tags"
            } else {
                "Suggested tags"
            },
        );
        muted(ui, "Separate tags with commas or line breaks.");
        ui.add(
            egui::TextEdit::multiline(&mut self.samples.draft_tags)
                .desired_rows(4)
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
                        self.status = "Suggested tags restored".into();
                    }
                    Err(error) => self.error(error),
                }
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
        match folder("Choose a destination for the selected samples") {
            Ok(Some(destination)) => {
                if self
                    .settings
                    .samples
                    .as_ref()
                    .is_some_and(|root| root.path == destination.path)
                {
                    self.error(anyhow::anyhow!(
                        "Choose a folder outside the selected Splice location."
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

fn display_tags(tags: &[String]) -> String {
    tags.iter()
        .map(|tag| labels::tag(tag))
        .collect::<Vec<_>>()
        .join(", ")
}

fn duration(seconds: Option<f64>) -> String {
    match seconds {
        Some(seconds) if seconds < 1.0 => format!("{seconds:.3} s"),
        Some(seconds) => format!("{seconds:.2} s"),
        None => "Unknown".into(),
    }
}
