use super::*;
use std::collections::BTreeSet;

impl Studio {
    pub(super) fn scan_projects(&mut self, ctx: &Context) {
        let settings = self.settings.clone();
        self.job = Some(Job::start(
            ctx,
            "프로젝트 탐색 중",
            move |cancel, update| {
                let scan = projects::scan(
                    &settings.roots,
                    &settings.manual,
                    &settings.rules,
                    &settings.outputs,
                    cancel,
                    |count| update(format!("· 후보 {count}개")),
                )?;
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
        caption(ui, "탐색 위치");
        muted(ui, "프로젝트 폴더나 외장 드라이브를 선택하세요.");
        if action(ui, self.ready(), "폴더 선택") {
            match folder("프로젝트 탐색 폴더 선택") {
                Ok(Some(root)) => {
                    if self.save_settings(|settings| replace_scope(&mut settings.roots, root)) {
                        self.scan_projects(ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.error(error),
            }
        }
        for root in self.settings.roots.clone() {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("×").small())
                    .on_hover_text("탐색 위치 제외")
                    .clicked()
                    && self.save_settings(|settings| {
                        settings.roots.retain(|item| item.path != root.path)
                    })
                {
                    self.scan_projects(ctx);
                }
                path_label(ui, &root.path);
            });
        }
        if ui
            .add_enabled(
                self.ready()
                    && (!self.settings.roots.is_empty() || !self.settings.manual.is_empty()),
                egui::Button::new("다시 탐색"),
            )
            .clicked()
        {
            self.scan_projects(ctx);
        }
        ui.separator();
        caption(ui, "직접 등록");
        ui.horizontal_wrapped(|ui| {
            for (is_folder, label) in [(false, "파일 등록"), (true, "폴더 등록")] {
                if ui
                    .add_enabled(self.ready(), egui::Button::new(label))
                    .clicked()
                {
                    let pick = if is_folder {
                        folder("프로젝트 폴더를 직접 등록")
                    } else {
                        file("프로젝트 파일 또는 패키지를 직접 등록")
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
                    .on_hover_text("직접 등록 해제")
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
        caption(ui, "DAW 설정");
        if let Some(daw) = &self.settings.primary {
            muted(ui, format!("주력 DAW · {daw}"));
        } else {
            muted(ui, "주력 DAW와 프로젝트 확장자를 설정할 수 있습니다.");
        }
        if ui
            .add_enabled(self.ready(), egui::Button::new("주력 DAW 및 탐색 규칙"))
            .clicked()
        {
            self.projects.primary_draft = self.settings.primary.clone().unwrap_or_default();
            self.projects.show_rules = true;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new("자동 탐지 형식").show(ui, |ui| {
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
        egui::Window::new("주력 DAW 및 탐색 규칙")
            .open(&mut open)
            .default_width(470.0)
            .show(ctx, |ui| {
                caption(ui, "주력 DAW");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("daw_preset")
                        .selected_text("기본 형식 선택")
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
                            .hint_text("DAW 이름")
                            .desired_width(180.0),
                    );
                });
                ui.add_space(5.0);
                caption(ui, "탐색 규칙 추가");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.projects.rule_name)
                            .hint_text("DAW 이름")
                            .desired_width(140.0),
                    );
                    ui.selectable_value(&mut self.projects.rule_folder, false, "확장자");
                    ui.selectable_value(&mut self.projects.rule_folder, true, "폴더 이름");
                    let hint = if self.projects.rule_folder {
                        "일치할 폴더 이름"
                    } else {
                        ".als 또는 사용자 확장자"
                    };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.projects.rule_pattern)
                            .hint_text(hint)
                            .desired_width(155.0),
                    );
                });
                if action(ui, self.ready(), "설정 저장") {
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
                                self.status = "DAW 설정 저장 완료".into();
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
                caption(ui, "저장된 추가 규칙");
                if self.settings.rules.is_empty() {
                    muted(ui, "추가 규칙 없음");
                }
                for (index, rule) in self.settings.rules.clone().into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let pattern = if rule.kind == RuleKind::Extension {
                            format!(".{}", rule.pattern)
                        } else {
                            format!("폴더 {}", rule.pattern)
                        };
                        ui.label(format!("{}  ·  {pattern}", rule.daw));
                        if ui
                            .add_enabled(self.ready(), egui::Button::new("삭제").small())
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
        ui.heading("프로젝트 후보");
        muted(ui, "곡 묶음, 버전, 원본 위치를 확인하세요.");
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.projects.query)
                    .hint_text("곡·프로젝트 검색")
                    .desired_width(210.0),
            );
            ui.checkbox(&mut self.projects.review_only, "확인 목록");
        });
        ui.separator();
        let query = key(self.projects.query.trim());
        let mut rows: Vec<_> = self
            .projects
            .rows
            .iter()
            .filter_map(|row| {
                let existing = self.settings.edits.get(&row.source.path);
                let edit = existing.unwrap_or(&row.suggested);
                let review = row.review && existing.is_none();
                let matches = query.is_empty()
                    || [&edit.title, &edit.group, &name(&row.source.path)]
                        .into_iter()
                        .any(|field| key(field).contains(&query));
                (matches && (!self.projects.review_only || review)).then_some((row, edit, review))
            })
            .collect();
        rows.sort_by_cached_key(|(row, edit, _)| {
            (
                key(&edit.group),
                key(&edit.version),
                row.source.path.clone(),
            )
        });
        let total_review = self
            .projects
            .rows
            .iter()
            .filter(|item| item.review && !self.settings.edits.contains_key(&item.source.path))
            .count();
        muted(
            ui,
            format!("{}개 표시 · 확인 필요 {total_review}개", rows.len()),
        );
        if rows.is_empty() {
            ui.add_space(24.0);
            if self.projects.rows.is_empty()
                && self.settings.roots.is_empty()
                && self.settings.manual.is_empty()
            {
                ui.label("‘폴더 선택’으로 프로젝트 후보를 찾아보세요.");
            } else {
                muted(ui, "표시할 프로젝트가 없습니다.");
            }
        }
        let mut select = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 54.0, rows.len(), |ui, visible| {
                for (row, edit, review) in &rows[visible] {
                    let selected = self.projects.selected.as_ref() == Some(&row.source.path);
                    let text = if edit.group == edit.title {
                        format!("{}   /   {}", edit.group, edit.version)
                    } else {
                        format!("{}   /   {}   /   {}", edit.group, edit.title, edit.version)
                    };
                    let button =
                        egui::Button::selectable(selected, RichText::new(text).strong()).truncate();
                    if ui.add_sized([ui.available_width(), 28.0], button).clicked() {
                        select = Some((row.source.path.clone(), (*edit).clone()));
                    }
                    ui.horizontal(|ui| {
                        if *review {
                            ui.label(RichText::new("확인 필요").small().color(WARN));
                        }
                        if row.manual {
                            ui.label(RichText::new("직접 등록").small().color(ACCENT));
                        }
                        path_label(ui, &row.source.path);
                    });
                }
            });
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
            caption(ui, "프로젝트 상세");
            muted(
                ui,
                "후보를 선택하면 곡 묶음과 복사 대상을 확인할 수 있습니다.",
            );
            return;
        };
        let path = row.source.path.clone();
        let saved = self
            .settings
            .edits
            .get(&path)
            .unwrap_or(&row.suggested)
            .clone();
        if self.projects.draft.title.is_empty() {
            self.projects.draft = saved.clone();
        }
        caption(ui, &row.daw);
        ui.heading(&saved.title);
        path_label(ui, &path);
        if ui.button("Finder에서 보기").clicked()
            && let Err(error) = reveal(&row.source)
        {
            self.error(error);
        }
        let review = row.review && !self.settings.edits.contains_key(&path);
        if review {
            ui.label(
                RichText::new("같은 제목의 후보가 있습니다. 묶음과 버전을 확인해 주세요.")
                    .color(WARN),
            );
        }
        ui.separator();
        for (label, value) in [
            ("곡 이름", &mut self.projects.draft.title),
            ("버전", &mut self.projects.draft.version),
            ("곡 묶음", &mut self.projects.draft.group),
        ] {
            ui.label(label);
            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
        }
        let editing = self.projects.draft != saved;
        if ui
            .add_enabled(
                self.ready() && (editing || review),
                egui::Button::new(if review {
                    "묶음 확인 및 저장"
                } else {
                    "곡 정보 저장"
                }),
            )
            .clicked()
        {
            let draft = self.projects.draft.clone();
            let validation = (|| -> Result<ProjectEdit> {
                Ok(ProjectEdit {
                    title: safe_component(&draft.title)?.into(),
                    version: safe_component(&draft.version)?.into(),
                    group: safe_component(&draft.group)?.into(),
                })
            })();
            match validation {
                Ok(edit) => {
                    self.projects.draft = edit.clone();
                    if self.save_settings(|settings| {
                        settings.edits.insert(path.clone(), edit);
                    }) {
                        self.status = "곡 정보 저장 완료".into();
                    }
                }
                Err(error) => self.error(error),
            }
        }
        ui.separator();
        caption(ui, "프로젝트 복사");
        muted(ui, projects::media_help(&extension(&path)));
        if row.directory {
            muted(ui, "프로젝트 폴더 내부 구조를 보존합니다.");
        }
        if !row.companions.is_empty() {
            muted(
                ui,
                format!(
                    "함께 복사 · {}",
                    row.companions
                        .iter()
                        .map(|entry| name(&entry.path))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        let missing_fender_folder = extension(&path) == "song" && !row.source.scope.directory;
        if missing_fender_folder {
            ui.label(
                RichText::new(
                    "Media·History를 보존하려면 .song이 있는 폴더를 탐색 위치에 추가하세요.",
                )
                .color(WARN),
            );
        }
        let can_copy = self.ready() && !review && !editing && !missing_fender_folder;
        if action(ui, can_copy, "대상 선택 및 복사 계획") {
            match folder("프로젝트 복사 대상 폴더 선택") {
                Ok(Some(destination)) => {
                    let related = self
                        .settings
                        .related
                        .get(&path)
                        .cloned()
                        .unwrap_or_default();
                    let project = row.clone();
                    self.job = Some(Job::start(
                        ctx,
                        "복사 계획 확인 중",
                        move |cancel, _| {
                            Ok(Outcome::Plan(project_plan(
                                &project,
                                &saved,
                                &related,
                                destination,
                                cancel,
                            )?))
                        },
                    ));
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
        caption(ui, "관련 파일");
        muted(ui, "참조 미확인 · DAW에서 미디어 연결을 확인하세요.");
        let related = self.settings.related.get(path).cloned().unwrap_or_default();
        if ui
            .add_enabled(self.ready(), egui::Button::new("관련 파일 추가"))
            .clicked()
        {
            match file("관련 파일 직접 선택") {
                Ok(Some(pick)) if pick.directory => {
                    self.error(anyhow::anyhow!("관련 파일 하나를 선택하세요."))
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
                    .on_hover_text("관련 파일 지정 해제")
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
        egui::CollapsingHeader::new(format!("근처 오디오 후보 · {}개", project.nearby.len())).show(
            ui,
            |ui| {
                for source in &project.nearby {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.ready() && !selected.contains(&source.path),
                                egui::Button::new("+").small(),
                            )
                            .on_hover_text("관련 파일로 추가")
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
            self.status = "관련 파일 저장 완료 · 참조 미확인".into();
        }
    }
}

fn replace_scope(scopes: &mut Vec<Scope>, new: Scope) {
    scopes.retain(|scope| scope.path != new.path);
    scopes.push(new);
}
