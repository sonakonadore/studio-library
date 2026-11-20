use super::*;
use std::collections::BTreeMap;
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
            "샘플 탐색 중",
            move |cancel, update| {
                let scan =
                    Store::open(&db)?.scan_samples(&root, &outputs, cancel, |count, decoded| {
                        update(format!("· {count}개 찾음 · {decoded}개 분석"))
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
            muted(ui, "Splice 다운로드 폴더 선택");
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
            match folder("Splice 다운로드 폴더 선택") {
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
            self.status = "샘플 폴더 등록 해제".into();
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
                    .on_hover_text("가상 폴더 삭제")
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
            .on_disabled_hover_text("Filter by tag에서 태그 선택")
            .clicked()
        {
            self.samples.show_folder_form = !self.samples.show_folder_form;
        }
        if self.samples.show_folder_form {
            ui.add(
                egui::TextEdit::singleline(&mut self.samples.folder_name)
                    .hint_text("가상 폴더 이름")
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
                    self.error(anyhow::anyhow!("중복 없는 가상 폴더 이름 입력"));
                } else if self.save_settings(|settings| {
                    settings.virtual_folders.push(VirtualFolder {
                        name: name.clone(),
                        tags,
                    })
                }) {
                    self.samples.active_folder = Some(name);
                    self.samples.folder_name.clear();
                    self.samples.show_folder_form = false;
                    self.status = "가상 폴더 저장 완료".into();
                }
            }
        }
    }

    fn sample_list(&mut self, ctx: &Context, ui: &mut Ui) {
        ui.heading("Browse samples");
        muted(ui, "WAV · AIFF · MP3");
        ui.horizontal_wrapped(|ui| {
            search_field(ui, &mut self.samples.query, "파일명 검색");
            egui::ComboBox::from_id_salt("filter_add")
                .selected_text("Filter by tag")
                .height(290.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    self.tag_filter_menu(ui);
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
        let mut navigation = None;
        let text_editing = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if !text_editing
            && self.plan.is_none()
            && !self.show_warnings
            && !egui::Popup::is_any_open(ctx)
        {
            let direction = ctx.input_mut(|input| {
                if !input.modifiers.is_none() {
                    return 0;
                }
                let down = input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                let up = input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                down as isize - up as isize
            });
            if direction != 0 {
                let current = rows
                    .iter()
                    .position(|row| self.samples.focus.as_ref() == Some(&row.source.path));
                navigation = next_preview_index(current, rows.len(), direction);
            }
        }
        let mut choose = navigation.map(|index| rows[index]);
        let mut copy = false;
        let ready = self.ready();
        let summary = format!(
            "{}개 표시 · {}개 선택",
            rows.len(),
            self.samples.selected.len()
        );
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
        if rows.is_empty() {
            ui.add_space(24.0);
            muted(
                ui,
                if self.settings.samples.is_some() {
                    "조건에 맞는 샘플 없음"
                } else {
                    "Choose folder에서 Splice 다운로드 폴더 지정"
                },
            );
        }
        const ROW_HEIGHT: f32 = 55.0;
        ScrollArea::vertical()
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
                            .on_hover_text("복사 대상 선택")
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
                                let button = ui
                                    .add(
                                        egui::Button::selectable(
                                            focused,
                                            RichText::new(name(path)).strong(),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(name(path));
                                if navigation == Some(index) {
                                    button.scroll_to_me_animation(
                                        None,
                                        egui::style::ScrollAnimation::none(),
                                    );
                                }
                                if button.clicked() {
                                    choose = Some(row);
                                }
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(duration(row.analysis.seconds))
                                    .size(14.0)
                                    .color(TEXT),
                            )
                            .on_hover_text("재생 길이");
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
        if let Some(row) = choose {
            let path = row.source.path.clone();
            let tags = display_tags(&row.tags);
            self.player.play(row.source.clone(), self.volume);
            self.samples.focus = Some(path);
            self.samples.draft_tags = tags;
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
            muted(ui, "표시할 태그 없음");
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
            if ui.checkbox(&mut selected, label).changed() {
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
            muted(ui, "샘플 클릭 또는 ↑/↓로 미리듣기");
            return;
        };
        ui.heading(name(&row.source.path));
        path_label(ui, &row.source.path);
        muted(ui, format!("Duration  {}", duration(row.analysis.seconds)));
        if let Some(error) = &row.analysis.error {
            ui.label(RichText::new("샘플 분석 실패").color(WARN))
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
        muted(ui, "쉼표/줄바꿈으로 태그 구분");
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
                        self.status = "태그 저장 완료".into();
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
                        self.status = "제안 태그 복원 완료".into();
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
        match folder("샘플 복사 대상 폴더 선택") {
            Ok(Some(destination)) => {
                if self
                    .settings
                    .samples
                    .as_ref()
                    .is_some_and(|root| root.path == destination.path)
                {
                    self.error(anyhow::anyhow!("Splice 폴더 외의 개인 폴더 선택"));
                    return;
                }
                self.job = Some(Job::start(
                    ctx,
                    "복사할 파일 확인 중",
                    move |cancel, _| Ok(Outcome::Plan(sample_plan(&sources, destination, cancel)?)),
                ));
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

#[cfg(test)]
mod tests {
    use super::next_preview_index;

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
