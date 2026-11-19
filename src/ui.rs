use crate::jobs::{Event, Job, Outcome};
use crate::player::{Playback, Player};
use anyhow::Result;
use eframe::egui::{self, Color32, Context, Frame, RichText, ScrollArea, Ui};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use studio_library::{
    catalog::{Settings, Store, VirtualFolder, database_path, edited_tags, matches_tags},
    copying::{Plan, project_plan, sample_plan},
    projects::{self, FORMATS, Project, ProjectEdit, Rule, RuleKind},
    scope::{Cancelled, Scope, Source, extension, key, name, safe_component},
};

mod project_tab;
mod sample_tab;

const BG: Color32 = Color32::from_rgb(22, 25, 27);
const SURFACE: Color32 = Color32::from_rgb(29, 33, 35);
const RAISED: Color32 = Color32::from_rgb(39, 44, 47);
const LINE: Color32 = Color32::from_rgb(54, 61, 63);
const TEXT: Color32 = Color32::from_rgb(231, 237, 234);
const MUTED: Color32 = Color32::from_rgb(166, 181, 178);
const ACCENT: Color32 = Color32::from_rgb(151, 231, 197);
const WARN: Color32 = Color32::from_rgb(255, 194, 137);

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
        appearance(ctx);
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
            status: "준비".into(),
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
            "작업 취소됨".into()
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
                    break Err(anyhow::anyhow!("백그라운드 작업이 중단되었습니다."));
                }
            }
        };
        self.job = None;
        self.warnings.clear();
        match result {
            Ok(Outcome::Projects(scan)) => {
                self.status = format!(
                    "프로젝트 탐색 완료 · {}개 · 링크 {}개 건너뜀",
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
                    "샘플 탐색 완료 · {}개 · 새로 분석 {}개 · 링크 등 {}개 건너뜀",
                    scan.rows.len(),
                    scan.decoded,
                    scan.skipped
                );
                self.warnings = scan.warnings;
                self.replace_samples(scan.rows);
            }
            Ok(Outcome::Plan(plan)) => {
                self.status = "출발지와 대상을 확인하세요.".into();
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
                    self.status = format!("{message} · 복사 위치 기록 {}", self.status);
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
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(24, 13))
                    .stroke(egui::Stroke::new(1.0_f32, LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(30.0, 31.0), egui::Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 7, ACCENT);
                    for (i, height) in [8.0, 15.0, 20.0, 12.0].into_iter().enumerate() {
                        let x = rect.left() + 7.0 + (i as f32) * 5.0;
                        let middle = rect.center().y;
                        painter.line_segment(
                            [
                                egui::pos2(x, middle - height / 2.0),
                                egui::pos2(x, middle + height / 2.0),
                            ],
                            egui::Stroke::new(2.0_f32, BG),
                        );
                    }
                    ui.add_space(5.0);
                    ui.label(RichText::new("스튜디오 라이브러리").size(22.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("로컬 라이브러리").color(MUTED));
                    });
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (Tab::Projects, "프로젝트 정리"),
                        (Tab::Samples, "샘플 라이브러리"),
                    ] {
                        if ui
                            .add_sized(
                                [135.0, 31.0],
                                egui::Button::selectable(self.tab == tab, label),
                            )
                            .clicked()
                        {
                            self.tab = tab;
                        }
                    }
                });
            });
    }

    fn footer(&mut self, ctx: &Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, LINE))
                    .inner_margin(egui::Margin::symmetric(20, 9)),
            )
            .show(ctx, |ui| {
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
                            .add_enabled(!cancelled, egui::Button::new("취소"))
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
                                .button(format!("경고 {}개", self.warnings.len()))
                                .clicked()
                        {
                            self.show_warnings = true;
                        }
                    }
                });
            });
    }

    fn dialogs(&mut self, ctx: &Context) {
        if self.show_warnings {
            egui::Window::new("탐색 경고")
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
                ui.heading("복사 계획 확인");
                ui.label(format!("대상  {}", plan.target().display()));
                ui.label(
                    RichText::new(format!("예상 용량  {}", byte_count(plan.bytes()))).color(ACCENT),
                );
                if plan.skipped > 0 {
                    ui.label(
                        RichText::new(format!("링크 또는 특수 항목 {}개 건너뜀", plan.skipped))
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
                            let source = format!("출발지  {}", item.source.display());
                            ui.add(egui::Label::new(RichText::new(&source).strong()).truncate())
                                .on_hover_text(source);
                            let target = format!(
                                "대상  {}  ·  {}",
                                item.target.display(),
                                if item.directory {
                                    "폴더".into()
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
                    approve = action(ui, true, "승인하고 복사");
                    decline = ui.button("닫기").clicked();
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
                self.status = "복사 취소됨".into();
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

fn appearance(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc") {
        fonts
            .font_data
            .insert("Korean".into(), egui::FontData::from_owned(data).into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("Korean".into());
        }
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, LINE);
    visuals.extreme_bg_color = BG;
    visuals.text_edit_bg_color = Some(RAISED);
    visuals.faint_bg_color = RAISED;
    visuals.selection.bg_fill = Color32::from_rgb(43, 80, 68);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.inactive.bg_fill = RAISED;
    visuals.widgets.inactive.weak_bg_fill = RAISED;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 64, 64);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(9.0, 5.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
    ctx.set_style(style);
}

fn left_panel(id: &'static str, ctx: &Context, add: impl FnOnce(&mut Ui)) {
    egui::SidePanel::left(id)
        .exact_width(238.0)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(SURFACE)
                .inner_margin(egui::Margin::same(15)),
        )
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, add);
        });
}

fn right_panel(id: &'static str, ctx: &Context, add: impl FnOnce(&mut Ui)) {
    egui::SidePanel::right(id)
        .default_width(343.0)
        .width_range(300.0..=480.0)
        .resizable(true)
        .frame(
            Frame::new()
                .fill(SURFACE)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, add);
        });
}

fn center(ctx: &Context, add: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(BG).inner_margin(egui::Margin::same(19)))
        .show(ctx, add);
}

fn muted(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong().color(ACCENT));
}

fn action(ui: &mut Ui, enabled: bool, text: &str) -> bool {
    let button = egui::Button::new(RichText::new(text).color(BG).strong()).fill(ACCENT);
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
