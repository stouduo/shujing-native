//! zinc/Linear 风格深色主题(对齐 Tauri 版视觉令牌)。
//! 颜色常量公开,供各视图取用,避免散落魔法数。

use egui::{style::Selection, Color32, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x11);
pub const PANEL: Color32 = Color32::from_rgb(0x11, 0x12, 0x14);
pub const ELEVATED: Color32 = Color32::from_rgb(0x16, 0x17, 0x1b);
pub const HOVER: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x21);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2b, 0x2e);
pub const TEXT: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf5);
pub const TEXT2: Color32 = Color32::from_rgb(0xc9, 0xc9, 0xcf);
pub const MUTED: Color32 = Color32::from_rgb(0x77, 0x78, 0x7f);
pub const ACCENT: Color32 = Color32::from_rgb(0x85, 0x87, 0xf6);
pub const GREEN: Color32 = Color32::from_rgb(0x4a, 0xd1, 0x8f);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x6b, 0x70);

/// 修改中单元格的底色/字色(底色为实色暗绿,近似 alpha 叠加)
pub const CHANGE_BG: Color32 = Color32::from_rgb(0x14, 0x2a, 0x20);
pub const CHANGE_FG: Color32 = Color32::from_rgb(0xa7, 0xf3, 0xd0);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = BG;
    v.faint_bg_color = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x0b, 0x0c, 0x0e);
    v.window_fill = ELEVATED;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = Color32::from_rgb(0xff, 0xd6, 0x0a);
    v.error_fg_color = RED;
    v.selection = Selection {
        bg_fill: ACCENT.gamma_multiply(0.32),
        stroke: Stroke::NONE,
    };

    let w = &mut style.visuals.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT2);
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT2);
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.bg_fill = ACCENT.gamma_multiply(0.30);
    w.active.weak_bg_fill = ACCENT.gamma_multiply(0.30);
    w.active.fg_stroke = Stroke::new(1.0, TEXT);

    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(6);
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(8);

    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(9.0, 3.0);
    style.spacing.menu_margin = egui::Margin::symmetric(6, 4);

    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(15.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(12.5)),
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    ctx.set_style(style);
}
