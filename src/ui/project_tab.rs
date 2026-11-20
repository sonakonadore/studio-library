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
                    |count| update(format!("· 프로젝트 {count}개 찾음")),
                )?;
                Ok(Outcome::Projects(scan))
            },
        ));
    }

    pub(super) fn project_tab(&mut self, ctx: &Context) {
        let grain = self.paper_grain.id();
        left_panel("project_sources", ctx, grain, |ui| {
            self.project_sources(ctx, ui)
        });
        right_panel("project_detail", ctx, grain, |ui| {
            self.project_detail(ctx, ui)
        });
        center(ctx, grain, |ui| self.project_list(ui));
        self.rule_dialog(ctx);
    }

    fn project_sources(&mut self, ctx: &Context, ui: &mut Ui) {
        caption(ui, "Project folders");
        muted(ui, "프로젝트 폴더나 외장 드라이브를 선택하세요.");
        if action(ui, self.ready(), "Choose folder") {
            match folder("프로젝트를 찾을 폴더를 선택하세요.") {
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
                    .on_hover_text("이 폴더를 탐색 대상에서 제외합니다.")
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
                        folder("등록할 프로젝트 폴더를 선택하세요.")
                    } else {
                        file("등록할 프로젝트 파일이나 패키지를 선택하세요.")
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
                    .on_hover_text("이 프로젝트의 등록을 해제합니다.")
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
            muted(ui, "주로 쓰는 DAW와 자동 탐색 규칙을 설정할 수 있어요.");
        }
        if ui
            .add_enabled(self.ready(), egui::Button::new("DAW and detection rules"))
            .clicked()
        {
            self.projects.primary_draft = self.settings.primary.clone().unwrap_or_default();
            self.projects.show_rules = true;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Supported formats").show(ui, |ui| {
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
                        "찾을 폴더의 정확한 이름"
                    } else {
                        "예시 .als"
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
                                self.status = "DAW 설정을 저장했어요.".into();
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
                    muted(ui, "저장된 추가 규칙이 없어요.");
                }
                for (index, rule) in self.settings.rules.clone().into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let pattern = if rule.kind == RuleKind::Extension {
                            format!(".{}", rule.pattern)
                        } else {
                            format!("Folder {}", rule.pattern)
                        };
                        ui.label(format!("{}  ·  {pattern}", rule.daw));
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
        ui.heading("Discovered projects");
        muted(ui, "곡별 묶음, 버전, 원본 위치를 확인할 수 있어요.");
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.projects.query)
                    .hint_text("곡 이름이나 프로젝트 파일명으로 검색")
                    .desired_width(210.0),
            );
            ui.checkbox(&mut self.projects.review_only, "Needs review");
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
            format!("{}개 표시 · 확인할 프로젝트 {total_review}개", rows.len()),
        );
        if rows.is_empty() {
            ui.add_space(24.0);
            if self.projects.rows.is_empty()
                && self.settings.roots.is_empty()
                && self.settings.manual.is_empty()
            {
                ui.label("Choose folder로 프로젝트를 찾을 폴더를 지정하세요.");
            } else {
                muted(ui, "조건에 맞는 프로젝트가 없어요.");
            }
        }
        let mut select = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 54.0, rows.len(), |ui, visible| {
                for (row, edit, review) in &rows[visible] {
                    let selected = self.projects.selected.as_ref() == Some(&row.source.path);
                    let version = labels::version(&edit.version);
                    let text = if edit.group == edit.title {
                        format!("{}   /   {version}", edit.group)
                    } else {
                        format!("{}   /   {}   /   {version}", edit.group, edit.title)
                    };
                    let button =
                        egui::Button::selectable(selected, RichText::new(text).strong()).truncate();
                    if ui.add_sized([ui.available_width(), 28.0], button).clicked() {
                        select = Some((row.source.path.clone(), (*edit).clone()));
                    }
                    ui.horizontal(|ui| {
                        if *review {
                            ui.label(RichText::new("Review").small().color(WARN));
                        }
                        if row.manual {
                            ui.label(RichText::new("Manual").small().color(ACCENT));
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
            caption(ui, "Project details");
            muted(
                ui,
                "목록에서 프로젝트를 선택하세요. 곡 묶음과 복사 옵션을 볼 수 있어요.",
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
        if ui.button("Show in Finder").clicked()
            && let Err(error) = reveal(&row.source)
        {
            self.error(error);
        }
        let review = row.review && !self.settings.edits.contains_key(&path);
        if review {
            ui.label(
                RichText::new("이름이 같은 프로젝트가 있어요. 곡 묶음과 버전을 확인해 주세요.")
                    .color(WARN),
            );
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
        let editing = self.projects.draft != saved;
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
                        self.status = "곡 정보를 저장했어요.".into();
                    }
                }
                Err(error) => self.error(error),
            }
        }
        ui.separator();
        caption(ui, "Copy project");
        muted(ui, projects::media_help(&extension(&path)));
        if row.directory {
            muted(ui, "프로젝트 안의 폴더 구조를 그대로 복사해요.");
        }
        if !row.companions.is_empty() {
            muted(
                ui,
                format!(
                    "함께 복사하는 폴더 · {}",
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
                    "Media와 History도 복사하려면 .song 파일이 있는 폴더를 탐색 위치에 추가하세요.",
                )
                .color(WARN),
            );
        }
        let can_copy = self.ready() && !review && !editing && !missing_fender_folder;
        if action(ui, can_copy, "Plan a copy") {
            match folder("프로젝트를 복사할 폴더를 선택하세요.") {
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
                        "복사할 파일 확인 중",
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
        caption(ui, "Related files");
        muted(ui, "참조 미확인 · 미디어 연결은 DAW에서 확인해 주세요.");
        let related = self.settings.related.get(path).cloned().unwrap_or_default();
        if ui
            .add_enabled(self.ready(), egui::Button::new("Add related file"))
            .clicked()
        {
            match file("추가할 관련 파일을 선택하세요.") {
                Ok(Some(pick)) if pick.directory => {
                    self.error(anyhow::anyhow!("관련 파일을 하나 선택하세요."))
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
                    .on_hover_text("이 파일을 관련 파일 목록에서 뺍니다.")
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
                            .on_hover_text("이 오디오를 관련 파일 목록에 추가합니다.")
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
            self.status = "관련 파일을 저장했어요. 미디어 연결은 DAW에서 확인해 주세요.".into();
        }
    }
}

fn replace_scope(scopes: &mut Vec<Scope>, new: Scope) {
    scopes.retain(|scope| scope.path != new.path);
    scopes.push(new);
}
