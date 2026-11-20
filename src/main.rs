mod jobs;
mod player;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1230.0, 790.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Studio Library",
        options,
        Box::new(|cc| Ok(Box::new(ui::Studio::new(&cc.egui_ctx)?))),
    )
}
