use eframe::egui::{self, Pos2, Rect, Shape, Stroke, Ui, Vec2};

use super::theme;

const LINKS: usize = 12;
const LINK_LENGTH: f32 = 9.0;
const CORD_LENGTH: f32 = LINKS as f32 * LINK_LENGTH;
const WIRE_LENGTH: f32 = 122.0;
const ANCHOR: Vec2 = egui::vec2(34.0, 63.0);
const STRETCH_LIMIT: f32 = 54.0;
const SWITCH_STRETCH: f32 = 22.0;
const SCALE: f32 = 0.145;
const SIZE: Vec2 = egui::vec2(52.0, 52.0);
const SETTLE_SECONDS: f32 = 3.5;

struct Spring {
    position: f32,
    velocity: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    fn new(position: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            position,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    fn step(&mut self, target: f32, dt: f32) {
        self.velocity +=
            ((target - self.position) * self.stiffness - self.velocity * self.damping) * dt;
        self.position += self.velocity * dt;
    }
}

struct Tap {
    target: Vec2,
    remaining: f32,
}

pub(super) struct PullCord {
    angle: Spring,
    drop: Spring,
    light: Spring,
    bulb: Spring,
    length: Spring,
    hand_x: Spring,
    hand_y: Spring,
    rope: [Vec2; LINKS + 1],
    velocity: [Vec2; LINKS + 1],
    grab_offset: Option<Vec2>,
    tap: Option<Tap>,
    next_side: f32,
    armed: bool,
    settle_for: f32,
}

impl PullCord {
    pub(super) fn new(lit: bool) -> Self {
        // Pull Cord's component-specific springs from DesignBookmark, in reference units.
        let anchor = ANCHOR + egui::vec2(0.0, WIRE_LENGTH);
        Self {
            angle: Spring::new(0.0, 46.0, 3.0),
            drop: Spring::new(0.0, 300.0, 13.0),
            light: Spring::new(f32::from(lit), 120.0, 21.0),
            bulb: Spring::new(1.0, 420.0, 13.0),
            length: Spring::new(1.0, 300.0, 17.0),
            hand_x: Spring::new(0.0, 210.0, 22.0),
            hand_y: Spring::new(0.0, 210.0, 22.0),
            rope: std::array::from_fn(|i| anchor + egui::vec2(0.0, i as f32 * LINK_LENGTH)),
            velocity: [Vec2::ZERO; LINKS + 1],
            grab_offset: None,
            tap: None,
            next_side: -1.0,
            armed: true,
            settle_for: 0.0,
        }
    }

    fn head(&self, point: Vec2) -> Vec2 {
        egui::emath::Rot2::from_angle(self.angle.position)
            * (point + egui::vec2(0.0, WIRE_LENGTH + self.drop.position))
    }

    fn tap(&mut self) {
        let bead = self.rope[LINKS];
        let speed = self.velocity[LINKS];
        self.hand_x.position = bead.x;
        self.hand_x.velocity = speed.x;
        self.hand_y.position = bead.y;
        self.hand_y.velocity = speed.y;
        self.tap = Some(Tap {
            target: self.head(ANCHOR) + egui::vec2(self.next_side * 26.0, CORD_LENGTH + 64.0),
            remaining: 1.1,
        });
        self.next_side = -self.next_side;
        self.armed = true;
    }

    fn step(&mut self, dt: f32, held: Option<Vec2>, lit: &mut bool) {
        let tap_point = self.tap.as_mut().map(|tap| {
            tap.remaining -= dt;
            self.hand_x.step(tap.target.x, dt);
            self.hand_y.step(tap.target.y, dt);
            egui::vec2(self.hand_x.position, self.hand_y.position)
        });
        if self.tap.as_ref().is_some_and(|tap| tap.remaining <= 0.0) {
            self.tap = None;
        }
        let anchor = self.head(ANCHOR);
        let mut stretch = 0.0;
        let mut lean = 0.0;
        let pin = held.or(tap_point).map(|point| {
            let delta = point - anchor;
            let distance = delta.length().max(0.001);
            let over = (distance - CORD_LENGTH).max(0.0);
            stretch = STRETCH_LIMIT * (1.0 - (-over / 70.0).exp());
            let reach = distance.min(CORD_LENGTH) + stretch;
            if over > 0.0 {
                lean = delta.x / distance;
                self.length.position = reach / CORD_LENGTH;
                self.length.velocity = 0.0;
            }
            anchor + delta * (reach / distance)
        });
        if stretch == 0.0 {
            self.length.step(1.0, dt);
        }
        if self.armed && stretch > SWITCH_STRETCH {
            self.armed = false;
            *lit = !*lit;
            self.bulb.position = if *lit { 1.35 } else { 0.8 };
            self.drop.velocity += 150.0;
            if let Some(tap) = &mut self.tap {
                tap.remaining = 0.12;
            }
        }
        self.angle
            .step((-lean * stretch / STRETCH_LIMIT * 0.5).clamp(-0.3, 0.3), dt);
        self.drop.step(stretch * 0.22, dt);
        self.bulb.step(1.0, dt);
        (self.light.stiffness, self.light.damping) =
            if *lit { (120.0, 21.0) } else { (170.0, 27.0) };
        self.light.step(f32::from(*lit), dt);

        let previous = self.rope;
        let link = LINK_LENGTH * self.length.position.max(1.0);
        for i in 1..=LINKS {
            let drag: f32 = if i == LINKS { 1.1 } else { 2.4 };
            let speed = (self.velocity[i] + egui::vec2(0.0, 2600.0 * dt)) * (-drag * dt).exp();
            self.velocity[i] = speed * (2600.0 / speed.length().max(2600.0));
            self.rope[i] += self.velocity[i] * dt;
        }
        for _ in 0..8 {
            self.rope[0] = anchor;
            if let Some(point) = pin {
                self.rope[LINKS] = point;
            }
            for i in 0..LINKS {
                let delta = self.rope[i + 1] - self.rope[i];
                let distance = delta.length().max(0.001);
                let correction = delta * (1.0 - link / distance);
                let a = f32::from(i > 0);
                let b = if i + 1 < LINKS {
                    1.0
                } else if pin.is_some() {
                    0.0
                } else {
                    0.3
                };
                self.rope[i] += correction * (a / (a + b));
                self.rope[i + 1] -= correction * (b / (a + b));
            }
        }
        self.rope[0] = anchor;
        if let Some(point) = pin {
            self.rope[LINKS] = point;
        }
        for (i, previous) in previous.iter().enumerate().skip(1) {
            self.velocity[i] = (self.rope[i] - *previous) / dt;
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, lit: bool) -> bool {
        let (rect, response) = ui.allocate_exact_size(SIZE, egui::Sense::click_and_drag());
        let origin = egui::pos2(rect.center().x, rect.top() + 1.0);

        if self.grab_offset.is_none()
            && (response.is_pointer_button_down_on() || response.drag_started())
            && let Some(start) = ui.input(|input| input.pointer.press_origin())
        {
            self.grab_offset = Some(self.rope[LINKS] - (start - origin) / SCALE);
            self.armed = true;
            self.tap = None;
        }
        if response.clicked() && self.tap.is_none() && (self.grab_offset.is_none() || self.armed) {
            self.grab_offset = None;
            self.tap();
        }
        let held = self
            .grab_offset
            .zip(ui.input(|input| input.pointer.interact_pos()))
            .map(|(offset, pointer)| offset + (pointer - origin) / SCALE);
        let mut next = lit;
        let dt = ui
            .input(|input| input.unstable_dt)
            .clamp(1.0 / 240.0, 1.0 / 30.0);
        if held.is_some() || self.tap.is_some() {
            self.settle_for = SETTLE_SECONDS;
        }
        if self.settle_for > 0.0 {
            for _ in 0..4 {
                self.step(dt / 4.0, held, &mut next);
            }
            self.settle_for -= dt;
            if self.settle_for > 0.0 {
                ui.ctx().request_repaint();
            } else {
                let next_side = self.next_side;
                *self = Self::new(next);
                self.next_side = next_side;
            }
        }
        if response.drag_stopped() || !ui.input(|input| input.pointer.primary_down()) {
            self.grab_offset = None;
        }

        self.draw(ui, rect, origin);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0_f32, theme::colors(ui).rule),
                egui::StrokeKind::Inside,
            );
        }
        response
            .on_hover_cursor(if held.is_some() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            })
            .on_hover_text(if lit {
                "Pull or click for dark mode"
            } else {
                "Pull or click for light mode"
            });

        next != lit
    }

    fn draw(&self, ui: &Ui, rect: Rect, origin: Pos2) {
        let painter = ui.painter();
        let to_screen = |point: Vec2| origin + point * SCALE;
        let bulb = to_screen(self.head(egui::vec2(0.0, 63.0)));
        let light = self.light.position.clamp(0.0, 1.0);
        painter.rect_filled(
            rect,
            4.0,
            if light >= 0.995 {
                theme::LIGHT.raised
            } else {
                theme::DARK.background
            },
        );
        if light > 0.01 && light < 0.995 {
            let far = bulb
                .distance(rect.left_bottom())
                .max(bulb.distance(rect.right_bottom()))
                .max(bulb.distance(rect.left_top()))
                .max(bulb.distance(rect.right_top()));
            painter.with_clip_rect(rect.shrink(1.0)).circle_filled(
                bulb,
                light * far,
                theme::LIGHT.raised,
            );
        }
        painter.rect_stroke(
            rect,
            4,
            Stroke::new(1.0_f32, theme::colors(ui).line),
            egui::StrokeKind::Inside,
        );
        let wire = if light > 0.45 {
            theme::LIGHT.rule
        } else {
            theme::DARK.muted
        };
        let shade = if light > 0.45 {
            theme::LIGHT.accent
        } else {
            theme::DARK.rule
        };
        let bead = if light > 0.45 {
            theme::LIGHT.accent
        } else {
            theme::DARK.accent
        };
        painter.line_segment(
            [origin, to_screen(self.head(egui::vec2(0.0, -4.0)))],
            Stroke::new(1.0_f32, wire),
        );
        let glow = theme::SKY.gamma_multiply(0.6 * light);
        painter.with_clip_rect(rect.shrink(1.0)).add(
            egui::epaint::RectShape::filled(
                Rect::from_center_size(bulb, egui::Vec2::splat(9.0)),
                4,
                glow,
            )
            .with_blur_width(10.0),
        );
        painter.circle_filled(
            bulb,
            19.0 * self.bulb.position * SCALE,
            if light > 0.03 {
                theme::SKY
            } else {
                theme::DARK.line
            },
        );
        painter.line_segment(
            [
                to_screen(self.head(egui::vec2(0.0, -9.0))),
                to_screen(self.head(egui::vec2(0.0, 2.0))),
            ],
            Stroke::new(18.0 * SCALE, wire),
        );
        let mut shade_outline: Vec<_> = (0..=20)
            .map(|i| {
                let angle = std::f32::consts::PI * i as f32 / 20.0;
                to_screen(self.head(egui::vec2(-62.0 * angle.cos(), 56.0 * (1.0 - angle.sin()))))
            })
            .collect();
        shade_outline.extend(
            [
                egui::vec2(60.0, 62.0),
                egui::vec2(53.0, 66.0),
                egui::vec2(-53.0, 66.0),
                egui::vec2(-60.0, 62.0),
            ]
            .map(|point| to_screen(self.head(point))),
        );
        painter.add(Shape::convex_polygon(shade_outline, shade, Stroke::NONE));
        painter.add(Shape::line(
            self.rope.iter().copied().map(to_screen).collect(),
            Stroke::new(1.0_f32, wire),
        ));
        let end = self.rope[LINKS];
        let direction = (end - self.rope[LINKS - 1]).normalized();
        let bottom = to_screen(end + direction * 21.0);
        let top = to_screen(end + direction * 2.0);
        painter.line_segment([top, bottom], Stroke::new(2.7_f32, bead));
        painter.circle_filled(top, 1.35, bead);
        painter.circle_filled(bottom, 1.35, bead);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_the_control_switches_once_then_centers_the_lamp() {
        let ctx = egui::Context::default();
        let mut cord = PullCord::new(true);
        let mut lit = true;
        let mut time = 0.0;
        let mut frame = |events| {
            time += 1.0 / 60.0;
            let input = egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if cord.show(ui, lit) {
                        lit = !lit;
                    }
                });
            });
        };
        let bead = egui::pos2(39.0, 53.0);
        let dragged = bead + egui::vec2(12.0, 26.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(bead)]);
        frame(vec![button(bead, true)]);
        frame(vec![egui::Event::PointerMoved(dragged)]);
        frame(vec![button(dragged, false)]);
        for _ in 0..240 {
            frame(vec![]);
        }
        assert!(!lit);
        assert_eq!(cord.head(egui::vec2(0.0, 63.0)).x, 0.0);
    }

    #[test]
    fn click_runs_a_single_pull_and_releases() {
        let mut cord = PullCord::new(true);
        let mut lit = true;
        cord.tap();
        for _ in 0..600 {
            cord.step(1.0 / 240.0, None, &mut lit);
        }
        assert!(!lit);
        assert!(cord.tap.is_none());
        assert!(cord.light.position.abs() < 0.001);
    }

    #[test]
    fn a_short_pull_does_not_flip_and_a_long_hold_flips_only_once() {
        let mut cord = PullCord::new(false);
        let mut lit = false;
        let anchor = cord.head(ANCHOR);
        cord.step(
            1.0 / 240.0,
            Some(anchor + egui::vec2(0.0, CORD_LENGTH + 10.0)),
            &mut lit,
        );
        assert!(!lit);
        for _ in 0..240 {
            cord.step(
                1.0 / 240.0,
                Some(anchor + egui::vec2(0.0, CORD_LENGTH + 92.0)),
                &mut lit,
            );
        }
        assert!(lit);
    }
}
