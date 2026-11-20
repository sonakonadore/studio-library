use eframe::egui::{self, Color32, Context, FontFamily, FontId, Rect, Shape, Stroke, Ui};

pub(super) const BG: Color32 = Color32::from_rgb(250, 242, 226);
pub(super) const SURFACE: Color32 = Color32::from_rgb(242, 230, 208);
pub(super) const RAISED: Color32 = Color32::from_rgb(255, 250, 239);
pub(super) const LINE: Color32 = Color32::from_rgb(190, 161, 116);
pub(super) const TEXT: Color32 = Color32::from_rgb(47, 38, 26);
pub(super) const MUTED: Color32 = Color32::from_rgb(101, 83, 59);
pub(super) const ACCENT: Color32 = Color32::from_rgb(161, 61, 29);
pub(super) const WARN: Color32 = ACCENT;
pub(super) const GOLD: Color32 = Color32::from_rgb(244, 191, 82);
const HIGHLIGHT: Color32 = Color32::from_rgb(244, 217, 160);
const INTERACTION: Color32 = Color32::from_rgb(240, 207, 144);
const TILE: usize = 160;

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
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    visuals.widgets.noninteractive.fg_stroke.color = MUTED;
    for (widget, fill, outline) in [
        (&mut visuals.widgets.inactive, SURFACE, LINE),
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
        .insert(egui::TextStyle::Heading, display_font(27.0));
    ctx.set_style(style);
}

pub(super) fn grain(ctx: &Context) -> egui::TextureHandle {
    let mut seed = 0x7639_912b_u32;
    let pixels = (0..TILE * TILE)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let fleck = (seed & 0xf) as u8;
            if seed & 0x100 == 0 {
                Color32::from_rgba_unmultiplied(91, 48, 16, fleck / 2)
            } else {
                Color32::from_rgba_unmultiplied(255, 253, 237, fleck)
            }
        })
        .collect();
    ctx.load_texture(
        "paper-grain",
        egui::ColorImage::new([TILE, TILE], pixels),
        egui::TextureOptions::LINEAR_REPEAT,
    )
}

pub(super) fn paper(ui: &Ui, texture: egui::TextureId, rect: Rect) -> Shape {
    let rect = rect.intersect(ui.clip_rect());
    let mut mesh = egui::Mesh::with_texture(texture);
    let step = TILE as f32 / 1.55;
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
