//! Design system: colors, spacing, and reusable surfaces.
//!
//! Apple-flavoured without pretending to be Apple: a dark neutral base, one
//! accent, generous radii, translucent panels over a subtle vertical gradient.
//! Everything here is a pure function of the palette so the whole app can be
//! re-themed from one struct.

use egui::{Align, Color32, CornerRadius, FontId, Layout, RichText, Stroke, Vec2};

/// Linear interpolation in sRGB byte space. Good enough for a backdrop and
/// avoids a dependency on a colour crate for one function.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

#[derive(Clone)]
pub struct Theme {
    pub bg_top: Color32,
    pub bg_bottom: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub stroke: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub positive: Color32,
    pub negative: Color32,
    pub warning: Color32,
}

impl Theme {
    /// Matches the app's pink accent; tuned for contrast on the dark base.
    pub fn dark() -> Self {
        Self {
            bg_top: Color32::from_rgb(24, 24, 27),
            bg_bottom: Color32::from_rgb(13, 13, 16),
            sidebar: Color32::from_rgba_premultiplied(32, 32, 38, 235),
            card: Color32::from_rgba_premultiplied(44, 44, 52, 225),
            card_hover: Color32::from_rgba_premultiplied(54, 54, 64, 235),
            stroke: Color32::from_rgba_unmultiplied(255, 255, 255, 22),
            text: Color32::from_rgb(242, 242, 247),
            text_dim: Color32::from_rgb(152, 152, 163),
            accent: Color32::from_rgb(233, 30, 99),
            positive: Color32::from_rgb(48, 209, 88),
            negative: Color32::from_rgb(255, 105, 97),
            warning: Color32::from_rgb(255, 214, 10),
        }
    }

    pub fn install(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals.panel_fill = self.bg_bottom;
        style.visuals.window_fill = self.card;
        style.visuals.extreme_bg_color = self.bg_bottom;
        style.visuals.widgets.inactive.bg_fill = self.card;
        style.visuals.widgets.hovered.bg_fill = self.card_hover;
        style.visuals.widgets.active.bg_fill = self.accent;
        style.visuals.widgets.inactive.weak_bg_fill = self.card;
        style.visuals.widgets.hovered.weak_bg_fill = self.card_hover;
        style.visuals.widgets.active.weak_bg_fill = self.accent;
        style.visuals.selection.bg_fill = self.accent.gamma_multiply(0.35);
        style.visuals.selection.stroke = Stroke::NONE;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.stroke);
        style.visuals.window_stroke = Stroke::new(1.0_f32, self.stroke);
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(16.0, 9.0);
        style.spacing.window_margin = egui::Margin::same(20);
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 6.0,
            floating_allocated_width: 6.0,
            ..egui::style::ScrollStyle::floating()
        };
        // Type scale. SF-like ramp: big jumps, few in-between sizes.
        style.text_styles = [
            (egui::TextStyle::Heading, FontId::proportional(30.0)),
            (egui::TextStyle::Body, FontId::proportional(14.0)),
            (egui::TextStyle::Button, FontId::proportional(15.0)),
            (egui::TextStyle::Small, FontId::proportional(12.0)),
            (egui::TextStyle::Monospace, FontId::proportional(13.0)),
        ]
        .into_iter()
        .collect();
        style.interaction.selectable_labels = false;
        ctx.set_style(style);
    }

    pub fn frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.card)
            .stroke(Stroke::new(1.0_f32, self.stroke))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(egui::Margin::same(20))
    }

    pub fn sidebar_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.sidebar)
            .inner_margin(egui::Margin::same(16))
    }

    /// Paint the app background: a vertical gradient done as ~64 strips.
    /// epaint 0.31 ships no gradient primitive, and a mesh with per-vertex
    /// colour costs more than 64 rects at this size.
    pub fn paint_background(&self, ctx: &egui::Context) {
        let rect = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(rect, 0.0, self.bg_bottom);
        const STRIPS: usize = 64;
        let strip_h = rect.height() / STRIPS as f32;
        for i in 0..STRIPS {
            let t = i as f32 / (STRIPS - 1) as f32;
            let color = lerp_color(self.bg_top, self.bg_bottom, t);
            let y = rect.top() + strip_h * i as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), y),
                    egui::pos2(rect.right(), (y + strip_h).min(rect.bottom())),
                ),
                0.0,
                color,
            );
        }
    }
}

/// Big screen title, e.g. "Balance".
pub fn title(ui: &mut egui::Ui, text: &str, t: &Theme) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(30.0).color(t.text));
    ui.add_space(2.0);
}

/// Small uppercase section label.
pub fn section_label(ui: &mut egui::Ui, text: &str, t: &Theme) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .color(t.text_dim)
            .strong(),
    );
}

/// A sidebar nav row. Returns true when clicked.
pub fn nav_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    selected: bool,
    t: &Theme,
) -> bool {
    let (fill, text_color) = if selected {
        (t.accent.gamma_multiply(0.22), t.text)
    } else {
        (Color32::TRANSPARENT, t.text_dim)
    };
    let response = ui
        .scope(|ui| {
            egui::Frame::new()
                .fill(fill)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(15.0).color(if selected { t.accent } else { t.text_dim }));
                        ui.label(RichText::new(label).size(14.0).color(text_color));
                    });
                })
                .response
        })
        .response;
    if response.clicked() {
        true
    } else {
        response.hovered()
    }
}

/// Primary accent button, sized like an Apple filled button.
pub fn primary_button(ui: &mut egui::Ui, label: &str, t: &Theme) -> bool {
    let btn = egui::Button::new(
        RichText::new(label).size(15.0).color(Color32::WHITE).strong(),
    )
    .fill(t.accent)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(11))
    .min_size(Vec2::new(180.0, 40.0));
    ui.add(btn).clicked()
}

/// Secondary, quieter button.
pub fn secondary_button(ui: &mut egui::Ui, label: &str, t: &Theme) -> bool {
    let btn = egui::Button::new(RichText::new(label).size(14.0).color(t.text))
        .fill(t.card_hover)
        .stroke(Stroke::new(1.0_f32, t.stroke))
        .corner_radius(CornerRadius::same(11))
        .min_size(Vec2::new(140.0, 36.0));
    ui.add(btn).clicked()
}

/// Right-aligned dim monospace metadata (a `trx_id`, a gateway host).
pub fn meta_label(ui: &mut egui::Ui, text: &str, t: &Theme) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            RichText::new(text)
                .size(11.0)
                .color(t.text_dim)
                .monospace(),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_palette_is_legible_on_dark() {
        let t = Theme::dark();
        // Luma of text must clear the card fill by a wide margin, or the UI is
        // unreadable. Computed on the sRGB byte values, not perceptually.
        let luma = |c: Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
        assert!(luma(t.text) - luma(t.card) > 80.0);
        assert!(luma(t.text_dim) - luma(t.card) > 30.0);
    }

    #[test]
    fn lerp_hits_both_ends_and_midpoint() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(100, 200, 40);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
        let mid = lerp_color(a, b, 0.5);
        assert_eq!(mid, Color32::from_rgb(50, 100, 20));
    }

    #[test]
    fn lerp_clamps_out_of_range_t() {
        let a = Color32::from_rgb(10, 10, 10);
        let b = Color32::from_rgb(20, 20, 20);
        assert_eq!(lerp_color(a, b, -1.0), a);
        assert_eq!(lerp_color(a, b, 2.0), b);
    }

    #[test]
    fn accent_is_distinct_from_state_colors() {
        let t = Theme::dark();
        assert_ne!(t.accent, t.positive);
        assert_ne!(t.accent, t.negative);
        assert_ne!(t.accent, t.warning);
    }
}