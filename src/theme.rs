//! Colors and global styling for the app's dark theme.

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontId, Stroke, TextStyle, vec2,
};

pub const BG: Color32 = Color32::from_rgb(0x10, 0x12, 0x17);
pub const PANEL: Color32 = Color32::from_rgb(0x17, 0x1b, 0x24);
pub const CELL: Color32 = Color32::from_rgb(0x1c, 0x21, 0x2e);
pub const CELL_STROKE: Color32 = Color32::from_rgb(0x2a, 0x32, 0x44);
pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x9c, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xed, 0xf5);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x93, 0xa8);
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
pub const RED: Color32 = Color32::from_rgb(0xf2, 0x55, 0x4f);
pub const SLATE: Color32 = Color32::from_rgb(0x3a, 0x43, 0x58);

/// `Stroke::new` is generic over `Into<f32>`, which trips the numeric
/// fallback lint with float literals; this keeps call sites tidy.
pub fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke { width, color }
}

/// Color for a score relative to the advance/fallback thresholds.
pub fn score_color(score: u32, advance: u32, fallback: u32) -> Color32 {
    if score >= advance {
        GREEN
    } else if score < fallback {
        RED
    } else {
        TEXT
    }
}

pub fn apply(ctx: &Context) {
    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;

    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x12, 0x15, 0x1c);
    v.faint_bg_color = Color32::from_rgb(0x1a, 0x1f, 0x2a);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = stroke(1.0, ACCENT);
    v.slider_trailing_fill = true;
    v.hyperlink_color = ACCENT;

    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.bg_stroke = stroke(1.0, CELL_STROKE);
    v.widgets.inactive.fg_stroke.color = TEXT;
    v.widgets.inactive.bg_fill = CELL;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x20, 0x26, 0x33);
    v.widgets.inactive.bg_stroke = stroke(1.0, CELL_STROKE);
    v.widgets.hovered.fg_stroke.color = TEXT;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x32, 0x44);
    v.widgets.hovered.bg_stroke = stroke(1.0, ACCENT.gamma_multiply(0.6));
    v.widgets.active.fg_stroke.color = TEXT;
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x33, 0x3e, 0x57);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(9);
    }

    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(16.0, 9.0);
    style.spacing.slider_width = 240.0;

    style.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(26.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));

    ctx.set_global_style(style);
}
