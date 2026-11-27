//! 网格与页面骨架的共用绘制原语:列头/行号头/徽章/幽灵按钮/芯片、
//! 结果网格的表头-行底色-单元格-行号绘制族、列宽启发式。
//! 全部是纯参数(&Painter/&Ui + Rect/Color32/String),app 经 theme 转发层使用。

use crate::icons::{tool_face, ICON_SIZE};
use crate::lucide::Icon;
use crate::theme::{chip_h, fs_px, radius_ctrl, radius_small};
use egui::{Color32, RichText, Sense};


/// 幽灵文字按钮(DBX 工具栏款:无边框无底色,悬停淡底;12px)
pub fn ghost_button(ui: &mut egui::Ui, label: impl Into<String>) -> egui::Response {
    let p = crate::theme::c();
    let g = ui
        .painter()
        .layout_no_wrap(label.into(), egui::FontId::proportional(fs_px(12.0)), p.text);
    // 左右各 6pt 内边距(DBX 的工具栏按钮):相邻按钮之间才留得出缝
    // (egui Button 关掉 frame 后内边距也没了,英文态两个按钮会贴成一团)
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(g.size().x + 12.0, chip_h()),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius_ctrl() as u8), crate::theme::tint(p.text, 16));
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0),
        g,
        p.text,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
/// 查询栏 WHERE 芯片(漏斗 + 等宽 WHERE,DBX 蓝);有生效条件时点亮,筛选栏打开时带淡底
pub fn where_chip(ui: &mut egui::Ui, active: bool, open: bool) -> egui::Response {
    let p = crate::theme::c();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(76.0, 24.0), egui::Sense::click());
    if open || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(radius_ctrl() as u8),
            if open {
                crate::theme::tint(crate::theme::where_color(), 34)
            } else {
                p.hover
            },
        );
    }
    let cy = rect.center().y;
    let col = if active {
        crate::theme::where_color()
    } else {
        p.text2
    };
    crate::icons::paint_icon(
        ui.painter(),
        &crate::lucide::FILTER,
        egui::pos2(rect.left() + 13.0, cy),
        13.0,
        col,
    );
    let g = ui.painter().layout_no_wrap(
        "WHERE".to_owned(),
        crate::theme::mono(12.0),
        crate::theme::where_color(),
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 25.0, cy - g.size().y / 2.0),
        g,
        crate::theme::where_color(),
    );
    resp
}
/// 状态栏竖直分隔符
pub fn vsep(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 20.0), Sense::hover());
    let p = crate::theme::c();
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.center().y - 6.0),
            egui::pos2(rect.center().x, rect.center().y + 6.0),
        ],
        egui::Stroke::new(1.0, p.border),
    );
}
pub fn header_bottom_line(ui: &egui::Ui) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, crate::theme::c().border),
    );
}
/// 数据格右侧竖线(DBX:列间有分隔线,行间只靠斑马纹)
pub fn col_right_line(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [
            egui::pos2(r.right() - 0.5, r.top()),
            egui::pos2(r.right() - 0.5, r.bottom()),
        ],
        egui::Stroke::new(1.0, crate::theme::c().row_line),
    );
}

// ---------------------------------------------------------------- 列头几何与部件

/// 列头左右内边距
pub const HEAD_PAD_X: f32 = 8.0;
/// 列头顶部内边距
pub const HEAD_PAD_Y: f32 = 6.0;
/// 第一行(名称 + 控件)行高
pub const HEAD_LINE1: f32 = 16.0;
/// 第二/三行(类型/注释)行高
pub const HEAD_LINE2: f32 = 12.0;

/// 主键钥匙小图标(圆环 + 钥匙柄),约 9×5
pub fn paint_key(p: &egui::Painter, center: egui::Pos2, color: Color32) {
    let st = egui::Stroke::new(1.2, color);
    p.circle_stroke(egui::pos2(center.x - 2.6, center.y), 2.3, st);
    p.line_segment(
        [
            egui::pos2(center.x - 0.4, center.y),
            egui::pos2(center.x + 4.6, center.y),
        ],
        st,
    );
    p.line_segment(
        [
            egui::pos2(center.x + 3.2, center.y),
            egui::pos2(center.x + 3.2, center.y + 2.0),
        ],
        st,
    );
}
/// 行号列头(表数据页与结果网格共用):"#" 对齐列名的名称行,
/// 右缘一道分隔线 + 底线
pub fn paint_rowno_head(painter: &egui::Painter, rect: egui::Rect) {
    let p = crate::theme::c();
    painter.rect_filled(rect, 0, p.header);
    painter.text(
        egui::pos2(rect.right() - 14.0, rect.top() + HEAD_PAD_Y + HEAD_LINE1 / 2.0),
        egui::Align2::RIGHT_CENTER,
        "#",
        egui::FontId::proportional(fs_px(11.0)),
        p.muted,
    );
    let st = egui::Stroke::new(1.0, p.border);
    painter.line_segment(
        [egui::pos2(rect.right() - 0.5, rect.top()), egui::pos2(rect.right() - 0.5, rect.bottom())],
        st,
    );
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], st);
}
/// 结果网格的列头(不能排序/筛选):只有 名称 + 类型 两行
pub fn paint_result_col_head(painter: &egui::Painter, rect: egui::Rect, name: &str, ty: &str) {
    paint_col_title(painter, rect, name, 0.0);
    let ty = (!ty.is_empty()).then_some(ty);
    paint_col_anno(painter, rect, ty, None, false);
}
/// 第一行:列名(13px 粗体);`reserve_right` 是名称右侧让给控件的宽度
pub fn paint_col_title(painter: &egui::Painter, rect: egui::Rect, name: &str, reserve_right: f32) {
    let p = crate::theme::c();
    let cy1 = rect.top() + HEAD_PAD_Y + HEAD_LINE1 / 2.0;
    let name_w = (rect.width() - HEAD_PAD_X * 2.0 - reserve_right).max(24.0);
    let g = crate::icons::text_galley(
        painter,
        name.to_owned(),
        crate::theme::bold(crate::theme::fs_header()),
        p.text,
        name_w,
    );
    painter.galley(
        egui::pos2(rect.left() + HEAD_PAD_X, cy1 - g.size().y / 2.0),
        g,
        p.text,
    );
}
/// 第二行:主键钥匙 + 类型小字(按类别着色);第三行:列注释(灰字,有才显示)
pub fn paint_col_anno(
    painter: &egui::Painter,
    rect: egui::Rect,
    ty: Option<&str>,
    comment: Option<&str>,
    pk: bool,
) {
    let p = crate::theme::c();
    let y2 = rect.top() + HEAD_PAD_Y + HEAD_LINE1;
    let mut x = rect.left() + HEAD_PAD_X;
    if pk {
        paint_key(
            painter,
            egui::pos2(x + 5.0, y2 + HEAD_LINE2 / 2.0),
            crate::theme::pk_key_color(),
        );
        x += 12.0;
    }
    // 只剩一个省略号的位置(列被压到最窄)就不画:孤零零的"…"没有信息
    if let Some(ty) = ty.filter(|_| rect.right() - HEAD_PAD_X - x >= 20.0) {
        let color = crate::theme::type_color(crate::theme::classify_type(ty));
        let avail = (rect.right() - HEAD_PAD_X - x).max(8.0);
        let tg = crate::icons::text_galley(
            painter,
            ty.to_owned(),
            egui::FontId::proportional(crate::theme::fs_type()),
            color,
            avail,
        );
        painter.galley(
            egui::pos2(x, y2 + (HEAD_LINE2 - tg.size().y) / 2.0),
            tg,
            color,
        );
    }
    if let Some(cm) = comment {
        let cg = crate::icons::text_galley(
            painter,
            cm.to_owned(),
            egui::FontId::proportional(crate::theme::fs_type()),
            p.muted,
            (rect.width() - HEAD_PAD_X * 2.0).max(8.0),
        );
        painter.galley(
            egui::pos2(
                rect.left() + HEAD_PAD_X,
                y2 + HEAD_LINE2 + (HEAD_LINE2 - cg.size().y) / 2.0,
            ),
            cg,
            p.muted,
        );
    }
}

// ---------------------------------------------------------------- 徽章

/// 画状态徽章(语义色 16% 底 + 语义色文字),原型 status 列样式
pub fn paint_badge(p: &egui::Painter, x: f32, cy: f32, label: &str, on: bool) {
    let c = crate::theme::c();
    let (fg, bg) = if on {
        (c.green, crate::theme::tint(c.green, 38))
    } else {
        (c.red, crate::theme::tint(c.red, 38))
    };
    let font = crate::theme::bold(9.5);
    let g = p.layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let w = (g.size().x + 9.0).max(16.0);
    let rect = egui::Rect::from_min_size(egui::pos2(x, cy - 8.0), egui::vec2(w, 16.0));
    p.rect_filled(rect, egui::CornerRadius::same(radius_small() as u8), bg);
    p.galley(
        egui::pos2(
            rect.center().x - g.size().x / 2.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        fg,
    );
}
/// 短状态值 → 徽章(仅 status/state/enabled/active/deleted 类列)
pub fn badge_value(col: &str, v: &Option<String>) -> Option<(String, bool)> {
    let val = v.as_deref()?.trim();
    if val.is_empty() || val.chars().count() > 6 {
        return None;
    }
    let name = col.to_ascii_lowercase();
    let is_state = [
        "status",
        "state",
        "enabled",
        "is_enabled",
        "active",
        "is_active",
        "deleted",
        "is_deleted",
        "disabled",
        "is_disabled",
    ]
    .iter()
    .any(|k| name == *k || name.ends_with(&format!("_{k}")));
    if !is_state {
        return None;
    }
    let on = matches!(
        val.to_ascii_lowercase().as_str(),
        "1" | "true"
            | "yes"
            | "y"
            | "on"
            | "active"
            | "enabled"
            | "normal"
            | "启用"
            | "正常"
            | "成功"
    );
    let off = matches!(
        val.to_ascii_lowercase().as_str(),
        "0" | "2"
            | "false"
            | "no"
            | "n"
            | "off"
            | "inactive"
            | "disabled"
            | "deleted"
            | "禁用"
            | "删除"
            | "失败"
    );
    // deleted/disabled 类列是反语义:1=true 表示"已删除/已禁用",应显示红色
    let invert = ["deleted", "is_deleted", "disabled", "is_disabled"]
        .iter()
        .any(|k| name == *k || name.ends_with(&format!("_{k}")));
    if on {
        Some((val.to_string(), !invert))
    } else if off {
        Some((val.to_string(), invert))
    } else {
        None
    }
}

// ---------------------------------------------------------------- 列宽启发式

pub fn text_width(s: &str) -> f32 {
    s.chars()
        .take(160)
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                6.8
            } else {
                13.0
            }
        })
        .sum()
}
/// 启发式列宽:表头与数据采样(前 80 行最长值);ASCII 6.8pt / CJK 13pt
/// (13px 比例字体的实测均值)。查询结果网格的单行列头用:名称 + 26 内边距
pub fn suggested_widths(rows: &[Vec<Option<String>>], columns: &[String]) -> Vec<f32> {
    let sample = rows.iter().take(80);
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let head = text_width(c);
            let data = sample
                .clone()
                .filter_map(|r| r.get(i).and_then(|v| v.as_deref()))
                .map(text_width)
                .fold(0.0_f32, f32::max);
            (head.max(data) + 26.0).clamp(72.0, 720.0)
        })
        .collect()
}
/// 免投影版本:直接在全量行上按可见列下标采样(避免每帧深拷贝整页数据)。
/// 表数据页的两行列头右侧要让出排序箭头 + 漏斗(36pt),名称另加 20pt 余量
pub fn suggested_widths_vis(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    vis_idx: &[usize],
) -> Vec<f32> {
    let sample = rows.iter().take(80);
    vis_idx
        .iter()
        .map(|&i| {
            let c = &columns[i];
            let head = text_width(c) + 30.0;
            let data = sample
                .clone()
                .filter_map(|r| r.get(i).and_then(|v| v.as_deref()))
                .map(text_width)
                .fold(0.0_f32, f32::max);
            (head.max(data) + 26.0).clamp(84.0, 720.0)
        })
        .collect()
}
/// 已有基础列宽 → 按可用宽度摊开(多余空间按比例分给各列,单列封顶 720)
pub fn fitted_from_base(base: Vec<f32>, available: f32) -> Vec<f32> {
    let sum: f32 = base.iter().sum();
    if sum <= 0.0 {
        return base;
    }
    let total = 48.0 + sum;
    if available > total + 1.0 {
        let scale = (available - 48.0) / sum;
        base.iter().map(|w| (w * scale).min(720.0)).collect()
    } else {
        base
    }
}

// ---------------------------------------------------------------- 结果网格绘制族

pub const ROWNO_W: f32 = 48.0;

/// 第 `i` 列的类型名;越界或驱动没给出时为空串
pub fn type_at(types: &[String], i: usize) -> &str {
    types.get(i).map(String::as_str).unwrap_or("")
}
/// 右缘竖线(画在矩形内侧,不被相邻格的底色盖掉)
pub fn vline(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    painter.line_segment(
        [
            egui::pos2(rect.right() - 0.5, rect.top()),
            egui::pos2(rect.right() - 0.5, rect.bottom()),
        ],
        egui::Stroke::new(1.0, color),
    );
}
/// 底缘横线
pub fn hline(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - 0.5),
            egui::pos2(rect.right(), rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, color),
    );
}
/// 列头格:底色 + 名称/类型两行 + 右竖线 + 底线
pub fn paint_grid_head(painter: &egui::Painter, rect: egui::Rect, name: &str, ty: &str) {
    let p = crate::theme::c();
    let cp = painter.with_clip_rect(rect);
    cp.rect_filled(rect, 0, p.header);
    paint_result_col_head(&cp, rect, name, ty);
    vline(&cp, rect, p.row_line);
    hline(&cp, rect, p.border);
}
/// 行底色:悬停 > 奇偶斑马纹;有未保存改动的格再叠一块内缩色块
pub fn paint_grid_row_bg(
    painter: &egui::Painter,
    rect: egui::Rect,
    ri: usize,
    hovered: bool,
    changed: bool,
) {
    let p = crate::theme::c();
    if hovered {
        painter.rect_filled(rect, 0, p.row_hover);
    } else if ri % 2 == 1 {
        painter.rect_filled(rect, 0, p.stripe);
    }
    if changed {
        painter.rect_filled(rect.shrink2(egui::vec2(2.0, 3.0)), 3, p.change_bg);
    }
}
pub fn paint_grid_cell_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    col: &str,
    shown: &Option<String>,
    changed: bool,
    numeric: bool,
) {
    let p = crate::theme::c();
    if let Some((label, on)) = badge_value(col, shown) {
        paint_badge(painter, rect.left() + 12.0, rect.center().y, &label, on);
    } else {
        let color = crate::icons::cell_text_color(shown, changed);
        let g = crate::icons::text_galley(
            painter,
            crate::icons::cell_text(shown, 4000),
            egui::FontId::proportional(crate::theme::fs_data()),
            color,
            rect.width() - 24.0,
        );
        let x = if numeric && shown.is_some() {
            rect.right() - 12.0 - g.size().x
        } else {
            rect.left() + 12.0
        };
        painter.galley(
            egui::pos2(x, rect.center().y - g.size().y / 2.0),
            g,
            color,
        );
    }
    vline(painter, rect, p.row_line);
}
/// 行号格:行号右对齐 + 右竖线
pub fn paint_grid_rowno(painter: &egui::Painter, rect: egui::Rect, rowno: usize) {
    painter.text(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{rowno}"),
        egui::FontId::proportional(fs_px(11.0)),
        crate::theme::c().muted,
    );
    vline(painter, rect, crate::theme::c().row_line);
}
/// 各列是否右对齐:有类型按类型类别(整数/小数);类型拿不到的列退回按值预判
/// (前 200 行的非空值全是数字)。按列数生成——类型数组可以整个为空
/// (PG 的简单查询协议、执行计划、Parquet 预览都没有类型)
pub fn numeric_flags(columns: &[String], column_types: &[String], rows: &[Vec<Option<String>>]) -> Vec<bool> {
    columns
        .iter()
        .enumerate()
        .map(|(i, _)| match (!type_at(column_types, i).is_empty()).then_some(type_at(column_types, i)) {
            Some(ty) => matches!(
                crate::theme::classify_type(ty),
                crate::theme::TypeKind::Integer | crate::theme::TypeKind::Numeric
            ),
            None => {
                let mut seen = false;
                let all_numeric = rows.iter().take(200).all(|r| match r.get(i).and_then(|v| v.as_deref()) {
                    Some(v) => {
                        seen = true;
                        looks_numeric(v)
                    }
                    None => true,
                });
                seen && all_numeric
            }
        })
        .collect()
}
/// 值看起来是不是数字。前导零(邮编/编号)与超过 15 位的数字串(卡号/条码/身份证)
/// 不算——这些多半是存在文本列里的标识,而不是数量
pub fn looks_numeric(v: &str) -> bool {
    if v.is_empty() || v.len() > 15 || !v.bytes().all(|b| b.is_ascii_digit() || b".-+eE".contains(&b)) {
        return false;
    }
    let body = v.strip_prefix(['-', '+']).unwrap_or(v);
    let leading_zero = body.starts_with('0') && !body.starts_with("0.") && body.len() > 1;
    !leading_zero && v.parse::<f64>().is_ok()
}
/// 结果网格列宽:同表数据页的启发式(列名/数据采样),并保证放得下类型小字。
/// 类型数组可以比列少甚至为空(没有类型的结果集只按名称与数据定宽)
pub fn fitted_widths_typed(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    column_types: &[String],
    available: f32,
) -> Vec<f32> {
    fitted_from_base(base_widths_typed(rows, columns, column_types), available)
}

/// 结果网格各列的内容宽度(列名/数据采样 + 类型小字放得下),不摊派富余空间。
/// "按内容自适应"用这个;`fitted_widths_typed` 是它再按可用宽度摊开的结果
pub fn base_widths_typed(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    column_types: &[String],
) -> Vec<f32> {
    (0..columns.len())
        .map(|i| base_width_at(rows, columns, column_types, i))
        .collect()
}

/// 结果网格第 `i` 列的内容宽度(同 `base_widths_typed` 的第 i 项);自适应单列时只算这一列
pub fn base_width_at(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    column_types: &[String],
    i: usize,
) -> f32 {
    let head = columns.get(i).map_or(0.0, |c| text_width(c));
    let data = rows
        .iter()
        .take(80)
        .filter_map(|r| r.get(i).and_then(|v| v.as_deref()))
        .map(text_width)
        .fold(0.0_f32, f32::max);
    // 类型小字 10px:ASCII 约 5.4pt/字,CJK 约 10pt/字
    let tw: f32 = type_at(column_types, i)
        .chars()
        .take(40)
        .map(|c| if c.is_ascii() { 5.4 } else { 10.0 })
        .sum();
    (head.max(data) + 26.0).clamp(72.0, 720.0).max(84.0).max(tw + 16.0)
}

// ---------------------------------------------------------------- 工具栏按钮

// ---------------------------------------------------------------- 工具栏按钮族

/// 工具栏按钮统一高度
pub const TOOL_H: f32 = 28.0;

fn tool_text(ui: &egui::Ui, label: &str) -> Option<std::sync::Arc<egui::Galley>> {
    if label.is_empty() {
        None
    } else {
        Some(
            ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(fs_px(12.0)),
                crate::theme::c().text,
            ),
        )
    }
}

/// 工具栏按钮:图标 + 文字(label 为空 = 只留图标,靠悬停提示说明)。
/// 全应用的工具栏(顶部、各页面、表头行)都用这一种,别处不再各写各的
pub fn tool_button(ui: &mut egui::Ui, icon: Option<&'static Icon>, label: &str, tip: &str) -> bool {
    tool_button_on(ui, icon, label, tip, true)
}

/// 同 `tool_button`,带禁用态(置灰、不响应点击,悬停说明保留——
/// 配 `on_disabled_hover_text` 说明为什么不可用)
pub fn tool_button_on(
    ui: &mut egui::Ui,
    icon: Option<&'static Icon>,
    label: &str,
    tip: &str,
    enabled: bool,
) -> bool {
    let p = crate::theme::c();
    let g = tool_text(ui, label);
    let w = if label.is_empty() {
        TOOL_H
    } else {
        8.0 + if icon.is_some() { ICON_SIZE + 5.0 } else { 0.0 } + g.as_ref().map_or(0.0, |g| g.size().x) + 9.0
    };
    let (rect, resp, hot) = tool_face(ui, w);
    let col = if !enabled {
        p.muted
    } else if hot {
        p.text
    } else {
        p.text2
    };
    let cy = rect.center().y;
    let mut x = rect.left() + 8.0;
    if let Some(ic) = icon {
        crate::icons::paint_icon(ui.painter(), ic, egui::pos2(x + ICON_SIZE / 2.0, cy), ICON_SIZE, col);
        x += ICON_SIZE + 5.0;
    }
    if let Some(g) = g {
        ui.painter().galley(egui::pos2(x, cy - g.size().y / 2.0), g, col);
    }
    // 文字是画出来的:补无障碍标签,读屏与 kittest 才找得到按钮
    let name = if label.is_empty() { tip } else { label };
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && ui.is_enabled(), name.to_owned())
    });
    if !tip.is_empty() {
        resp.clone().on_hover_text(tip);
    }
    enabled && resp.clicked()
}

/// 工具栏菜单按钮:图标 + 文字 + 下箭头,点开在正下方弹菜单(与 `tool_button` 同一张脸)。
/// 菜单项由调用方的闭包给出;点中某项后由调用方 `ui.close_menu()` 关闭
pub fn tool_menu<R>(
    ui: &mut egui::Ui,
    icon: Option<&'static Icon>,
    label: &str,
    tip: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let p = crate::theme::c();
    let popup_id = ui.id().with(("tool-menu", label, tip));
    let opened = ui.memory(|m| m.is_popup_open(popup_id));
    let g = tool_text(ui, label);
    // 箭头紧贴内容(图标-only 时按钮才不会中间空一截,看着像两个按钮)
    let w = 8.0
        + if icon.is_some() { ICON_SIZE + 5.0 } else { 0.0 }
        + g.as_ref().map_or(0.0, |g| g.size().x)
        + 2.0
        + 13.0
        + 5.0;
    let (rect, resp, hot) = tool_face(ui, w);
    let col = if hot || opened { p.text } else { p.text2 };
    let cy = rect.center().y;
    let mut x = rect.left() + 8.0;
    if let Some(ic) = icon {
        crate::icons::paint_icon(ui.painter(), ic, egui::pos2(x + ICON_SIZE / 2.0, cy), ICON_SIZE, col);
        x += ICON_SIZE + 5.0;
    }
    if let Some(g) = g {
        ui.painter().galley(egui::pos2(x, cy - g.size().y / 2.0), g, col);
    }
    if !label.is_empty() {
        crate::icons::paint_icon(
            ui.painter(),
            &crate::lucide::CHEVRON_DOWN,
            egui::pos2(rect.right() - 5.0 - 6.5, cy),
            13.0,
            p.muted,
        );
    }
    let name = if label.is_empty() { tip } else { label };
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.to_owned())
    });
    if !tip.is_empty() {
        resp.clone().on_hover_text(tip);
    }
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    if opened {
        let _ = egui::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::PopupCloseBehavior::CloseOnClick,
            add,
        );
    }
    resp
}

// ---------------------------------------------------------------- 设计器小件

pub fn section_label(ui: &mut egui::Ui, label: &str) {
    let p = crate::theme::c();
    ui.label(crate::theme::strong(label, 12.0).color(p.text2));
}
/// 小号单行输入(设计器网格用)
pub fn tiny_field(ui: &mut egui::Ui, val: &mut String, ph: &str, enabled: bool, color: Color32) {
    let p = crate::theme::c();
    ui.add_sized(
        [ui.available_width(), 24.0],
        egui::TextEdit::singleline(val)
            .hint_text(
                RichText::new(ph)
                    .font(crate::theme::mono(10.5))
                    .color(p.muted.gamma_multiply(0.7)),
            )
            .font(crate::theme::mono(11.5))
            .text_color(color)
            .interactive(enabled),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内容宽不随可用宽度变:摊派函数会把富余空间分给各列(顶到 720),
    /// "按内容自适应"要的是没摊派的那份;单列取的必须是该列自己的数据
    #[test]
    fn base_widths_are_content_only() {
        let cols = ["id", "note"].map(String::from);
        let rows = vec![vec![
            Some("1".to_string()),
            Some("a fairly long piece of text".to_string()),
        ]];
        let types = vec!["int4".to_string(), "text".to_string()];
        let base = base_widths_typed(&rows, &cols, &types);
        assert!(
            base[1] > base[0] + 100.0,
            "note 列内容更长应更宽: {base:?}"
        );
        assert_eq!(
            fitted_widths_typed(&rows, &cols, &types, 10_000.0),
            vec![720.0; 2],
            "摊派函数在超大可用宽度下会顶到上限"
        );
        for (i, w) in base.iter().enumerate() {
            assert_eq!(base_width_at(&rows, &cols, &types, i), *w);
        }
        // 类型数组可以缺
        assert_eq!(
            base_width_at(&rows, &cols, &[], 1),
            (text_width("a fairly long piece of text") + 26.0).max(84.0)
        );
    }
}
