//! painter 直绘的小图标:不依赖系统字形(缺字形时不会出现豆腐块),
//! 全部按原型的 1px 线性风格绘制,并与 PingFang/Helvetica 文字基线对齐。

use crate::theme;
use crate::theme::{fs_px, radius_ctrl};

use crate::lucide::Icon;
use egui::{Color32, Pos2, Sense, Shape, Stroke, Ui, Vec2};

// ---------------------------------------------------------------- Lucide 线性图标

/// 图标的标准显示尺寸(工具栏、菜单、表头都用这一档;行内小图标用 12)
pub const ICON_SIZE: f32 = 16.0;

/// 画一个 Lucide 图标:以 `c` 为中心、外接尺寸 `size`。线宽随尺寸等比(24 网格上描边 2),
/// 圆头圆角与原版一致
pub fn paint_icon(p: &egui::Painter, icon: &Icon, c: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let stroke = Stroke::new(2.0 * k, color);
    for line in icon.lines {
        let Some(first) = line.first() else {
            continue;
        };
        if line.len() < 2 {
            // 退化的单点:画一个圆点
            p.circle_filled(
                Pos2::new(c.x + (first[0] - 12.0) * k, c.y + (first[1] - 12.0) * k),
                stroke.width / 2.0,
                color,
            );
            continue;
        }
        let pts: Vec<Pos2> = line
            .iter()
            .map(|&[x, y]| Pos2::new(c.x + (x - 12.0) * k, c.y + (y - 12.0) * k))
            .collect();
        p.add(Shape::line(pts, stroke));
    }
}

/// 图标按钮的底:`w × 28` 的可点区,悬停/按住时垫淡底。返回 (矩形, 响应, 是否高亮)
pub(crate) fn tool_face(ui: &mut Ui, w: f32) -> (egui::Rect, egui::Response, bool) {
    let p = theme::c();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    let hot = resp.hovered() || resp.is_pointer_button_down_on();
    if hot {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(5),
            theme::tint(p.text, 16),
        );
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    (rect, resp, hot)
}

/// 图标按钮(28×28),悬停才提亮并垫淡底。图标不是文字,所以补无障碍标签
/// (读屏与 kittest 靠它找到按钮)
pub fn icon_button(ui: &mut Ui, icon: &Icon, tip: &str) -> egui::Response {
    let p = theme::c();
    let (rect, resp, hot) = tool_face(ui, 28.0);
    paint_icon(
        ui.painter(),
        icon,
        rect.center(),
        ICON_SIZE,
        if hot { p.text } else { p.text2 },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip.to_owned())
    });
    resp.clone().on_hover_text(tip);
    resp
}

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
        6 => {
            // DDL:文档卷轴(矩形+折角线)
            let c = rect.center();
            let st = Stroke::new(1.1, color);
            p.rect_stroke(
                egui::Rect::from_min_size(egui::pos2(c.x - 4.0, c.y - 5.5), egui::vec2(8.0, 11.0)),
                1.5,
                st,
                egui::StrokeKind::Inside,
            );
            for dy in [-2.5, 0.5, 3.5] {
                p.line_segment(
                    [egui::pos2(c.x - 2.0, c.y + dy), egui::pos2(c.x + 2.0, c.y + dy)],
                    st,
                );
            }
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

/// 眼睛图标(密码 显示/隐藏),宽 13;open = 睁眼(密码明文),否则划一道斜线
pub fn eye_glyph(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let st = Stroke::new(1.2, color);
    let (x, y) = (center.x, center.y);
    let lid = |s: f32| {
        egui::Shape::Path(egui::epaint::PathShape {
            points: vec![
                egui::pos2(x - 6.5, y),
                egui::pos2(x - 3.5, y + s * 3.0),
                egui::pos2(x, y + s * 4.2),
                egui::pos2(x + 3.5, y + s * 3.0),
                egui::pos2(x + 6.5, y),
            ],
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: st.into(),
        })
    };
    p.add(lid(-1.0));
    p.add(lid(1.0));
    p.circle_filled(egui::pos2(x, y), 1.9, color);
}

/// "密码隐藏"的眼睛:睁眼 + 一道斜线
pub fn eye_off_glyph(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    eye_glyph(p, center, color);
    p.line_segment(
        [
            egui::pos2(center.x - 6.0, center.y + 5.0),
            egui::pos2(center.x + 6.0, center.y - 5.0),
        ],
        Stroke::new(1.2, color),
    );
}

/// 漏斗 glyph(筛选语义,12×12;内部已是 Lucide FILTER)
pub fn funnel_glyph(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    paint_icon(p, &crate::lucide::FILTER, center, 12.0, color);
}

/// 放大镜 glyph(搜索框内;Lucide SEARCH)
pub fn magnifier(p: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    paint_icon(p, &crate::lucide::SEARCH, center, 13.0, color);
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
        egui::FontId::proportional(fs_px(10.0)),
        Color32::PLACEHOLDER,
    );
    let w = g.size().x + 14.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, theme::mini_h()), Sense::hover());
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

/// 刷新按钮(26×26,调用方自己挂提示)。调用点多,保留这个名字;图标已是 Lucide 的
pub fn refresh(ui: &mut Ui, color: Color32) -> egui::Response {
    let (rect, resp, hot) = tool_face(ui, 26.0);
    paint_icon(
        ui.painter(),
        &crate::lucide::REFRESH,
        rect.center(),
        ICON_SIZE,
        if hot { theme::c().text } else { color },
    );
    resp
}

/// ‹ / › 翻页箭头(22×24)
pub fn chevron(ui: &mut Ui, left: bool, color: Color32) -> egui::Response {
    let (rect, resp, hot) = tool_face(ui, 22.0);
    let icon = if left {
        &crate::lucide::CHEVRON_LEFT
    } else {
        &crate::lucide::CHEVRON_RIGHT
    };
    paint_icon(
        ui.painter(),
        icon,
        rect.center(),
        ICON_SIZE,
        if hot { theme::c().text } else { color },
    );
    resp
}

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
    let font = egui::FontId::proportional(fs_px(11.5));
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
        .rect_filled(pill, egui::CornerRadius::same(radius_ctrl() as u8), p.elevated);
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
        None => "NULL".into(),
    }
}

// ---------------------------------------------------------------- 顶部工具栏矢量图标

pub fn icon_plus_sq(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    let r = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    p.rect_stroke(r, egui::CornerRadius::same(3), st, egui::StrokeKind::Inside);
    let s = 2.6;
    p.line_segment([egui::pos2(c.x - s, c.y), egui::pos2(c.x + s, c.y)], st);
    p.line_segment([egui::pos2(c.x, c.y - s), egui::pos2(c.x, c.y + s)], st);
}
pub fn icon_doc_plus(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    let r = egui::Rect::from_center_size(egui::pos2(c.x - 1.5, c.y), egui::vec2(10.0, 12.0));
    p.rect_stroke(r, egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
    p.line_segment(
        [egui::pos2(r.left() + 2.5, r.top() + 4.0), egui::pos2(r.right() - 2.5, r.top() + 4.0)],
        st,
    );
    p.line_segment(
        [egui::pos2(r.left() + 2.5, r.top() + 7.0), egui::pos2(r.right() - 2.5, r.top() + 7.0)],
        st,
    );
    // 右下角的加号:先用工具栏底色垫一圈,免得和文档边框糊在一起
    let q = egui::pos2(r.right() + 0.5, r.bottom());
    p.circle_filled(q, 3.6, crate::theme::c().chrome);
    let s = 2.4;
    p.line_segment([egui::pos2(q.x - s, q.y), egui::pos2(q.x + s, q.y)], st);
    p.line_segment([egui::pos2(q.x, q.y - s), egui::pos2(q.x, q.y + s)], st);
}
pub fn icon_swap(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    let (x, y) = (c.x, c.y);
    p.line_segment([egui::pos2(x - 6.0, y - 2.5), egui::pos2(x + 6.0, y - 2.5)], st);
    p.line_segment([egui::pos2(x + 3.0, y - 5.5), egui::pos2(x + 6.0, y - 2.5)], st);
    p.line_segment([egui::pos2(x + 3.0, y + 0.5), egui::pos2(x + 6.0, y - 2.5)], st);
    p.line_segment([egui::pos2(x + 6.0, y + 3.0), egui::pos2(x - 6.0, y + 3.0)], st);
    p.line_segment([egui::pos2(x - 3.0, y), egui::pos2(x - 6.0, y + 3.0)], st);
    p.line_segment([egui::pos2(x - 3.0, y + 6.0), egui::pos2(x - 6.0, y + 3.0)], st);
}
pub fn icon_diff(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    let a = egui::Rect::from_min_max(egui::pos2(c.x - 6.5, c.y - 6.5), egui::pos2(c.x + 2.5, c.y + 2.5));
    let b = egui::Rect::from_min_max(egui::pos2(c.x - 2.5, c.y - 2.5), egui::pos2(c.x + 6.5, c.y + 6.5));
    p.rect_filled(b, egui::CornerRadius::same(2), crate::theme::c().chrome);
    p.rect_stroke(a, egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
    p.rect_stroke(b, egui::CornerRadius::same(2), st, egui::StrokeKind::Inside);
}
pub fn icon_box(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    let r = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    p.rect_stroke(r, egui::CornerRadius::same(3), st, egui::StrokeKind::Inside);
    p.line_segment([r.left_center(), r.right_center()], st);
    p.line_segment([egui::pos2(c.x, r.top()), egui::pos2(c.x, r.bottom())], st);
}
