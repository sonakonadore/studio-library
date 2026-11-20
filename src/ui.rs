use crate::jobs::{Event, Job, Outcome};
use crate::player::{Playback, Player};
use anyhow::Result;
use eframe::egui::{self, Context, Frame, RichText, ScrollArea, Ui};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use studio_library::{
    catalog::{Settings, Store, VirtualFolder, database_path, edited_tags, matches_tags},
    copying::{Plan, project_plan, sample_plan},
    projects::{self, FORMATS, Project, ProjectEdit, Rule, RuleKind},
    scope::{Cancelled, Scope, Source, extension, key, name, safe_component},
};

mod labels;
mod project_tab;
mod sample_tab;

mod theme;

use theme::{ACCENT, BG, BUTTON_ORANGE, GOLD, LINE, MUTED, RAISED, RULE, SURFACE, TEXT, WARN};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Projects,
    Samples,
}

#[derive(Default)]
struct ProjectView {
    rows: Vec<Project>,
    selected: Option<PathBuf>,
    draft: ProjectEdit,
    query: String,
    review_only: bool,
    show_rules: bool,
    rule_name: String,
    rule_pattern: String,
    rule_folder: bool,
    primary_draft: String,
}

#[derive(Default)]
struct SampleView {
    rows: Vec<studio_library::catalog::Sample>,
    focus: Option<PathBuf>,
    selected: BTreeSet<PathBuf>,
    query: String,
    filters: Vec<String>,
    active_folder: Option<String>,
    folder_name: String,
    show_folder_form: bool,
    draft_tags: String,
    playing: Option<PathBuf>,
}

pub struct Studio {
    db: PathBuf,
    paper_grain: egui::TextureHandle,
    store: Store,
    settings: Settings,
    tab: Tab,
    projects: ProjectView,
    samples: SampleView,
    job: Option<Job>,
    plan: Option<Plan>,
    status: String,
    warnings: Vec<String>,
    show_warnings: bool,
    player: Player,
    volume: f32,
    close_after_job: bool,
}

impl Studio {
    pub fn new(ctx: &Context) -> Result<Self> {
        theme::style(ctx);
        let paper_grain = theme::grain(ctx)?;
        let db = database_path()?;
        let store = Store::open(&db)?;
        let settings = store.settings()?;
        let rows = settings
            .samples
            .as_ref()
            .map(|root| store.samples(root))
            .transpose()?
            .unwrap_or_default();
        let mut app = Self {
            db,
            paper_grain,
            store,
            settings,
            tab: Tab::Projects,
            projects: ProjectView::default(),
            samples: SampleView {
                rows,
                ..Default::default()
            },
            job: None,
            plan: None,
            status: "준비 완료".into(),
            warnings: Vec::new(),
            show_warnings: false,
            player: Player::new(ctx.clone()),
            volume: 0.65,
            close_after_job: false,
        };
        if !app.settings.roots.is_empty() || !app.settings.manual.is_empty() {
            app.scan_projects(ctx);
        }
        Ok(app)
    }

    fn ready(&self) -> bool {
        self.job.is_none() && self.plan.is_none()
    }

    fn save_settings(&mut self, update: impl FnOnce(&mut Settings)) -> bool {
        let mut settings = self.settings.clone();
        update(&mut settings);
        match self.store.save_settings(&settings) {
            Ok(()) => {
                self.settings = settings;
                true
            }
            Err(error) => {
                self.error(error);
                false
            }
        }
    }

    fn error(&mut self, error: anyhow::Error) {
        self.status = if error.is::<Cancelled>() {
            "작업 취소".into()
        } else {
            format!("실패 · {error:#}")
        };
    }

    fn poll(&mut self, ctx: &Context) {
        let mut playback_error = None;
        for event in self.player.events() {
            match event {
                Playback::Playing(path) => self.samples.playing = Some(path),
                Playback::Stopped => self.samples.playing = None,
                Playback::Error(error) => {
                    self.samples.playing = None;
                    playback_error = Some(error);
                }
            }
        }
        if let Some(error) = playback_error {
            self.status = error;
        }
        let Some(job) = &mut self.job else {
            return;
        };
        let result = loop {
            match job.events.try_recv() {
                Ok(Event::Progress(progress)) => job.progress = progress,
                Ok(Event::Done(outcome)) => break outcome,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    break Err(anyhow::anyhow!("작업 중단 · 다시 시도"));
                }
            }
        };
        self.job = None;
        self.warnings.clear();
        match result {
            Ok(Outcome::Projects(scan)) => {
                self.status = format!(
                    "탐색 완료 · 프로젝트 {}개 · 건너뛴 링크 {}개",
                    scan.projects.len(),
                    scan.skipped
                );
                self.warnings = scan.warnings;
                self.projects.rows = scan.projects;
                if self.projects.selected.as_ref().is_some_and(|path| {
                    !self
                        .projects
                        .rows
                        .iter()
                        .any(|row| &row.source.path == path)
                }) {
                    self.projects.selected = None;
                }
            }
            Ok(Outcome::Samples(scan)) => {
                self.status = format!(
                    "탐색 완료 · 샘플 {}개 · 새로 분석 {}개 · 건너뛴 항목 {}개",
                    scan.rows.len(),
                    scan.decoded,
                    scan.skipped
                );
                self.warnings = scan.warnings;
                self.replace_samples(scan.rows);
            }
            Ok(Outcome::Plan(plan)) => {
                self.status = "원본, 복사 위치 확인".into();
                self.plan = Some(plan);
            }
            Ok(Outcome::Copy(report)) => {
                let message = report.message();
                if report.published.is_empty()
                    || self.save_settings(|settings| {
                        settings.outputs.extend(report.published);
                        settings.outputs.sort_unstable();
                        settings.outputs.dedup();
                    })
                {
                    self.status = message;
                } else {
                    self.status = format!("{message} · 복사 위치 저장 실패 · {}", self.status);
                }
            }
            Err(error) => self.error(error),
        }
        if self.close_after_job {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn header(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::new()
                    .fill(GOLD)
                    .inner_margin(egui::Margin::symmetric(27, 17))
                    .stroke(egui::Stroke::new(1.0_f32, LINE)),
            )
            .show(ctx, |ui| {
                let grain = ui.painter().add(egui::Shape::Noop);
                ui.horizontal(|ui| {
                    theme::mark(ui);
                    ui.add_space(5.0);
                    ui.label(RichText::new("Studio Library").font(theme::display_font(37.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (tab, label) in [(Tab::Samples, "Samples"), (Tab::Projects, "Projects")]
                        {
                            let selected = self.tab == tab;
                            let label = RichText::new(label)
                                .font(theme::display_font(14.0))
                                .color(TEXT);
                            let button = egui::Button::new(label).fill(if selected {
                                BUTTON_ORANGE
                            } else {
                                RAISED
                            });
                            if ui.add_sized([112.0, 34.0], button).clicked() {
                                self.tab = tab;
                            }
                        }
                    });
                });
                let rect = ui.min_rect().expand2(egui::vec2(27.0, 17.0));
                ui.painter()
                    .set(grain, theme::paper(ui, self.paper_grain.id(), rect));
            });
    }

    fn footer(&mut self, ctx: &Context) {
        egui::TopBottomPanel::bottom("status")
            .min_height(42.0)
            .frame(
                Frame::new()
                    .fill(SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, LINE))
                    .inner_margin(egui::Margin::symmetric(23, 10)),
            )
            .show(ctx, |ui| {
                let grain = ui.painter().add(egui::Shape::Noop);
                ui.horizontal(|ui| {
                    if let Some(job) = &self.job {
                        ui.spinner();
                        let cancelled = job.cancel.load(Ordering::Relaxed);
                        let label = if cancelled {
                            "취소 중…"
                        } else {
                            job.label
                        };
                        ui.label(RichText::new(format!("{label} {}", job.progress)).color(ACCENT));
                        if ui
                            .add_enabled(!cancelled, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        let color = if self.status.contains("실패") || self.status.contains("취소")
                        {
                            WARN
                        } else {
                            MUTED
                        };
                        ui.add(
                            egui::Label::new(RichText::new(&self.status).color(color)).truncate(),
                        )
                        .on_hover_text(&self.status);
                        if !self.warnings.is_empty()
                            && ui
                                .button(format!("Warnings ({})", self.warnings.len()))
                                .clicked()
                        {
                            self.show_warnings = true;
                        }
                    }
                });
                let rect = ui.min_rect().expand2(egui::vec2(23.0, 10.0));
                ui.painter()
                    .set(grain, theme::paper(ui, self.paper_grain.id(), rect));
            });
    }

    fn dialogs(&mut self, ctx: &Context) {
        if self.show_warnings {
            egui::Window::new("Scan warnings")
                .open(&mut self.show_warnings)
                .default_width(540.0)
                .show(ctx, |ui| {
                    ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        for warning in &self.warnings {
                            ui.label(warning);
                        }
                    });
                });
        }
        if let Some(plan) = self.plan.take() {
            let mut approve = false;
            let mut decline = false;
            egui::Modal::new(egui::Id::new("approved_copy")).show(ctx, |ui| {
                ui.set_width(650.0);
                ui.heading("Review copy plan");
                muted(ui, "원본 보존 · 아래 위치에 복사");
                ui.label(format!("Destination  {}", plan.target().display()));
                ui.label(
                    RichText::new(format!("Estimated size  {}", byte_count(plan.bytes())))
                        .color(ACCENT),
                );
                if plan.skipped > 0 {
                    ui.label(
                        RichText::new(format!("링크/특수 파일 {}개 건너뜀", plan.skipped))
                            .color(WARN),
                    );
                }
                ui.separator();
                ScrollArea::vertical().max_height(360.0).show_rows(
                    ui,
                    52.0,
                    plan.entry_count(),
                    |ui, range| {
                        for item in plan.previews(range) {
                            let source = format!("Source  {}", item.source.display());
                            ui.add(egui::Label::new(RichText::new(&source).strong()).truncate())
                                .on_hover_text(source);
                            let target = format!(
                                "Destination  {}  ·  {}",
                                item.target.display(),
                                if item.directory {
                                    "Folder".into()
                                } else {
                                    byte_count(item.bytes)
                                }
                            );
                            ui.add(
                                egui::Label::new(RichText::new(&target).color(MUTED)).truncate(),
                            )
                            .on_hover_text(target);
                            ui.add_space(5.0);
                        }
                    },
                );
                ui.separator();
                ui.horizontal(|ui| {
                    approve = action(ui, true, "Confirm copy");
                    decline = ui.button("Close").clicked();
                });
            });
            if approve {
                self.job = Some(Job::start(ctx, "복사 중", move |cancel, update| {
                    let report = plan.execute(cancel, |done, total| {
                        update(format!("{} / {}", byte_count(done), byte_count(total)));
                    });
                    Ok(Outcome::Copy(report))
                }));
            } else if decline {
                self.status = "복사 취소".into();
            } else {
                self.plan = Some(plan);
            }
        }
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|input| input.viewport().close_requested())
            && let Some(job) = &self.job
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            job.cancel.store(true, Ordering::Relaxed);
            self.close_after_job = true;
        }
        self.header(ctx);
        self.footer(ctx);
        match self.tab {
            Tab::Projects => self.project_tab(ctx),
            Tab::Samples => self.sample_tab(ctx),
        }
        self.dialogs(ctx);
    }
}

fn left_panel(id: &'static str, ctx: &Context, grain: egui::TextureId, add: impl FnOnce(&mut Ui)) {
    egui::SidePanel::left(id)
        .exact_width(230.0)
        .resizable(false)
        .frame(Frame::new().fill(GOLD).inner_margin(egui::Margin::same(20)))
        .show(ctx, |ui| {
            ui.painter().add(theme::paper(ui, grain, ui.clip_rect()));
            ScrollArea::vertical().show(ui, add);
        });
}

fn right_panel(id: &'static str, ctx: &Context, grain: egui::TextureId, add: impl FnOnce(&mut Ui)) {
    let max_width = (ctx.content_rect().width() - 230.0 - 348.0).clamp(300.0, 480.0);
    egui::SidePanel::right(id)
        .default_width(322.0)
        .width_range(300.0..=max_width)
        .resizable(true)
        .frame(Frame::new().fill(GOLD).inner_margin(egui::Margin::same(20)))
        .show(ctx, |ui| {
            ui.set_min_width(ui.max_rect().width());
            ui.painter().add(theme::paper(ui, grain, ui.clip_rect()));
            ScrollArea::vertical().show(ui, add);
        });
}

fn center(ctx: &Context, grain: egui::TextureId, add: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(BG).inner_margin(egui::Margin::same(20)))
        .show(ctx, |ui| {
            ui.painter().add(theme::paper(ui, grain, ui.clip_rect()));
            add(ui);
        });
}

fn search_field(ui: &mut Ui, query: &mut String, placeholder: &str) -> egui::Response {
    let width = (ui.available_width() - 160.0).clamp(85.0, 206.0);
    let response = ui.add(
        egui::TextEdit::singleline(query)
            .hint_text(placeholder)
            .desired_width(width)
            .margin(egui::Margin {
                left: 27,
                right: 7,
                top: 4,
                bottom: 4,
            }),
    );
    let center = egui::pos2(response.rect.left() + 12.0, response.rect.center().y - 1.0);
    let stroke = egui::Stroke::new(1.5_f32, if response.has_focus() { RULE } else { MUTED });
    ui.painter().circle_stroke(center, 4.4, stroke);
    ui.painter().line_segment(
        [center + egui::vec2(3.2, 3.2), center + egui::vec2(6.3, 6.3)],
        stroke,
    );
    response
}

fn muted(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::display_font(16.0))
            .color(ACCENT),
    );
}

fn action(ui: &mut Ui, enabled: bool, text: &str) -> bool {
    let color = if enabled { TEXT } else { MUTED };
    let fill = if enabled { BUTTON_ORANGE } else { SURFACE };
    let button = egui::Button::new(
        RichText::new(text)
            .color(color)
            .font(theme::display_font(14.0)),
    )
    .fill(fill);
    ui.add_enabled(enabled, button).clicked()
}

fn folder(title: &str) -> Result<Option<Scope>> {
    rfd::FileDialog::new()
        .set_title(title)
        .pick_folder()
        .map(|p| Scope::select(&p))
        .transpose()
}

fn file(title: &str) -> Result<Option<Scope>> {
    rfd::FileDialog::new()
        .set_title(title)
        .pick_file()
        .map(|p| Scope::select(&p))
        .transpose()
}

fn reveal(source: &Source) -> Result<()> {
    source.metadata()?;
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&source.path)
        .spawn()?;
    Ok(())
}

fn byte_count(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.2} GiB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn path_label(ui: &mut Ui, path: &Path) {
    let text = path.display().to_string();
    ui.add(egui::Label::new(RichText::new(&text).small().color(MUTED)).truncate())
        .on_hover_text(text);
}
