//! painter 直绘的小图标:系统中文字体缺 ▤/▸ 等字形(显示成方框),符号一律不依赖字体。

use crate::theme;
use egui::{Color32, Pos2, Sense, Shape, Stroke, Ui, Vec2};

/// 展开/收起三角,可点击
pub fn triangle(ui: &mut Ui, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let color = if resp.hovered() { theme::c().text } else { theme::c().text2 };
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
    let (on_c, off_c) = (theme::c().green, theme::c().dot_off);
    ui.painter().circle_filled(rect.center() - egui::vec2(0.0, 1.5), 4.0, if online { on_c } else { off_c });
}


/// 标签页类型 glyph:表(网格)/查询(播放)/结构(横线);绝对位置绘制
pub fn tab_glyph(ui: &mut Ui, kind: u8, color: egui::Color32, left: f32, center_y: f32) {
    let rect = egui::Rect::from_min_size(
        egui::pos2(left, center_y - 6.0),
        Vec2::splat(12.0),
    );
    let p = ui.painter();
    let st = Stroke::new(1.1, color);
    match kind {
        0 => {
            // 表:圆角矩形 + 中横 + 半竖
            p.rect_stroke(rect.shrink(1.0), egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
            let mid_y = rect.center().y;
            p.line_segment([egui::pos2(rect.left() + 1.0, mid_y), egui::pos2(rect.right() - 1.0, mid_y)], st);
            let vx = rect.left() + 1.0 + (rect.width() - 2.0) * 0.45;
            p.line_segment([egui::pos2(vx, rect.top() + 1.0), egui::pos2(vx, mid_y)], st);
        }
        1 => {
            // 查询:小三角
            let c = rect.center();
            let s = 3.2;
            p.add(Shape::convex_polygon(
                vec![egui::pos2(c.x - s / 2.0, c.y - s), egui::pos2(c.x - s / 2.0, c.y + s), egui::pos2(c.x + s, c.y)],
                color,
                Stroke::NONE,
            ));
        }
        _ => {
            // 结构:三横线
            let c = rect.center();
            for dy in [-3.0, 0.0, 3.0] {
                let y = c.y + dy;
                let shrink = if dy == 0.0 { 0.0 } else { 1.5 };
                p.line_segment(
                    [egui::pos2(rect.left() + 1.0 + shrink, y), egui::pos2(rect.right() - 1.0 - shrink, y)],
                    st,
                );
            }
        }
    }
}

/// 主题切换钮:系统字形 ☀/☾(Apple Symbols,与文字同一 metrics)
pub fn theme_toggle(ui: &mut Ui, dark: bool) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(if dark { "☀" } else { "☾" })
                .size(14.0)
                .color(theme::c().accent),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .min_size(egui::vec2(26.0, 24.0)),
    )
}

/// 漏斗 glyph(筛选语义)
pub fn funnel_glyph(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let st = Stroke::new(1.3, color);
    let (x, y) = (center.x, center.y);
    p.line_segment([egui::pos2(x - 4.0, y - 3.5), egui::pos2(x + 4.0, y - 3.5)], st);
    p.line_segment([egui::pos2(x - 4.0, y - 3.5), egui::pos2(x - 1.0, y + 0.5)], st);
    p.line_segment([egui::pos2(x + 4.0, y - 3.5), egui::pos2(x + 1.0, y + 0.5)], st);
    p.line_segment([egui::pos2(x - 1.0, y + 0.5), egui::pos2(x - 1.0, y + 4.0)], st);
    p.line_segment([egui::pos2(x + 1.0, y + 0.5), egui::pos2(x + 1.0, y + 2.5)], st);
    p.line_segment([egui::pos2(x - 1.0, y + 4.0), egui::pos2(x + 1.0, y + 2.5)], st);
}

/// 状态徽章(0/1/2 类短值):绿=on 红=off
pub fn status_badge(ui: &mut Ui, text: &str, on: bool) {
    let p = theme::c();
    let font = egui::FontId::proportional(10.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), p.text);
    let size = galley.size() + egui::vec2(10.0, 5.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let (bg, fg) = if on { (p.green, p.green) } else { (p.red, p.red) };
    ui.painter().rect_filled(rect, 5, bg.gamma_multiply(0.16));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, font, fg);
}

/// 放大镜 glyph(搜索框内)
pub fn magnifier(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let st = Stroke::new(1.3, color);
    p.circle_stroke(egui::pos2(center.x - 1.0, center.y - 1.0), 3.4, st);
    p.line_segment(
        [egui::pos2(center.x + 1.8, center.y + 1.8), egui::pos2(center.x + 5.0, center.y + 5.0)],
        st,
    );
}

/// iOS 式搜索框:圆角底 + 内嵌放大镜 + 无框输入
pub fn search_box(
    ui: &mut Ui,
    id_salt: &str,
    text: &mut String,
    placeholder: &str,
    width: f32,
) -> egui::Response {
    search_box_icon(ui, id_salt, text, placeholder, width, false)
}

/// 同上,funnel=true 用漏斗图标(筛选语义)
pub fn search_box_icon(
    ui: &mut Ui,
    id_salt: &str,
    text: &mut String,
    placeholder: &str,
    width: f32,
    funnel: bool,
) -> egui::Response {
    let h = 26.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), egui::Sense::hover());
    ui.painter().rect_filled(rect, 8, crate::theme::c().input_bg);
    if funnel {
        funnel_glyph(
            ui.painter(),
            egui::pos2(rect.left() + 12.0, rect.center().y),
            crate::theme::c().muted,
        );
    } else {
        magnifier(ui.painter(), egui::pos2(rect.left() + 12.0, rect.center().y), crate::theme::c().muted);
    }
    let inner = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 24.0, rect.top()),
        egui::Vec2::new(width - 30.0, h),
    );
    let resp = ui
        .allocate_new_ui(
            egui::UiBuilder::new().max_rect(inner),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .id(egui::Id::new(id_salt))
                        .hint_text(egui::RichText::new(placeholder).color(crate::theme::c().muted))
                        .desired_width(inner.width())
                        .frame(false)
                        .font(egui::TextStyle::Body),
                )
            },
        )
        .inner;
    resp
}

/// 刷新:系统字形 ↻(Apple Symbols)
pub fn refresh(ui: &mut Ui, color: egui::Color32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new("↻").size(15.0).color(color))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(egui::vec2(26.0, 24.0)),
    )
}

/// 更多 ⋯
pub fn dots(ui: &mut Ui, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::click());
    let p = ui.painter();
    let y = rect.center().y;
    for dx in [-4.0, 0.0, 4.0] {
        p.circle_filled(egui::pos2(rect.center().x + dx, y), 1.4, color);
    }
    resp
}

/// ‹ / ›
pub fn chevron(ui: &mut Ui, left: bool, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::click());
    let p = ui.painter();
    let st = Stroke::new(1.6, color);
    let (c, d) = (rect.center(), 3.0);
    if left {
        p.line_segment([egui::pos2(c.x + d * 0.6, c.y - d), egui::pos2(c.x - d * 0.6, c.y)], st);
        p.line_segment([egui::pos2(c.x - d * 0.6, c.y), egui::pos2(c.x + d * 0.6, c.y + d)], st);
    } else {
        p.line_segment([egui::pos2(c.x - d * 0.6, c.y - d), egui::pos2(c.x + d * 0.6, c.y)], st);
        p.line_segment([egui::pos2(c.x + d * 0.6, c.y), egui::pos2(c.x - d * 0.6, c.y + d)], st);
    }
    resp
}

/// 表图标:小网格
pub fn table(ui: &mut Ui, active: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
    let color = if active { theme::c().accent } else { theme::c().text2 };
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
    let color = if resp.hovered() { theme::c().text } else { theme::c().text2 };
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
        theme::c().accent.gamma_multiply(0.18),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "PK",
        egui::FontId::proportional(9.5),
        theme::c().accent,
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
        crate::theme::c().change_fg
    } else if v.is_none() {
        Color32::from_rgb(0x55, 0x55, 0x5a)
    } else {
        theme::c().text2
    }
}

pub fn cell_text(v: &Option<String>, max_chars: usize) -> String {
    match v {
        Some(s) => clip(s, max_chars),
        None => "null".into(),
    }
}
