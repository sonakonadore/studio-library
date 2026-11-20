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
        Box::new(|cc| {
            #[cfg(target_os = "macos")]
            set_window_color_space(cc);
            Ok(Box::new(ui::Studio::new(&cc.egui_ctx)?))
        }),
    )
}

#[cfg(target_os = "macos")]
fn set_window_color_space(context: &eframe::CreationContext<'_>) {
    use objc2_app_kit::{NSColorSpace, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = context.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // The creation callback runs on the main thread with a live AppKit view.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    if let Some(window) = view.window() {
        window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
    }
}
