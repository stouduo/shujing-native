//! painter 直绘的小图标:不依赖系统字形(缺字形时不会出现豆腐块),
//! 全部按原型的 1px 线性风格绘制,并与 PingFang/Helvetica 文字基线对齐。

use crate::theme;
use egui::{Color32, Pos2, Sense, Shape, Stroke, Ui, Vec2};

/// 展开/收起三角(自绘;位置由调用方给定)
pub fn paint_triangle(p: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    let s = 3.0;
    let (x, y) = (center.x, center.y);
    let pts = if open {
        vec![
            Pos2::new(x - s, y - s / 2.0),
            Pos2::new(x + s, y - s / 2.0),
            Pos2::new(x, y + s),
        ]
    } else {
        vec![
            Pos2::new(x - s / 2.0, y - s),
            Pos2::new(x - s / 2.0, y + s),
            Pos2::new(x + s, y),
        ]
    };
    p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// 表格 glyph(原型:圆角方框 + 竖线 + 横线,左上小格填充)
pub fn paint_table(p: &egui::Painter, rect: egui::Rect, color: Color32) {
    let r = rect.shrink(1.0);
    let st = Stroke::new(1.1, color);
    p.rect_stroke(r, egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
    let mid_y = r.top() + (r.height() * 0.5).round();
    let vx = r.left() + (r.width() * 0.42).round();
    p.line_segment(
        [
            Pos2::new(r.left() + 1.0, mid_y),
            Pos2::new(r.right() - 1.0, mid_y),
        ],
        st,
    );
    p.line_segment(
        [
            Pos2::new(vx, r.top() + 1.0),
            Pos2::new(vx, r.bottom() - 1.0),
        ],
        st,
    );
    p.rect_filled(
        egui::Rect::from_min_max(
            Pos2::new(r.left() + 1.0, r.top() + 1.0),
            Pos2::new(vx - 1.0, mid_y - 1.0),
        ),
        egui::CornerRadius::same(1),
        color.gamma_multiply(0.55),
    );
}

/// 标签页类型 glyph:表(网格)/查询(播放)/结构(横线);绝对位置绘制
pub fn tab_glyph(ui: &mut Ui, kind: u8, color: egui::Color32, left: f32, center_y: f32) {
    let rect = egui::Rect::from_min_size(egui::pos2(left, center_y - 6.0), Vec2::splat(12.0));
    let p = ui.painter();
    match kind {
        0 => paint_table(p, rect, color),
        1 => {
            let c = rect.center();
            let s = 3.2;
            p.add(Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - s / 2.0, c.y - s),
                    egui::pos2(c.x - s / 2.0, c.y + s),
                    egui::pos2(c.x + s, c.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        3 => {
            // 设计器:铅笔(斜杆 + 笔尖三角)
            let c = rect.center();
            let st = Stroke::new(1.2, color);
            p.line_segment(
                [egui::pos2(c.x - 3.5, c.y + 3.5), egui::pos2(c.x + 2.0, c.y - 4.0)],
                st,
            );
            p.line_segment(
                [egui::pos2(c.x - 1.0, c.y - 5.8), egui::pos2(c.x + 2.0, c.y - 4.0)],
                st,
            );
            p.line_segment(
                [egui::pos2(c.x - 3.5, c.y + 3.5), egui::pos2(c.x - 1.0, c.y - 5.8)],
                st,
            );
            p.line_segment(
                [egui::pos2(c.x - 5.0, c.y + 5.5), egui::pos2(c.x - 3.5, c.y + 3.5)],
                st,
            );
        }
        5 => {
            // ER 图:三节点小图
            let c = rect.center();
            let st = Stroke::new(1.1, color);
            p.rect_stroke(
                egui::Rect::from_min_size(egui::pos2(c.x - 5.5, c.y - 4.5), Vec2::splat(5.0)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                egui::Rect::from_min_size(egui::pos2(c.x + 1.0, c.y - 4.5), Vec2::splat(5.0)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                egui::Rect::from_min_size(egui::pos2(c.x - 2.5, c.y + 1.0), Vec2::splat(5.0)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
        }
        4 => {
            // Redis:菱形(品牌红钻)
            let c = rect.center();
            let s = 5.0;
            p.add(Shape::convex_polygon(
                vec![
                    egui::pos2(c.x, c.y - s),
                    egui::pos2(c.x + s, c.y),
                    egui::pos2(c.x, c.y + s),
                    egui::pos2(c.x - s, c.y),
                ],
                Color32::TRANSPARENT,
                Stroke::new(1.2, color),
            ));
        }
        _ => {
            let c = rect.center();
            let st = Stroke::new(1.1, color);
            for dy in [-3.0, 0.0, 3.0] {
                let y = c.y + dy;
                let shrink = if dy == 0.0 { 0.0 } else { 1.5 };
                p.line_segment(
                    [
                        egui::pos2(rect.left() + 1.0 + shrink, y),
                        egui::pos2(rect.right() - 1.0 - shrink, y),
                    ],
                    st,
                );
            }
        }
    }
}

/// 漏斗 glyph(筛选语义),宽 12,高 12
pub fn funnel_glyph(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let st = Stroke::new(1.2, color);
    let (x, y) = (center.x, center.y - 1.0);
    p.line_segment(
        [egui::pos2(x - 5.0, y - 3.5), egui::pos2(x + 5.0, y - 3.5)],
        st,
    );
    p.line_segment(
        [egui::pos2(x - 5.0, y - 3.5), egui::pos2(x - 1.6, y + 0.8)],
        st,
    );
    p.line_segment(
        [egui::pos2(x + 5.0, y - 3.5), egui::pos2(x + 1.6, y + 0.8)],
        st,
    );
    p.line_segment(
        [egui::pos2(x - 1.6, y + 0.8), egui::pos2(x - 1.6, y + 4.5)],
        st,
    );
    p.line_segment(
        [egui::pos2(x + 1.6, y + 0.8), egui::pos2(x + 1.6, y + 2.8)],
        st,
    );
    p.line_segment(
        [egui::pos2(x - 1.6, y + 4.5), egui::pos2(x + 1.6, y + 2.8)],
        st,
    );
}

/// 放大镜 glyph(搜索框内)
pub fn magnifier(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let st = Stroke::new(1.3, color);
    p.circle_stroke(egui::pos2(center.x - 1.0, center.y - 1.0), 3.6, st);
    p.line_segment(
        [
            egui::pos2(center.x + 1.9, center.y + 1.9),
            egui::pos2(center.x + 5.2, center.y + 5.2),
        ],
        st,
    );
}

/// 画一个关闭叉(位置/尺寸由调用方给定)
pub fn paint_close(p: &egui::Painter, center: egui::Pos2, half: f32, color: Color32) {
    let st = Stroke::new(1.2, color);
    p.line_segment(
        [
            egui::pos2(center.x - half, center.y - half),
            egui::pos2(center.x + half, center.y + half),
        ],
        st,
    );
    p.line_segment(
        [
            egui::pos2(center.x - half, center.y + half),
            egui::pos2(center.x + half, center.y - half),
        ],
        st,
    );
}

/// 画一个加号
pub fn paint_plus(p: &egui::Painter, center: egui::Pos2, half: f32, color: Color32) {
    let st = Stroke::new(1.4, color);
    p.line_segment(
        [
            egui::pos2(center.x - half, center.y),
            egui::pos2(center.x + half, center.y),
        ],
        st,
    );
    p.line_segment(
        [
            egui::pos2(center.x, center.y - half),
            egui::pos2(center.x, center.y + half),
        ],
        st,
    );
}

/// 画一个减号
pub fn paint_minus(p: &egui::Painter, center: egui::Pos2, half: f32, color: Color32) {
    p.line_segment(
        [
            egui::pos2(center.x - half, center.y),
            egui::pos2(center.x + half, center.y),
        ],
        Stroke::new(1.4, color),
    );
}

/// 行内小 chip(圆角底 + 文字),垂直居中于当前行
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let p = theme::c();
    let g = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(10.0),
        Color32::PLACEHOLDER,
    );
    let w = g.size().x + 14.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 18.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), theme::tint(p.panel, 90));
    ui.painter().galley(
        egui::pos2(
            rect.center().x - g.size().x / 2.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        color,
    );
    resp
}

/// 刷新图标:3/4 圆弧 + 箭头(手绘,避免字形基线问题)
pub fn refresh(ui: &mut Ui, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let col = if resp.hovered() {
        theme::c().text
    } else {
        color
    };
    let p = ui.painter();
    let c = rect.center();
    let r = 6.0;
    let st = Stroke::new(1.4, col);
    let start = -std::f32::consts::FRAC_PI_2 + 0.55;
    let end = start + std::f32::consts::TAU * 0.78;
    let mut pts = Vec::new();
    let n = 28;
    for i in 0..=n {
        let a = start + (end - start) * i as f32 / n as f32;
        pts.push(egui::pos2(c.x + a.cos() * r, c.y + a.sin() * r));
    }
    p.add(Shape::line(pts, st));
    // 箭头(弧尾切线方向)
    let a = end;
    let tip = egui::pos2(c.x + a.cos() * (r + 1.6), c.y + a.sin() * (r + 1.6));
    let s = 3.2;
    let dx = -a.sin();
    let dy = a.cos();
    let nx = a.cos();
    let ny = a.sin();
    let base = egui::pos2(tip.x - dx * s * 0.6, tip.y - dy * s * 0.6);
    p.add(Shape::convex_polygon(
        vec![
            tip,
            egui::pos2(base.x + nx * s, base.y + ny * s),
            egui::pos2(base.x - nx * s, base.y - ny * s),
        ],
        col,
        Stroke::NONE,
    ));
    resp
}

/// ‹ / › 翻页箭头
pub fn chevron(ui: &mut Ui, left: bool, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(22.0, 24.0), Sense::click());
    let col = if resp.hovered() {
        theme::c().text
    } else {
        color
    };
    let p = ui.painter();
    let st = Stroke::new(1.5, col);
    let (c, d) = (rect.center(), 3.4);
    if left {
        p.line_segment(
            [
                egui::pos2(c.x + d * 0.5, c.y - d),
                egui::pos2(c.x - d * 0.5, c.y),
            ],
            st,
        );
        p.line_segment(
            [
                egui::pos2(c.x - d * 0.5, c.y),
                egui::pos2(c.x + d * 0.5, c.y + d),
            ],
            st,
        );
    } else {
        p.line_segment(
            [
                egui::pos2(c.x - d * 0.5, c.y - d),
                egui::pos2(c.x + d * 0.5, c.y),
            ],
            st,
        );
        p.line_segment(
            [
                egui::pos2(c.x + d * 0.5, c.y),
                egui::pos2(c.x - d * 0.5, c.y + d),
            ],
            st,
        );
    }
    resp
}

/// 是否中日韩表意文字/全角标点(用于修正回退字体的基线)
pub fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
    )
}

/// 文本基线相对 galley 顶部的距离(含中文回退字体的视觉修正)
pub fn baseline_top(g: &egui::Galley, size: f32) -> f32 {
    let first = g
        .rows
        .first()
        .and_then(|r| r.glyphs.iter().find(|gl| gl.chr != ' '));
    match first {
        Some(gl) => {
            let mut base = gl.pos.y;
            if is_cjk(gl.chr) {
                base += size * crate::theme::CJK_BASELINE_FACTOR;
            }
            base
        }
        None => size * 0.8,
    }
}

/// 单行文本排版:超宽用 … 省略(原型表格长 URL 的截断方式)
pub fn text_galley(
    p: &egui::Painter,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    p.layout_job(job)
}

/// 在 32pt 行内按统一基线画文字(等宽/比例/中文混排时基线一致)
pub fn paint_baseline_text(
    p: &egui::Painter,
    rect: egui::Rect,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
    pad_x: f32,
    max_width: f32,
) {
    let size = font.size;
    let g = text_galley(p, text, font, color, max_width);
    let baseline = rect.center().y + 4.5;
    p.galley(
        egui::pos2(rect.left() + pad_x, baseline - baseline_top(&g, size)),
        g,
        color,
    );
}

/// 在工具条/行内画一段与主标题共基线的文字(不参与垂直居中)
pub fn label_at_baseline(
    ui: &mut Ui,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
    baseline: f32,
    max_width: f32,
) -> egui::Response {
    let size = font.size;
    let g = text_galley(ui.painter(), text, font, Color32::PLACEHOLDER, max_width);
    let (rect, resp) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(
        egui::pos2(rect.left(), baseline - baseline_top(&g, size)),
        g,
        color,
    );
    resp
}

/// 与主标题共基线的 chip(药丸底仍按文字视觉位置定,不随行高居中)
pub fn chip_at_baseline(ui: &mut Ui, text: &str, color: Color32, baseline: f32) -> egui::Response {
    let p = theme::c();
    let font = egui::FontId::proportional(11.5);
    let g = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER);
    let size = g.size() + Vec2::new(16.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let text_top = baseline - baseline_top(&g, 11.5);
    let pill = egui::Rect::from_min_size(
        egui::pos2(rect.left(), text_top - 4.0),
        egui::vec2(size.x, g.size().y + 8.0),
    );
    ui.painter()
        .rect_filled(pill, egui::CornerRadius::same(6), p.elevated);
    ui.painter()
        .galley(egui::pos2(pill.left() + 8.0, text_top), g, color);
    resp
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

/// 千分位(原型"共 86,241 行")
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// 单元格文本颜色:NULL 弱、修改中绿
pub fn cell_text_color(v: &Option<String>, changed: bool) -> Color32 {
    if changed {
        theme::c().change_fg
    } else if v.is_none() {
        theme::c().null_text
    } else {
        theme::c().cell_text
    }
}

pub fn cell_text(v: &Option<String>, max_chars: usize) -> String {
    match v {
        Some(s) => clip(s, max_chars),
        None => "null".into(),
    }
}
