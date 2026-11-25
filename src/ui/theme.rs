use eframe::egui::{
    self, Color32, Context, FontFamily, FontId, Rect, Response, RichText, Stroke, Ui,
    epaint::RectShape,
};

const INDIGO: Color32 = Color32::from_rgb(33, 25, 106);
const COBALT: Color32 = Color32::from_rgb(38, 53, 169);
const AZURE: Color32 = Color32::from_rgb(10, 99, 188);
pub(super) const SKY: Color32 = Color32::from_rgb(63, 173, 255);

pub(super) struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub line: Color32,
    pub rule: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub warn: Color32,
    pub panel: Color32,
    pub highlight: Color32,
    pub interaction: Color32,
    pub faint: Color32,
}

pub(super) const LIGHT: Palette = Palette {
    background: Color32::WHITE,
    surface: Color32::from_rgb(253, 254, 255),
    raised: Color32::WHITE,
    line: Color32::from_rgb(218, 226, 248),
    rule: Color32::from_rgb(119, 139, 186),
    text: Color32::from_rgb(19, 24, 58),
    muted: Color32::from_rgb(72, 86, 127),
    accent: Color32::from_rgb(46, 56, 159),
    warn: Color32::from_rgb(153, 39, 85),
    panel: Color32::from_rgb(254, 254, 255),
    highlight: Color32::from_rgb(235, 242, 255),
    interaction: Color32::from_rgb(219, 231, 255),
    faint: Color32::from_rgb(247, 250, 255),
};

pub(super) const DARK: Palette = Palette {
    background: Color32::from_gray(22),
    surface: Color32::from_gray(25),
    raised: Color32::from_gray(36),
    line: Color32::from_gray(51),
    rule: Color32::from_gray(100),
    text: Color32::from_gray(248),
    muted: Color32::from_gray(197),
    accent: Color32::from_gray(224),
    warn: Color32::from_rgb(255, 168, 199),
    panel: Color32::from_gray(27),
    highlight: Color32::from_gray(48),
    interaction: Color32::from_gray(59),
    faint: Color32::from_gray(31),
};

pub(super) fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

pub(super) fn colors(ui: &Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

pub(super) fn set_mode(ctx: &Context, dark: bool) {
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

#[derive(Clone, Copy)]
pub(super) enum Face {
    Light,
    Regular,
    Medium,
    Semibold,
    Bold,
    Italic,
}

impl Face {
    fn name(self) -> &'static str {
        match self {
            Self::Light => "IBM Plex Sans Light",
            Self::Regular => "IBM Plex Sans",
            Self::Medium => "IBM Plex Sans Medium",
            Self::Semibold => "IBM Plex Sans SemiBold",
            Self::Bold => "IBM Plex Sans Bold",
            Self::Italic => "IBM Plex Sans Italic",
        }
    }

    fn family(self) -> FontFamily {
        match self {
            Self::Regular => FontFamily::Proportional,
            _ => FontFamily::Name(self.name().into()),
        }
    }
}

pub(super) fn font(face: Face, size: f32) -> FontId {
    FontId::new(size, face.family())
}

pub(super) fn style(ctx: &Context, dark: bool) {
    let gradients = Gradients::new(ctx);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("studio_gradients"), gradients));

    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc") {
        let hangul = egui::FontData::from_owned(bytes).tweak(egui::FontTweak {
            scale: 0.93,
            ..Default::default()
        });
        fonts.font_data.insert("Hangul".into(), hangul.into());
        for face in fonts.families.values_mut() {
            face.push("Hangul".into());
        }
    }
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    for (face, bytes) in [
        (
            Face::Regular,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            Face::Light,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Light.ttf")[..],
        ),
        (
            Face::Medium,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Medium.ttf")[..],
        ),
        (
            Face::Semibold,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf")[..],
        ),
        (
            Face::Bold,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Bold.ttf")[..],
        ),
        (
            Face::Italic,
            &include_bytes!("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Italic.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            face.name().into(),
            egui::FontData::from_static(bytes).into(),
        );
        let family = std::iter::once(face.name().into())
            .chain(fallbacks.iter().cloned())
            .collect();
        fonts.families.insert(face.family(), family);
    }
    ctx.set_fonts(fonts);

    for mode in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*ctx.style_of(mode)).clone();
        style.visuals = visuals(mode == egui::Theme::Dark);
        style.spacing.item_spacing = egui::vec2(7.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.5));
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, font(Face::Medium, 14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, font(Face::Semibold, 23.0));
        ctx.set_style_of(mode, style);
    }
    set_mode(ctx, dark);
}

fn visuals(dark: bool) -> egui::Visuals {
    let colors = palette(dark);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(colors.text);
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::new(1.0_f32, colors.line);
    visuals.extreme_bg_color = colors.raised;
    visuals.text_edit_bg_color = Some(colors.raised);
    visuals.faint_bg_color = colors.faint;
    visuals.selection.bg_fill = colors.highlight;
    visuals.selection.stroke = Stroke::new(1.0_f32, colors.accent);
    if dark {
        visuals.hyperlink_color = colors.accent;
        visuals.text_cursor.stroke.color = colors.text;
    }
    visuals.slider_trailing_fill = true;
    visuals.warn_fg_color = colors.warn;
    visuals.error_fg_color = colors.warn;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, colors.line);
    visuals.widgets.noninteractive.fg_stroke.color = colors.muted;
    for (widget, fill, outline) in [
        (&mut visuals.widgets.inactive, colors.raised, colors.rule),
        (
            &mut visuals.widgets.hovered,
            colors.highlight,
            colors.accent,
        ),
        (
            &mut visuals.widgets.active,
            colors.interaction,
            colors.accent,
        ),
        (&mut visuals.widgets.open, colors.highlight, colors.accent),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0_f32, outline);
        widget.fg_stroke.color = colors.text;
        widget.corner_radius = egui::CornerRadius::same(4);
    }

    visuals
}

#[derive(Clone)]
struct Gradients {
    accent: egui::TextureHandle,
    wash: egui::TextureHandle,
}

impl Gradients {
    fn new(ctx: &Context) -> Self {
        Self {
            accent: Self::texture(ctx, "accent gradient", |x, y| {
                let diagonal = (0.8 * x + 0.2 * y).clamp(0.0, 1.0);
                let indigo = INDIGO.lerp_to_gamma(COBALT, diagonal);
                let blue =
                    (1.0 - ((x - 1.05) * 1.15).powi(2) - ((y - 0.8) * 0.7).powi(2)).clamp(0.0, 1.0);
                indigo.lerp_to_gamma(AZURE, blue * blue)
            }),
            wash: Self::texture(ctx, "header wash", |x, y| {
                let distance =
                    ((x - 0.58) * 2.12).powi(2) + ((y - 0.48 + (x - 0.5) * 0.32) * 2.05).powi(2);
                let edge = x.min(1.0 - x).min(y).min(1.0 - y);
                let opacity = (1.0 - distance).max(0.0).powi(2) * (6.0 * edge).min(1.0);
                let hue = INDIGO.lerp_to_gamma(SKY, (0.64 * x + 0.36 * (1.0 - y)).clamp(0.0, 1.0));
                Color32::from_rgba_unmultiplied(hue.r(), hue.g(), hue.b(), (84.0 * opacity) as u8)
            }),
        }
    }

    fn texture(
        ctx: &Context,
        name: &str,
        color: impl Fn(f32, f32) -> Color32,
    ) -> egui::TextureHandle {
        const WIDTH: usize = 128;
        const HEIGHT: usize = 48;
        let mut pixels = Vec::with_capacity(WIDTH * HEIGHT);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                pixels.push(color(
                    x as f32 / (WIDTH - 1) as f32,
                    y as f32 / (HEIGHT - 1) as f32,
                ));
            }
        }
        let image = egui::ColorImage::new([WIDTH, HEIGHT], pixels);
        ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
    }

    fn get(ui: &Ui) -> Self {
        ui.data(|data| data.get_temp(egui::Id::new("studio_gradients")))
            .expect("theme initialized")
    }
}

fn textured_rect(rect: Rect, radius: u8, tint: Color32, texture: egui::TextureId) -> RectShape {
    let uv = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    RectShape::filled(rect, radius, tint).with_texture(texture, uv)
}

pub(super) fn header_wash(ui: &Ui, panel: Rect) -> RectShape {
    let rect = Rect::from_min_max(
        egui::pos2(panel.right() - panel.width().min(565.0), panel.top()),
        panel.right_bottom(),
    );
    textured_rect(rect, 0, Color32::WHITE, Gradients::get(ui).wash.id())
}

pub(super) fn accent_button(ui: &mut Ui, label: RichText, min_size: egui::Vec2) -> Response {
    let background = ui.painter().add(egui::Shape::Noop);
    let button = egui::Button::new(label.color(Color32::WHITE))
        .min_size(min_size)
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE);
    let response = ui.add(button);
    let tint = if response.is_pointer_button_down_on() {
        Color32::from_gray(209)
    } else if response.hovered() || response.has_focus() {
        Color32::WHITE
    } else {
        Color32::from_gray(242)
    };
    ui.painter().set(
        background,
        textured_rect(response.rect, 4, tint, Gradients::get(ui).accent.id()),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            3,
            Stroke::new(1.0_f32, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    response
}

pub(super) fn mark(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
    let painter = ui.painter();
    if ui.visuals().dark_mode {
        painter.rect(
            rect,
            9,
            DARK.raised,
            Stroke::new(1.0_f32, DARK.line),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.add(textured_rect(
            rect,
            9,
            Color32::WHITE,
            Gradients::get(ui).accent.id(),
        ));
    }
    for (i, height) in [9.0, 15.0, 21.0, 13.0].into_iter().enumerate() {
        let x = rect.left() + 8.5 + i as f32 * 5.6;
        let middle = rect.center().y;
        painter.line_segment(
            [
                egui::pos2(x, middle - height / 2.0),
                egui::pos2(x, middle + height / 2.0),
            ],
            Stroke::new(2.3_f32, Color32::WHITE),
        );
    }
}
