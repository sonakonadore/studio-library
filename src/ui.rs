use crate::jobs::{Event, Job, Outcome};
use crate::player::{Playback, Player};
use anyhow::Result;
use eframe::egui::{self, Context, Frame, RichText, ScrollArea, Ui};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use studio_library::{
    catalog::{
        ProjectGrouping, Settings, Store, VirtualFolder, database_path, edited_tags, matches_tags,
    },
    copying::{Plan, project_plan, sample_plan},
    projects::{self, FORMATS, Project, ProjectEdit, Rule, RuleKind},
    scope::{Cancelled, Scope, Source, extension, key, name, safe_component},
};

mod labels;
mod project_tab;
mod pull_cord;
mod sample_tab;

mod theme;
use theme::Face;

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
    place: Option<PathBuf>,
    browse_path: Vec<String>,
    opened_song: Option<project_tab::SongKey>,
    drive_check: Option<std::sync::mpsc::Receiver<Vec<Scope>>>,
    next_drive_check: Option<std::time::Instant>,
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
    browse_path: Vec<String>,
    show_filters: bool,
    place: Option<PathBuf>,
    active_folder: Option<String>,
    folder_name: String,
    show_folder_form: bool,
    draft_tags: String,
    editing_tags: bool,
    manual_preview: bool,
    playing: Option<PathBuf>,
}

pub struct Studio {
    db: PathBuf,
    lamp: pull_cord::PullCord,
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
        let db = database_path()?;
        let store = Store::open(&db)?;
        let settings = store.settings()?;
        theme::style(ctx, settings.dark_mode);
        let rows = store.samples_in(settings.sample_folders())?;
        let project_rows = store.cached_projects(&settings)?;
        let mut app = Self {
            db,
            lamp: pull_cord::PullCord::new(!settings.dark_mode),
            store,
            settings,
            tab: Tab::Projects,
            projects: ProjectView {
                rows: project_rows,
                ..Default::default()
            },
            samples: SampleView {
                rows,
                ..Default::default()
            },
            job: None,
            plan: None,
            status: "Ready".into(),
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
            "Canceled".into()
        } else {
            format!("Failed · {error:#}")
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
                    break Err(anyhow::anyhow!("Operation interrupted. Try again."));
                }
            }
        };
        self.job = None;
        self.warnings.clear();
        match result {
            Ok(Outcome::Projects(scan)) => {
                let offline = scan.projects.iter().filter(|row| row.offline).count();
                self.status = format!(
                    "Scan complete · {} projects · {offline} offline · {} links skipped",
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
                    "Scan complete · {} samples · {} analyzed · {} skipped",
                    scan.rows.len(),
                    scan.decoded,
                    scan.skipped
                );
                self.warnings = scan.warnings;
                self.replace_samples(scan.rows);
            }
            Ok(Outcome::Plan(plan)) => {
                self.status = "Review the source and destination".into();
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
                    self.status = format!(
                        "{message} · Could not save the copy location · {}",
                        self.status
                    );
                }
            }
            Err(error) => self.error(error),
        }
        if self.close_after_job {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn header(&mut self, ctx: &Context) {
        let dark = ctx.style().visuals.dark_mode;
        let colors = theme::palette(dark);
        let margin = egui::Margin::symmetric(21, 8);
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::new()
                    .fill(colors.panel)
                    .inner_margin(margin)
                    .stroke(egui::Stroke::new(1.0_f32, colors.line)),
            )
            .show(ctx, |ui| {
                let wash = (!dark).then(|| ui.painter().add(egui::Shape::Noop));
                ui.horizontal(|ui| {
                    theme::mark(ui);
                    ui.add_space(5.0);
                    let title = RichText::new("Studio Library").font(theme::font(Face::Bold, 27.0));
                    let brand = ui
                        .add(
                            egui::Label::new(title)
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Switch tabs · Tab");
                    if brand.clicked() {
                        self.toggle_tab();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.lamp.show(ui, !self.settings.dark_mode)
                            && self
                                .save_settings(|settings| settings.dark_mode = !settings.dark_mode)
                        {
                            ctx.request_repaint();
                        }
                        for (tab, label) in [(Tab::Samples, "Samples"), (Tab::Projects, "Projects")]
                        {
                            let label = RichText::new(label).font(theme::font(Face::Medium, 14.0));
                            let size = egui::vec2(100.0, 29.0);
                            let selected = self.tab == tab;
                            let response = if selected && !dark {
                                theme::accent_button(ui, label, size)
                            } else {
                                ui.add(egui::Button::new(label).min_size(size).selected(selected))
                            };
                            if response.clicked() {
                                self.tab = tab;
                            }
                        }
                    });
                });
                if let Some(wash) = wash {
                    ui.painter()
                        .set(wash, theme::header_wash(ui, ui.min_rect() + margin));
                }
            });
    }

    fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Projects => Tab::Samples,
            Tab::Samples => Tab::Projects,
        };
    }

    fn footer(&mut self, ctx: &Context) {
        let colors = theme::palette(ctx.style().visuals.dark_mode);
        egui::TopBottomPanel::bottom("status")
            .min_height(42.0)
            .frame(
                Frame::new()
                    .fill(colors.surface)
                    .stroke(egui::Stroke::new(1.0_f32, colors.line))
                    .inner_margin(egui::Margin::symmetric(23, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(job) = &self.job {
                        ui.spinner();
                        let cancelled = job.cancel.load(Ordering::Relaxed);
                        let label = if cancelled { "Canceling…" } else { job.label };
                        ui.label(
                            RichText::new(format!("{label} {}", job.progress))
                                .color(theme::colors(ui).accent),
                        );
                        if ui
                            .add_enabled(!cancelled, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        let status = self.status.to_ascii_lowercase();
                        let color = if status.contains("failed") || status.contains("canceled") {
                            colors.warn
                        } else {
                            colors.muted
                        };
                        let status = labels::composed(self.status.as_str());
                        ui.add(
                            egui::Label::new(RichText::new(status.as_ref()).color(color))
                                .truncate(),
                        )
                        .on_hover_text(status);
                        if !self.warnings.is_empty()
                            && ui
                                .button(format!("Warnings ({})", self.warnings.len()))
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
            egui::Window::new("Scan warnings")
                .open(&mut self.show_warnings)
                .default_width(540.0)
                .show(ctx, |ui| {
                    ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        for warning in &self.warnings {
                            ui.label(labels::composed(warning.as_str()));
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
                muted(ui, "Original files stay in place.");
                ui.label(format!(
                    "Destination  {}",
                    labels::display_path(&plan.target())
                ));
                ui.label(
                    RichText::new(format!("Estimated size  {}", byte_count(plan.bytes())))
                        .color(theme::colors(ui).accent),
                );
                if plan.skipped > 0 {
                    ui.label(
                        RichText::new(format!("{} links or special files skipped", plan.skipped))
                            .color(theme::colors(ui).warn),
                    );
                }
                ui.separator();
                ScrollArea::vertical().max_height(360.0).show_rows(
                    ui,
                    52.0,
                    plan.entry_count(),
                    |ui, range| {
                        for item in plan.previews(range) {
                            let source = format!("Source  {}", labels::display_path(item.source));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&source).font(theme::font(Face::Medium, 14.0)),
                                )
                                .truncate(),
                            )
                            .on_hover_text(source);
                            let target = format!(
                                "Destination  {}  ·  {}",
                                labels::display_path(&item.target),
                                if item.directory {
                                    "Folder".into()
                                } else {
                                    byte_count(item.bytes)
                                }
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&target).color(theme::colors(ui).muted),
                                )
                                .truncate(),
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
                self.job = Some(Job::start(ctx, "Copying", move |cancel, update| {
                    let report = plan.execute(cancel, |done, total| {
                        update(format!("{} / {}", byte_count(done), byte_count(total)));
                    });
                    Ok(Outcome::Copy(report))
                }));
            } else if decline {
                self.status = "Copy canceled".into();
            } else {
                self.plan = Some(plan);
            }
        }
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if self.tab == Tab::Projects && self.ready() {
            self.check_project_drives(ctx);
        }
        if (ctx.theme() == egui::Theme::Dark) != self.settings.dark_mode {
            theme::set_mode(ctx, self.settings.dark_mode);
        }
        if ctx.input(|input| input.viewport().close_requested())
            && let Some(job) = &self.job
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            job.cancel.store(true, Ordering::Relaxed);
            self.close_after_job = true;
        }
        let dialog_open = self.plan.is_some()
            || self.show_warnings
            || (self.tab == Tab::Projects && self.projects.show_rules);
        if !dialog_open && !egui::Popup::is_any_open(ctx) && take_tab_shortcut(ctx) {
            self.toggle_tab();
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

fn replace_scope(scopes: &mut Vec<Scope>, new: Scope) {
    scopes.retain(|scope| scope.path != new.path);
    scopes.push(new);
}

fn take_tab_shortcut(ctx: &Context) -> bool {
    let (toggle, consumed) = ctx.input_mut(|input| {
        let mut toggle = false;
        let before = input.events.len();
        input.events.retain(|event| {
            if let egui::Event::Key {
                key: egui::Key::Tab,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
                && modifiers.is_none()
            {
                if !repeat {
                    toggle = !toggle;
                }
                false
            } else {
                true
            }
        });
        (toggle, input.events.len() != before)
    });
    if consumed {
        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    toggle
}

fn left_panel(id: &'static str, ctx: &Context, add: impl FnOnce(&mut Ui)) {
    let colors = theme::palette(ctx.style().visuals.dark_mode);
    egui::SidePanel::left(id)
        .exact_width(230.0)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(colors.panel)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, add);
        });
}

fn right_panel(id: &'static str, ctx: &Context, add: impl FnOnce(&mut Ui)) {
    let colors = theme::palette(ctx.style().visuals.dark_mode);
    let max_width = (ctx.content_rect().width() - 230.0 - 348.0).clamp(300.0, 480.0);
    egui::SidePanel::right(id)
        .default_width(322.0)
        .width_range(300.0..=max_width)
        .resizable(true)
        .frame(
            Frame::new()
                .fill(colors.surface)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(ui.max_rect().width());
            ScrollArea::vertical().show(ui, add);
        });
}

fn center(ctx: &Context, add: impl FnOnce(&mut Ui)) {
    let colors = theme::palette(ctx.style().visuals.dark_mode);
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(colors.background)
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ctx, |ui| {
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
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        response.request_focus();
    }
    let center = egui::pos2(response.rect.left() + 12.0, response.rect.center().y - 1.0);
    let stroke = egui::Stroke::new(
        1.5_f32,
        if response.has_focus() {
            theme::colors(ui).rule
        } else {
            theme::colors(ui).muted
        },
    );
    ui.painter().circle_stroke(center, 4.4, stroke);
    ui.painter().line_segment(
        [center + egui::vec2(3.2, 3.2), center + egui::vec2(6.3, 6.3)],
        stroke,
    );
    response
}

fn muted(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(labels::composed(text.into())).color(theme::colors(ui).muted));
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(labels::composed(text))
            .font(theme::font(Face::Medium, 16.0))
            .color(theme::colors(ui).accent),
    );
}

fn folder_breadcrumb(ui: &mut Ui, path: &mut Vec<String>) {
    if path.is_empty() {
        return;
    }
    let mut depth = path.len();
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("Folders").clicked() {
            depth = 0;
        }
        for (index, name) in path.iter().enumerate() {
            muted(ui, "›");
            let name = labels::composed(name.as_str());
            if index + 1 == path.len() {
                ui.label(name);
            } else if ui.small_button(name).clicked() {
                depth = index + 1;
            }
        }
    });
    path.truncate(depth);
}

fn folder_list(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    folders: &[(&str, usize)],
    count_label: &str,
) -> Option<String> {
    let mut open = None;
    ui.separator();
    ui.horizontal(|ui| {
        caption(ui, "Folder");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            caption(ui, count_label);
        });
    });
    ScrollArea::vertical()
        .id_salt(id)
        .show_rows(ui, 27.0, folders.len(), |ui, visible| {
            for &(name, count) in &folders[visible] {
                let name_on_screen = labels::composed(name);
                let button = egui::Button::new(format!("›  {name_on_screen}"))
                    .frame_when_inactive(false)
                    .right_text(count.to_string())
                    .truncate();
                if ui
                    .add_sized([ui.available_width(), 27.0], button)
                    .on_hover_text(name_on_screen)
                    .clicked()
                {
                    open = Some(name.to_owned());
                }
            }
        });
    open
}

fn action(ui: &mut Ui, enabled: bool, text: &str) -> bool {
    let label = RichText::new(text).font(theme::font(Face::Medium, 14.0));
    if enabled {
        theme::accent_button(ui, label, egui::Vec2::ZERO).clicked()
    } else {
        let colors = theme::colors(ui);
        ui.add_enabled(
            false,
            egui::Button::new(label.color(colors.muted)).fill(colors.faint),
        )
        .clicked()
    }
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
    let text = labels::display_path(path);
    ui.add(
        egui::Label::new(
            RichText::new(text.as_ref())
                .small()
                .color(theme::colors(ui).muted),
        )
        .truncate(),
    )
    .on_hover_text(text);
}
