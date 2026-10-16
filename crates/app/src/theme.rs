//! iOS(Cupertino)深色设计令牌:纯黑背景 + systemBackground 层级 + 系统色。
//! 色值取自 iOS 暗色模式:secondarySystemBackground #1C1C1E / separator #38383A /
//! systemBlue #0A84FF / systemGreen #30D158 / systemRed #FF453A。

use egui::{style::Selection, Color32, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const PANEL: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);
pub const ELEVATED: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2e);
pub const HOVER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3c);
pub const BORDER: Color32 = Color32::from_rgb(0x38, 0x38, 0x3a);
pub const TEXT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const TEXT2: Color32 = Color32::from_rgb(0x8e, 0x8e, 0x93);
pub const MUTED: Color32 = Color32::from_rgb(0x63, 0x63, 0x66);
pub const ACCENT: Color32 = Color32::from_rgb(0x0a, 0x84, 0xff);
pub const GREEN: Color32 = Color32::from_rgb(0x30, 0xd1, 0x58);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);

/// 修改中单元格(绿色系,黑底上调暗)
pub const CHANGE_BG: Color32 = Color32::from_rgb(0x0f, 0x2a, 0x1b);
pub const CHANGE_FG: Color32 = Color32::from_rgb(0x7c, 0xe0, 0xa8);

/// 行悬停/格悬停/选中格(预乘,白/系统蓝低透明度)
pub const ROW_HOVER: Color32 = Color32::from_rgba_premultiplied(16, 16, 18, 22);
pub const CELL_HOVER: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 24);
pub const CELL_SELECT: Color32 = Color32::from_rgba_premultiplied(2, 29, 56, 64);

/// 数据行发丝线(比 separator 更轻,密集行场景不抢视觉)
pub const ROW_LINE: Color32 = Color32::from_rgb(0x22, 0x22, 0x24);

/// 侧栏当前表选中行(系统蓝浸染)
pub const ACTIVE_ROW: Color32 = Color32::from_rgba_premultiplied(2, 29, 56, 56);

/// SQL 高亮配色(Apple 开发者配色:蓝关键字/青字符串/橙数字/灰注释)
pub const SQL_KEYWORD: Color32 = Color32::from_rgb(0x42, 0x9b, 0xff);
pub const SQL_STRING: Color32 = Color32::from_rgb(0x64, 0xd2, 0xff);
pub const SQL_NUMBER: Color32 = Color32::from_rgb(0xff, 0x9f, 0x0a);
pub const SQL_COMMENT: Color32 = Color32::from_rgb(0x63, 0x63, 0x66);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = BG;
    v.faint_bg_color = Color32::from_rgb(0x1a, 0x1a, 0x1e);
    v.extreme_bg_color = Color32::from_rgb(0x00, 0x00, 0x00);
    v.window_fill = ELEVATED;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = Color32::from_rgb(0xff, 0xd6, 0x0a);
    v.error_fg_color = RED;
    v.selection = Selection {
        bg_fill: ACCENT.gamma_multiply(0.25),
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
    w.active.bg_fill = ACCENT.gamma_multiply(0.28);
    w.active.weak_bg_fill = ACCENT.gamma_multiply(0.28);
    w.active.fg_stroke = Stroke::new(1.0, TEXT);

    // iOS 圆角语言:控件 8,弹窗 12,菜单 10
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(10);

    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.menu_margin = egui::Margin::symmetric(6, 4);
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    ctx.set_style(style);
}

/// iOS 发丝分隔线(separator):画在当前 ui 底边,横向铺满
pub fn hairline_bottom(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [egui::pos2(r.left() - 12.0, r.bottom()), egui::pos2(r.right() + 12.0, r.bottom())],
        egui::Stroke::new(1.0, BORDER),
    );
}

/// iOS 发丝分隔线:画在当前 ui 顶边
pub fn hairline_top(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [egui::pos2(r.left() - 12.0, r.top()), egui::pos2(r.right() + 12.0, r.top())],
        egui::Stroke::new(1.0, BORDER),
    );
}

/// iOS 导航栏式文字按钮(系统蓝、无底),返回是否点击
pub fn text_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).color(ACCENT)).fill(egui::Color32::TRANSPARENT))
        .clicked()
}
