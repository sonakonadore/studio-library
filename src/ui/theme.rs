use eframe::egui::{self, Color32, Context, FontFamily, FontId, Rect, Shape, Stroke, Ui};

pub(super) const BG: Color32 = Color32::from_rgb(249, 234, 207);
pub(super) const SURFACE: Color32 = Color32::from_rgb(244, 223, 192);
pub(super) const RAISED: Color32 = Color32::from_rgb(255, 243, 218);
pub(super) const LINE: Color32 = Color32::from_rgb(147, 112, 64);
pub(super) const RULE: Color32 = Color32::from_rgb(175, 76, 30);
pub(super) const TEXT: Color32 = Color32::from_rgb(45, 32, 20);
pub(super) const MUTED: Color32 = Color32::from_rgb(78, 52, 29);
pub(super) const ACCENT: Color32 = Color32::from_rgb(171, 60, 16);
pub(super) const WARN: Color32 = Color32::from_rgb(126, 39, 14);
pub(super) const GOLD: Color32 = Color32::from_rgb(253, 190, 59);
const HIGHLIGHT: Color32 = Color32::from_rgb(247, 214, 156);
const INTERACTION: Color32 = Color32::from_rgb(240, 191, 117);
const TILE: usize = 512;

pub(super) fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub(super) fn style(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc") {
        fonts
            .font_data
            .insert("Hangul".into(), egui::FontData::from_owned(bytes).into());
        for face in fonts.families.values_mut() {
            face.push("Hangul".into());
        }
    }
    let mut display = fonts.families[&FontFamily::Proportional].clone();
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/HelveticaNeue.ttc") {
        let regular = egui::FontData::from_owned(bytes);
        let mut bold = regular.clone();
        bold.index = 1;
        fonts
            .font_data
            .insert("Helvetica Neue".into(), regular.into());
        fonts
            .font_data
            .insert("Helvetica Neue Bold".into(), bold.into());
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "Helvetica Neue".into());
        display.insert(0, "Helvetica Neue Bold".into());
    }
    fonts
        .families
        .insert(FontFamily::Name("display".into()), display);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    visuals.extreme_bg_color = RAISED;
    visuals.text_edit_bg_color = Some(RAISED);
    visuals.faint_bg_color = Color32::from_rgb(246, 232, 205);
    visuals.selection.bg_fill = HIGHLIGHT;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = WARN;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, RULE);
    visuals.widgets.noninteractive.fg_stroke.color = MUTED;
    for (widget, fill, outline) in [
        (&mut visuals.widgets.inactive, RAISED, RULE),
        (&mut visuals.widgets.hovered, HIGHLIGHT, ACCENT),
        (&mut visuals.widgets.active, INTERACTION, ACCENT),
        (&mut visuals.widgets.open, HIGHLIGHT, ACCENT),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0_f32, outline);
        widget.fg_stroke.color = TEXT;
        widget.corner_radius = egui::CornerRadius::same(4);
    }

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style.spacing.interact_size.y = 27.0;
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.5));
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, display_font(28.0));
    ctx.set_style(style);
}

pub(super) fn grain(ctx: &Context) -> egui::TextureHandle {
    // Combine soft pulp variations with fine flecks, keeping grain out of the text itself.
    let mut seed = 0x7639_912b_u32;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed & 0xff) as f32 / 127.5 - 1.0
    };
    let coarse: Vec<_> = (0..8 * 8).map(|_| noise()).collect();
    let fine: Vec<_> = (0..64 * 64).map(|_| noise()).collect();
    let pixels = (0..TILE * TILE)
        .map(|index| {
            let x = index % TILE;
            let y = index / TILE;
            let tone = 13.0 * paper_noise(&coarse, 8, x, y)
                + 18.0 * paper_noise(&fine, 64, x, y)
                + 20.0 * noise();
            if tone < 0.0 {
                Color32::from_rgba_unmultiplied(97, 48, 13, (tone.abs() * 0.75) as u8)
            } else {
                Color32::from_rgba_unmultiplied(255, 249, 227, tone as u8)
            }
        })
        .collect();
    ctx.load_texture(
        "paper-grain",
        egui::ColorImage::new([TILE, TILE], pixels),
        egui::TextureOptions::LINEAR_REPEAT,
    )
}

fn paper_noise(grid: &[f32], width: usize, x: usize, y: usize) -> f32 {
    let pitch = TILE / width;
    let row = y / pitch;
    let col = x / pitch;
    let next_row = (row + 1) % width;
    let next_col = (col + 1) % width;
    let x_weight = (x % pitch) as f32 / pitch as f32;
    let y_weight = (y % pitch) as f32 / pitch as f32;
    let top = egui::lerp(
        grid[row * width + col]..=grid[row * width + next_col],
        x_weight,
    );
    let bottom = egui::lerp(
        grid[next_row * width + col]..=grid[next_row * width + next_col],
        x_weight,
    );
    egui::lerp(top..=bottom, y_weight)
}

pub(super) fn paper(ui: &Ui, texture: egui::TextureId, rect: Rect) -> Shape {
    let rect = rect.intersect(ui.clip_rect());
    let mut mesh = egui::Mesh::with_texture(texture);
    let step = TILE as f32 / 1.75;
    let uv = Rect::from_min_max(rect.min / step, rect.max / step);
    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
    Shape::mesh(mesh)
}

pub(super) fn mark(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(39.0, 39.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 9, TEXT);
    for (i, height) in [10.0, 18.0, 25.0, 15.0].into_iter().enumerate() {
        let x = rect.left() + 9.75 + i as f32 * 6.5;
        let middle = rect.center().y;
        painter.line_segment(
            [
                egui::pos2(x, middle - height / 2.0),
                egui::pos2(x, middle + height / 2.0),
            ],
            Stroke::new(2.6_f32, GOLD),
        );
    }
}
