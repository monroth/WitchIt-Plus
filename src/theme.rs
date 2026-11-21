// The window's look: a dark witchy purple with teal and warm orange, rounded cards, Nunito.
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::Arc;

pub const BACKGROUND: Color32 = Color32::from_rgb(0x1d, 0x15, 0x2b);
pub const PANEL: Color32 = Color32::from_rgb(0x28, 0x1d, 0x3b);
pub const CARD: Color32 = Color32::from_rgb(0x33, 0x26, 0x4b);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x3d, 0x2e, 0x59);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xe4, 0xf7);
pub const MUTED: Color32 = Color32::from_rgb(0xa8, 0x9b, 0xc2);
pub const TEAL: Color32 = Color32::from_rgb(0x4f, 0xd1, 0xc5);
pub const ORANGE: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x4c);
pub const RED: Color32 = Color32::from_rgb(0xf2, 0x6d, 0x7d);
pub const GREEN: Color32 = Color32::from_rgb(0x8b, 0xd8, 0x6f);

pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("nunito".into(), Arc::new(FontData::from_static(include_bytes!("../assets/Nunito-Regular.ttf"))));
    fonts.font_data.insert("nunito-bold".into(), Arc::new(FontData::from_static(include_bytes!("../assets/Nunito-Bold.ttf"))));
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "nunito".into());
    fonts.families.insert(FontFamily::Name("bold".into()), vec!["nunito-bold".into(), "nunito".into()]);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
    style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
    style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
    style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::new(24.0, FontFamily::Name("bold".into())));
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.interact_size.y = 30.0;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = BACKGROUND;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x15, 0x0f, 0x20);
    v.faint_bg_color = PANEL;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = TEAL;
    v.selection.bg_fill = Color32::from_rgb(0x5b, 0x3f, 0x8c);
    v.selection.stroke = Stroke::new(1.0_f32, TEAL);
    v.window_corner_radius = CornerRadius::same(14);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x4a, 0x38, 0x6b));
    for (w, fill) in [(&mut v.widgets.inactive, CARD), (&mut v.widgets.hovered, CARD_HOVER), (&mut v.widgets.active, Color32::from_rgb(0x4a, 0x38, 0x6b))] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(10);
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.noninteractive.corner_radius = CornerRadius::same(10);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x3a, 0x2c, 0x54));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, TEAL);
    ctx.set_style(style);
}

/// A card: the frame around one mod, preset or source.
pub fn card() -> egui::Frame {
    egui::Frame::new().fill(CARD).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(12))
}

/// A small coloured label, with what it means on hover.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32, meaning: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.22))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.7)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| { ui.label(egui::RichText::new(text).small().color(color)); })
        .response
        .on_hover_text(meaning);
}

/// The big call to action (Play, Apply).
pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).size(18.0).strong().color(Color32::from_rgb(0x1d, 0x15, 0x2b)))
        .fill(ORANGE).corner_radius(CornerRadius::same(12)).min_size(egui::vec2(140.0, 40.0)))
}

pub fn bold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(FontFamily::Name("bold".into()))
}
