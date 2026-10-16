//! painter 直绘的小图标:系统中文字体缺 ▤/▸ 等字形(显示成方框),符号一律不依赖字体。

use crate::theme::{ACCENT, TEXT, TEXT2};
use egui::{Color32, Pos2, Sense, Shape, Stroke, Ui, Vec2};

/// 展开/收起三角,可点击
pub fn triangle(ui: &mut Ui, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let color = if resp.hovered() { TEXT } else { TEXT2 };
    let s = 3.2;
    // PingFang 行框 metrics 使文字墨迹中心高于几何中心,图标随之上移对齐
    let (x, y) = (rect.center().x, rect.center().y - 1.5);
    let pts = if open {
        // ▼
        vec![Pos2::new(x - s, y - s / 2.0), Pos2::new(x + s, y - s / 2.0), Pos2::new(x, y + s)]
    } else {
        // ▸
        vec![Pos2::new(x - s / 2.0, y - s), Pos2::new(x - s / 2.0, y + s), Pos2::new(x + s, y)]
    };
    ui.painter().add(Shape::convex_polygon(pts, color, Stroke::NONE));
    resp
}

/// 连接状态圆点
pub fn dot(ui: &mut Ui, online: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center() - egui::vec2(0.0, 1.5), 4.0, if online { GREEN_DOT } else { MUTED_DOT });
}

const GREEN_DOT: Color32 = Color32::from_rgb(0x3f, 0xc7, 0x84);
const MUTED_DOT: Color32 = Color32::from_rgb(0x55, 0x56, 0x5c);

/// 表图标:小网格
pub fn table(ui: &mut Ui, active: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
    let color = if active { ACCENT } else { TEXT2 };
    let p = ui.painter();
    let st = Stroke::new(1.0, color);
    p.rect_stroke(rect.shrink(1.5).translate(egui::vec2(0.0, -1.5)), egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
    let mid_y = rect.center().y - 1.5;
    p.line_segment([Pos2::new(rect.left() + 1.5, mid_y), Pos2::new(rect.right() - 1.5, mid_y)], st);
    let vx = rect.left() + 1.5 + (rect.width() - 3.0) * 0.42;
    p.line_segment([Pos2::new(vx, rect.top() + 1.5), Pos2::new(vx, mid_y)], st);
}

/// 结构图标:三条横线
pub fn structure(ui: &mut Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let color = if resp.hovered() { TEXT } else { TEXT2 };
    let p = ui.painter();
    let st = Stroke::new(1.2, color);
    for dy in [-3.5, 0.0, 3.5] {
        let y = rect.center().y - 1.5 + dy;
        let shrink = if dy == 0.0 { 0.0 } else { 1.5 };
        p.line_segment(
            [Pos2::new(rect.left() + 1.0 + shrink, y), Pos2::new(rect.right() - 1.0 - shrink, y)],
            st,
        );
    }
    resp
}

/// 主键钥匙小标(字段行内)
pub fn key_badge(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 15.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(3),
        ACCENT.gamma_multiply(0.18),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "PK",
        egui::FontId::proportional(9.5),
        ACCENT,
    );
}

/// 圆角小胶囊标签(如 UNIQUE)
pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    let font = egui::FontId::proportional(9.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = galley.size() + Vec2::new(12.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(7), color.gamma_multiply(0.16));
    ui.painter().galley(rect.left_top() + Vec2::new(6.0, 3.0), galley, color);
}

/// 文本截断(按字符,避免多字节 panic);None→NULL 由调用方决定
pub fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let out: String = s.chars().take(max_chars).collect();
        format!("{out}…")
    }
}

/// 单元格文本颜色:NULL 弱、修改中绿
pub fn cell_text_color(v: &Option<String>, changed: bool) -> Color32 {
    if changed {
        crate::theme::CHANGE_FG
    } else if v.is_none() {
        Color32::from_rgb(0x55, 0x55, 0x5a)
    } else {
        TEXT2
    }
}

pub fn cell_text(v: &Option<String>, max_chars: usize) -> String {
    match v {
        Some(s) => clip(s, max_chars),
        None => "null".into(),
    }
}
