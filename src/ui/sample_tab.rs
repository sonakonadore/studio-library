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
            "샘플 탐색 중",
            move |cancel, update| {
                let scan =
                    Store::open(&db)?.scan_samples(&root, &outputs, cancel, |count, decoded| {
                        update(format!("· 발견 {count}개 · 분석 {decoded}개"))
                    })?;
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
        caption(ui, "Splice 다운로드 폴더");
        if let Some(root) = &self.settings.samples {
            path_label(ui, &root.path);
        } else {
            muted(ui, "다운로드한 샘플이 있는 폴더를 지정하세요.");
        }
        if action(
            ui,
            self.ready(),
            if self.settings.samples.is_some() {
                "폴더 변경"
            } else {
                "폴더 선택"
            },
        ) {
            match folder("로컬 Splice 다운로드 폴더 선택") {
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
                egui::Button::new("변경 파일 탐색"),
            )
            .clicked()
        {
            self.scan_samples(ctx);
        }
        if self.settings.samples.is_some()
            && ui
                .add_enabled(self.ready(), egui::Button::new("폴더 등록 해제"))
                .clicked()
            && self.save_settings(|settings| settings.samples = None)
        {
            self.player.stop();
            self.replace_samples(Vec::new());
            self.status = "샘플 탐색 위치 해제 완료".into();
        }
        ui.separator();
        caption(ui, "가상 폴더");
        if ui
            .selectable_label(self.samples.active_folder.is_none(), "전체 샘플")
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
                    .on_hover_text(item.tags.join(", "))
                    .clicked()
                {
                    self.samples.active_folder = Some(item.name);
                    self.samples.filters = item.tags;
                }
            });
        }
        let save_enabled = self.ready() && !self.samples.filters.is_empty();
        if ui
            .add_enabled(
                save_enabled,
                egui::Button::new("현재 태그로 가상 폴더 만들기"),
            )
            .on_disabled_hover_text("샘플 목록에서 태그를 먼저 추가하세요.")
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
                .add_enabled(save_enabled, egui::Button::new("저장"))
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
                    self.error(anyhow::anyhow!(
                        "비어 있거나 이미 사용 중인 가상 폴더 이름입니다."
                    ));
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
        ui.heading("샘플 찾아보기");
        muted(ui, "WAV · AIFF · MP3");
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.samples.query)
                    .hint_text("파일명 검색")
                    .desired_width(210.0),
            );
            let available: BTreeSet<_> = self
                .samples
                .rows
                .iter()
                .flat_map(|row| row.tags.iter())
                .collect();
            egui::ComboBox::from_id_salt("filter_add")
                .selected_text("태그 추가")
                .height(290.0)
                .show_ui(ui, |ui| {
                    if available.is_empty() {
                        muted(ui, "등록된 태그 없음");
                    }
                    for tag in available {
                        if !matches_tags(&self.samples.filters, std::slice::from_ref(tag))
                            && ui.button(tag).clicked()
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
                    if ui.button(format!("{filter} ×")).clicked() {
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
                    "{}개 표시 · {}개 선택",
                    rows.len(),
                    self.samples.selected.len()
                ),
            );
            copy = action(ui, ready && !self.samples.selected.is_empty(), "선택 복사");
            if ui
                .add_enabled(
                    !self.samples.selected.is_empty(),
                    egui::Button::new("선택 해제"),
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
                    "표시할 샘플이 없습니다."
                } else {
                    "Splice 다운로드 폴더를 선택하세요."
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
                            .on_hover_text("복사 목록에 선택")
                            .changed()
                        {
                            if marked {
                                self.samples.selected.insert(path.clone());
                            } else {
                                self.samples.selected.remove(path);
                            }
                        }
                        if ui
                            .add(
                                egui::Button::selectable(
                                    focused,
                                    RichText::new(name(path)).strong(),
                                )
                                .truncate(),
                            )
                            .clicked()
                        {
                            choose = Some((path.clone(), row.tags.join(", ")));
                        }
                    });
                    ui.horizontal(|ui| {
                        if self.samples.playing.as_ref() == Some(path) {
                            ui.label(RichText::new("재생 중").small().color(ACCENT));
                        }
                        let text = format!(
                            "{} · {}",
                            row.analysis
                                .seconds
                                .map_or_else(|| "길이 미확인".into(), |s| format!("{s:.2}초")),
                            row.tags
                                .iter()
                                .take(3)
                                .map(String::as_str)
                                .collect::<Vec<_>>()
                                .join("  /  ")
                        );
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
        caption(ui, "미리듣기");
        let playing = self.samples.playing.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(playing, egui::Button::new("정지")).clicked() {
                self.player.stop();
            }
            if ui
                .add(
                    egui::Slider::new(&mut self.volume, 0.0..=1.0)
                        .show_value(false)
                        .text("볼륨"),
                )
                .changed()
            {
                self.player.volume(self.volume);
            }
            ui.label(format!("{:.0}%", self.volume * 100.0));
        });
        if let Some(path) = &self.samples.playing {
            ui.label(
                RichText::new(format!("재생 중 · {}", name(path)))
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
            muted(ui, "목록에서 샘플 이름을 선택하세요.");
            return;
        };
        ui.heading(name(&row.source.path));
        path_label(ui, &row.source.path);
        if let Some(seconds) = row.analysis.seconds {
            muted(ui, format!("{seconds:.2}초"));
        }
        if let Some(error) = &row.analysis.error {
            ui.label(RichText::new("오디오 분석 실패").color(WARN))
                .on_hover_text(error);
        }
        ui.horizontal_wrapped(|ui| {
            if action(ui, true, "재생") {
                self.player.play(row.source.clone(), self.volume);
            }
            if ui.button("Finder에서 보기").clicked()
                && let Err(error) = reveal(&row.source)
            {
                self.error(error);
            }
        });
        ui.separator();
        caption(
            ui,
            if row.customized {
                "내 태그"
            } else {
                "제안 태그"
            },
        );
        muted(ui, "태그는 쉼표나 줄바꿈으로 구분하세요.");
        ui.add(
            egui::TextEdit::multiline(&mut self.samples.draft_tags)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            if action(ui, self.ready(), "태그 저장") {
                let tags = edited_tags(&self.samples.draft_tags);
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
                    egui::Button::new("제안 태그로 되돌리기"),
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
        self.samples.draft_tags = tags.join(", ");
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
        match folder("선택한 샘플을 복사할 개인 폴더") {
            Ok(Some(destination)) => {
                if self
                    .settings
                    .samples
                    .as_ref()
                    .is_some_and(|root| root.path == destination.path)
                {
                    self.error(anyhow::anyhow!(
                        "Splice 폴더와 다른 개인 폴더를 선택하세요."
                    ));
                    return;
                }
                self.job = Some(Job::start(
                    ctx,
                    "복사 계획 확인 중",
                    move |cancel, _| Ok(Outcome::Plan(sample_plan(&sources, destination, cancel)?)),
                ));
            }
            Ok(None) => {}
            Err(error) => self.error(error),
        }
    }
}
