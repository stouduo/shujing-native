//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化/横向滚动)。
//! 查询结果、执行计划、Parquet 预览、Kafka 消息共用;视觉与表数据页同一套:
//! 两行列头(名称 + 类型小字,52pt)、30pt 行高、13px 比例字体、数字右对齐、
//! 列间分隔线、行间靠斑马纹。

use crate::table_view::{badge_value, paint_badge};
use crate::{icons, theme};
use egui::{Color32, Sense};
use egui_extras::{Column, TableBuilder};

/// 读上一帧横向偏移(按网格 id 存 egui 临时态;静态会被同帧渲染的优化计划网格互相覆盖)
fn load_prev_hscroll(ctx: &egui::Context, id_salt: &str) -> f32 {
    ctx.data_mut(|d| {
        d.get_temp::<f32>(egui::Id::new(("grid-hscroll", id_salt)))
            .unwrap_or(0.0)
    })
}

fn store_hscroll(ctx: &egui::Context, id_salt: &str, x: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("grid-hscroll", id_salt)), x));
}

/// 冻结覆盖层需要的渲染几何(每帧由网格填充)
#[derive(Default)]
struct GridGeom {
    header_top: Option<f32>,
    /// 本帧实际渲染的 (行号, 行带顶部真实屏幕 y)。
    /// 行虚拟化首行可能带裁剪(滚动偏移非行高整数倍),y 必须逐行记录不可推算
    vis_rows: Vec<(usize, f32)>,
}

/// 可编辑网格的编辑态(冻结覆盖层与底层共用;只读网格传 None)
struct EditFields<'a> {
    changes: &'a [(usize, String, Option<String>)],
    editing: &'a mut Option<(usize, usize)>,
    draft: &'a mut String,
    start_edit: &'a mut Option<(usize, usize, String)>,
    commit: &'a mut Option<(usize, usize, String)>,
}

/// 渲染只读结果集(带行号列)。`column_types` 与 `columns` 等长,拿不到类型的列传空串。
#[allow(clippy::too_many_arguments)]
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
) {
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let col_widths = fitted_widths_typed(rows, columns, column_types, ui.available_width());
    let numeric = numeric_flags(columns, column_types, rows);
    let desired_w = ROWNO_W + col_widths.iter().sum::<f32>();
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    let outer = ui.available_rect_before_wrap();
    let mut geom = GridGeom::default();
    let actual_widths = std::cell::Cell::new(Vec::<f32>::new());
    let prev_hscroll = load_prev_hscroll(ui.ctx(), id_salt);
    // 结果网格和表数据页共用同一套横向滚动条策略:常驻、实体、高可见度
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.9;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.15;
    ui.spacing_mut().scroll.interact_handle_opacity = 1.0;
    ui.spacing_mut().scroll.active_handle_opacity = 1.0;
    ui.spacing_mut().scroll.bar_width = 12.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
    let hout = egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));
            // 拖拽平移:按住结果网格表体左右拖 = 横向滚动(只读网格无编辑态);
            // 起点须在表头下方内容区,拖列宽手柄/滚动条不连带平移
            {
                let content = ui.max_rect();
                let (drag_pan, started) = ui.input(|i| {
                    (
                        i.pointer.button_down(egui::PointerButton::Primary)
                            && i.pointer.is_decidedly_dragging(),
                        i.pointer.press_origin(),
                    )
                });
                let in_body: bool = started
                    .map(|p| {
                        content.contains(p)
                            && p.y > content.top() + HEADER_H
                            && p.x < content.right() - 16.0
                    })
                    .unwrap_or(false);
                if drag_pan && in_body {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    if dx.abs() > 0.1 {
                        // 瞬时平移:默认走 style 动画会跟手发飘,且松手后内容仍在滑,
                        // 滑动尾巴里的点击会被 egui 静默取消
                        ui.scroll_with_delta_animation(
                            egui::vec2(-dx, 0.0),
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                }
            }
            // body 闭包内外层 ui 被 builder 借用,先 clone painter 供空行延伸
            let body_painter = ui.painter().clone();
            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(false)
                .sense(Sense::hover())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(ROWNO_W));
            // initial(而非 exact):exact 把 width_range 钉死在 w..=w,拖拽调宽无效
            for (i, _c) in columns.iter().enumerate() {
                tb = tb
                    .column(Column::initial(col_widths[i]).resizable(true).clip(true).at_least(40.0));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
                    if geom.header_top.is_none() {
                        geom.header_top = Some(rect.top());
                    }
                    crate::table_view::paint_rowno_head(ui.painter(), rect);
                });
                for (vw, c) in columns.iter().enumerate() {
                    header.col(|ui| {
                        let rect = ui.max_rect();
                        paint_grid_head(ui.painter(), rect, c, type_at(column_types, vw));
                        let resp = ui.allocate_rect(rect, Sense::click());
                        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
                    });
                }
                header.col(|ui| {
                    // 末尾占位列:延续表头底色与底线
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, p.header);
                    hline(ui.painter(), rect, p.border);
                });
            })
            .body(|body| {
                // rows() 消费 body:列宽、数据区矩形都先取出来
                let ws = body.widths().to_vec();
                actual_widths.set(ws.clone());
                let brect = body.max_rect();
                // 行悬停:每格用自身矩形判定(跨行累积并工会点亮悬停行下方全部行)
                let hovers = |ui: &egui::Ui, rect: egui::Rect| {
                    ui.input(|i| i.pointer.latest_pos())
                        .map(|pt| {
                            pt.y >= rect.top()
                                && pt.y < rect.bottom()
                                && pt.x >= table_left
                                && pt.x <= table_right
                        })
                        .unwrap_or(false)
                };
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let row_y = std::cell::Cell::new(f32::NAN);
                    let rowdata = &rows[ri];
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        row_y.set(rect.top());
                        paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                        paint_grid_rowno(ui.painter(), rect, ri + 1);
                    });
                    for (ci, col) in columns.iter().enumerate() {
                        row.col(|ui| {
                            let rect = ui.max_rect();
                            let painter = ui.painter().with_clip_rect(rect);
                            paint_grid_row_bg(&painter, rect, ri, hovers(ui, rect), false);
                            paint_grid_cell_text(
                                &painter,
                                rect,
                                col,
                                &rowdata[ci],
                                false,
                                numeric[ci],
                            );
                        });
                    }
                    // 末尾占位列:延续行底色
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                    });
                    geom.vis_rows.push((ri, row_y.get()));
                });

                // 空白区网格延续(与数据表格页同款):斑马纹 + 列分隔线,竖线位置与数据列右缘对齐
                {
                    let mut y = brect.top() + rows.len() as f32 * theme::ROW_H;
                    let mut empty_ri = rows.len();
                    while y + theme::ROW_H <= brect.bottom() + 0.5 {
                        let band = egui::Rect::from_min_max(
                            egui::pos2(brect.left(), y),
                            egui::pos2(brect.right(), y + theme::ROW_H),
                        );
                        if empty_ri % 2 == 1 {
                            body_painter.rect_filled(band, 0, p.stripe);
                        }
                        let mut ex = band.left();
                        // 末位是 remainder 占位列,没有右缘
                        for w in ws.iter().take(ws.len().saturating_sub(1)) {
                            ex += *w;
                            if ex >= band.right() - 1.0 {
                                break;
                            }
                            body_painter.line_segment(
                                [
                                    egui::pos2(ex - 0.5, band.top()),
                                    egui::pos2(ex - 0.5, band.bottom()),
                                ],
                                egui::Stroke::new(1.0, p.row_line),
                            );
                        }
                        y += theme::ROW_H;
                        empty_ri += 1;
                    }
                }
            });
        });
    store_hscroll(ui.ctx(), id_salt, hout.state.offset.x);
    draw_frozen_grid_overlay(
        ui,
        id_salt,
        columns,
        column_types,
        &numeric,
        rows,
        frozen,
        hout.state.offset.x,
        prev_hscroll,
        &geom,
        &actual_widths,
        outer,
        None,
        &std::cell::Cell::new(false),
    );
    ui.spacing_mut().item_spacing = old_spacing;
}

// ---------------------------------------------------------------- 绘制(三处网格同一套)

/// 表头高度:与表数据页同一套两行列头(名称 + 类型)
const HEADER_H: f32 = theme::DATA_HEADER_H;
/// 行号列宽
const ROWNO_W: f32 = 48.0;

/// 第 `i` 列的类型名;越界或驱动没给出时为空串
fn type_at(types: &[String], i: usize) -> &str {
    types.get(i).map(String::as_str).unwrap_or("")
}

/// 右缘竖线(画在矩形内侧,不被相邻格的底色盖掉)
fn vline(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    painter.line_segment(
        [
            egui::pos2(rect.right() - 0.5, rect.top()),
            egui::pos2(rect.right() - 0.5, rect.bottom()),
        ],
        egui::Stroke::new(1.0, color),
    );
}

/// 底缘横线
fn hline(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - 0.5),
            egui::pos2(rect.right(), rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, color),
    );
}

/// 列头格:底色 + 名称/类型两行 + 右竖线 + 底线
fn paint_grid_head(painter: &egui::Painter, rect: egui::Rect, name: &str, ty: &str) {
    let p = theme::c();
    let cp = painter.with_clip_rect(rect);
    cp.rect_filled(rect, 0, p.header);
    crate::table_view::paint_result_col_head(&cp, rect, name, ty);
    vline(&cp, rect, p.row_line);
    hline(&cp, rect, p.border);
}

/// 行底色:悬停 > 奇偶斑马纹;有未保存改动的格再叠一块内缩色块
fn paint_grid_row_bg(
    painter: &egui::Painter,
    rect: egui::Rect,
    ri: usize,
    hovered: bool,
    changed: bool,
) {
    let p = theme::c();
    if hovered {
        painter.rect_filled(rect, 0, p.row_hover);
    } else if ri % 2 == 1 {
        painter.rect_filled(rect, 0, p.stripe);
    }
    if changed {
        painter.rect_filled(rect.shrink2(egui::vec2(2.0, 3.0)), 3, p.change_bg);
    }
}

/// 数据格:状态徽章列画徽章;数字列右对齐(NULL 靠左);右竖线
#[allow(clippy::too_many_arguments)]
fn paint_grid_cell_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    col: &str,
    shown: &Option<String>,
    changed: bool,
    numeric: bool,
) {
    let p = theme::c();
    if let Some((label, on)) = badge_value(col, shown) {
        paint_badge(painter, rect.left() + 12.0, rect.center().y, &label, on);
    } else {
        let color = icons::cell_text_color(shown, changed);
        let g = icons::text_galley(
            painter,
            icons::cell_text(shown, 4000),
            egui::FontId::proportional(theme::FS_DATA),
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
fn paint_grid_rowno(painter: &egui::Painter, rect: egui::Rect, rowno: usize) {
    painter.text(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{rowno}"),
        egui::FontId::proportional(11.0),
        theme::c().muted,
    );
    vline(painter, rect, theme::c().row_line);
}

/// 各列是否右对齐:有类型按类型类别(整数/小数);类型拿不到的列退回按值预判
/// (前 200 行的非空值全是数字)。按列数生成——类型数组可以整个为空
/// (PG 的简单查询协议、执行计划、Parquet 预览都没有类型)
fn numeric_flags(columns: &[String], column_types: &[String], rows: &[Vec<Option<String>>]) -> Vec<bool> {
    columns
        .iter()
        .enumerate()
        .map(|(i, _)| match (!type_at(column_types, i).is_empty()).then_some(type_at(column_types, i)) {
            Some(ty) => matches!(
                theme::classify_type(ty),
                theme::TypeKind::Integer | theme::TypeKind::Numeric
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
fn looks_numeric(v: &str) -> bool {
    if v.is_empty() || v.len() > 15 || !v.bytes().all(|b| b.is_ascii_digit() || b".-+eE".contains(&b)) {
        return false;
    }
    let body = v.strip_prefix(['-', '+']).unwrap_or(v);
    let leading_zero = body.starts_with('0') && !body.starts_with("0.") && body.len() > 1;
    !leading_zero && v.parse::<f64>().is_ok()
}

/// 结果网格列宽:同表数据页的启发式(列名/数据采样),并保证放得下类型小字。
/// 类型数组可以比列少甚至为空(没有类型的结果集只按名称与数据定宽)
fn fitted_widths_typed(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    column_types: &[String],
    available: f32,
) -> Vec<f32> {
    let mut base = crate::table_view::suggested_widths(rows, columns);
    for (i, w) in base.iter_mut().enumerate() {
        // 类型小字 10px:ASCII 约 5.4pt/字,CJK 约 10pt/字
        let tw: f32 = type_at(column_types, i)
            .chars()
            .take(40)
            .map(|c| if c.is_ascii() { 5.4 } else { 10.0 })
            .sum();
        *w = w.max(84.0).max(tw + 16.0);
    }
    crate::table_view::fitted_from_base(base, available)
}

/// 可编辑结果集(查询回写):双击进入编辑,改动以 (行号,列名,新值) 记在 changes。
/// 返回 (请求开始编辑的格, 提交的格);开始编辑由调用方置 draft,提交由调用方记 changes。
#[allow(clippy::too_many_arguments)]
pub fn result_rows_editable(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<Option<String>>],
    changes: &[(usize, String, Option<String>)],
    editing: &mut Option<(usize, usize)>,
    draft: &mut String,
    frozen: &mut usize,
) -> (
    Option<(usize, usize, String)>,
    Option<(usize, usize, String)>,
) {
    let mut start_edit: Option<(usize, usize, String)> = None;
    let mut commit: Option<(usize, usize, String)> = None;
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let col_widths = fitted_widths_typed(rows, columns, column_types, ui.available_width());
    let numeric = numeric_flags(columns, column_types, rows);
    let desired_w = ROWNO_W + col_widths.iter().sum::<f32>();
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.command);
    let editing_active = editing.is_some();
    // 编辑行滚出视口的帧末取消(与表数据页同款):行虚拟化下编辑格不渲染,
    // Esc/Enter 处理执行不到,q_editing 悬死 → 焦点门全局失效且 Esc 救不回
    let editing_at_start = editing.is_some();
    let editing_rendered = std::cell::Cell::new(false);
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    let outer = ui.available_rect_before_wrap();
    let frozen_n = (*frozen).min(columns.len());
    let mut geom = GridGeom::default();
    let actual_widths = std::cell::Cell::new(Vec::<f32>::new());
    let prev_hscroll = load_prev_hscroll(ui.ctx(), id_salt);
    // 结果网格和表数据页共用同一套横向滚动条策略:常驻、实体、高可见度
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.9;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.15;
    ui.spacing_mut().scroll.interact_handle_opacity = 1.0;
    ui.spacing_mut().scroll.active_handle_opacity = 1.0;
    ui.spacing_mut().scroll.bar_width = 12.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
    let hout = egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));
            // 拖拽平移(编辑态除外;起点须在表头下方内容区)
            if !editing_active {
                let content = ui.max_rect();
                let (drag_pan, started) = ui.input(|i| {
                    (
                        i.pointer.button_down(egui::PointerButton::Primary)
                            && i.pointer.is_decidedly_dragging(),
                        i.pointer.press_origin(),
                    )
                });
                let in_body: bool = started
                    .map(|p| {
                        content.contains(p)
                            && p.y > content.top() + HEADER_H
                            && p.x < content.right() - 16.0
                    })
                    .unwrap_or(false);
                if drag_pan && in_body {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    if dx.abs() > 0.1 {
                        // 瞬时平移:默认走 style 动画会跟手发飘,且松手后内容仍在滑,
                        // 滑动尾巴里的点击会被 egui 静默取消
                        ui.scroll_with_delta_animation(
                            egui::vec2(-dx, 0.0),
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                }
            }
            // body 闭包内外层 ui 被 builder 借用,先 clone painter 供空行延伸
            let body_painter = ui.painter().clone();
            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(false)
                .sense(Sense::hover())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(ROWNO_W));
            // initial(而非 exact):exact 把 width_range 钉死在 w..=w,拖拽调宽无效
            for (i, _c) in columns.iter().enumerate() {
                tb = tb
                    .column(Column::initial(col_widths[i]).resizable(true).clip(true).at_least(40.0));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
                    if geom.header_top.is_none() {
                        geom.header_top = Some(rect.top());
                    }
                    crate::table_view::paint_rowno_head(ui.painter(), rect);
                });
                for (vw, c) in columns.iter().enumerate() {
                    header.col(|ui| {
                        let rect = ui.max_rect();
                        paint_grid_head(ui.painter(), rect, c, type_at(column_types, vw));
                        let resp = ui.allocate_rect(rect, Sense::click());
                        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
                    });
                }
                header.col(|ui| {
                    // 末尾占位列:延续表头底色与底线
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, p.header);
                    hline(ui.painter(), rect, p.border);
                });
            })
            .body(|body| {
                // rows() 消费 body:列宽、数据区矩形都先取出来
                let ws = body.widths().to_vec();
                actual_widths.set(ws.clone());
                let brect = body.max_rect();
                // 行悬停:每格用自身矩形判定(跨行累积并工会点亮悬停行下方全部行)
                let hovers = |ui: &egui::Ui, rect: egui::Rect| {
                    ui.input(|i| i.pointer.latest_pos())
                        .map(|pt| {
                            pt.y >= rect.top()
                                && pt.y < rect.bottom()
                                && pt.x >= table_left
                                && pt.x <= table_right
                        })
                        .unwrap_or(false)
                };
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let row_y = std::cell::Cell::new(f32::NAN);
                    let rowdata = &rows[ri];
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        row_y.set(rect.top());
                        paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                        paint_grid_rowno(ui.painter(), rect, ri + 1);
                    });
                    for (ci, col) in columns.iter().enumerate() {
                        let v = &rowdata[ci];
                        // 冻结列已横向滚出视口:编辑框改由覆盖层持有,底层画普通格防双 TextEdit
                        // 用函数开头采样的 prev 值:与覆盖层编辑框归属判定严格同源
                        let frozen_scrolled = frozen_n > 0 && ci < frozen_n && prev_hscroll > 0.5;
                        let is_editing =
                            *editing == Some((ri, ci)) && !frozen_scrolled;
                        row.col(|ui| {
                            let rect = ui.max_rect();
                            let painter = ui.painter().with_clip_rect(rect);
                            let changed = changes.iter().any(|(r, c, _)| *r == ri && c == col);
                            paint_grid_row_bg(&painter, rect, ri, hovers(ui, rect), changed);
                            if is_editing {
                                editing_rendered.set(true);
                                let inner = rect.shrink2(egui::vec2(4.0, 2.0));
                                let mut eui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(inner)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                let resp = eui.add(
                                    egui::TextEdit::singleline(draft)
                                        .font(egui::FontId::proportional(theme::FS_DATA))
                                        .frame(false)
                                        .desired_width(inner.width()),
                                );
                                // 双击出现编辑框后立即抢焦点,否则打字仍进原焦点控件
                                resp.request_focus();
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if esc {
                                    *editing = None;
                                } else if enter || resp.lost_focus() {
                                    commit = Some((ri, ci, draft.clone()));
                                    *editing = None;
                                }
                            } else {
                                let resp = ui.allocate_rect(rect, Sense::click());
                                if resp.double_clicked() {
                                    // 草稿取现值(优先待保存改动):取原始值会让 Enter 静默撤销改动
                                    let shown = changes
                                        .iter()
                                        .find(|(r, c, _)| *r == ri && c == col)
                                        .and_then(|(_, _, nv)| nv.clone())
                                        .or_else(|| v.clone())
                                        .unwrap_or_default();
                                    start_edit = Some((ri, ci, shown));
                                }
                                let shown = changes
                                    .iter()
                                    .find(|(r, c, _)| *r == ri && c == col)
                                    .and_then(|(_, _, nv)| nv.clone())
                                    .or_else(|| v.clone());
                                paint_grid_cell_text(
                                    &painter,
                                    rect,
                                    col,
                                    &shown,
                                    changed,
                                    numeric[ci],
                                );
                            }
                        });
                    }
                    // 末尾占位列:延续行底色
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                    });
                    geom.vis_rows.push((ri, row_y.get()));
                });

                // 空白区网格延续:斑马纹 + 列分隔线(与只读网格同一套)
                {
                    let mut y = brect.top() + rows.len() as f32 * theme::ROW_H;
                    let mut empty_ri = rows.len();
                    while y + theme::ROW_H <= brect.bottom() + 0.5 {
                        let band = egui::Rect::from_min_max(
                            egui::pos2(brect.left(), y),
                            egui::pos2(brect.right(), y + theme::ROW_H),
                        );
                        if empty_ri % 2 == 1 {
                            body_painter.rect_filled(band, 0, p.stripe);
                        }
                        let mut ex = band.left();
                        for w in ws.iter().take(ws.len().saturating_sub(1)) {
                            ex += *w;
                            if ex >= band.right() - 1.0 {
                                break;
                            }
                            body_painter.line_segment(
                                [
                                    egui::pos2(ex - 0.5, band.top()),
                                    egui::pos2(ex - 0.5, band.bottom()),
                                ],
                                egui::Stroke::new(1.0, p.row_line),
                            );
                        }
                        y += theme::ROW_H;
                        empty_ri += 1;
                    }
                }
            });
        });
    store_hscroll(ui.ctx(), id_salt, hout.state.offset.x);
    draw_frozen_grid_overlay(
        ui,
        id_salt,
        columns,
        column_types,
        &numeric,
        rows,
        frozen,
        hout.state.offset.x,
        prev_hscroll,
        &geom,
        &actual_widths,
        outer,
        Some(EditFields {
            changes,
            editing,
            draft,
            start_edit: &mut start_edit,
            commit: &mut commit,
        }),
        &editing_rendered,
    );
    if editing_at_start && editing.is_some() && !editing_rendered.get() {
        *editing = None;
    }
    ui.spacing_mut().item_spacing = old_spacing;
    (start_edit, commit)
}

/// 列头右键:冻结到此列 / 取消冻结列(只读与可编辑结果网格共用)
fn grid_header_freeze_menu(
    resp: &egui::Response,
    vw: usize,
    vis_len: usize,
    frozen: &mut usize,
) {
    let mut act: Option<u8> = None;
    let _ = resp.clone().context_menu(|ui| {
        if vw + 1 < vis_len && ui.button(ui_kit::t!("Freeze to This Column", "冻结到此列")).clicked() {
            act = Some(0);
            ui.close_menu();
        }
        if *frozen > 0 && ui.button(ui_kit::t!("Unfreeze Columns", "取消冻结列")).clicked() {
            act = Some(1);
            ui.close_menu();
        }
    });
    if let Some(a) = act {
        *frozen = if a == 0 { vw + 1 } else { 0 };
    }
}

/// 查询结果网格的冻结覆盖层:横向滚动时把行号+前 N 列钉在视口左侧重绘。
/// 表头、数据格与底层同一套绘制;edits=Some(可编辑网格)时支持改动高亮/双击编辑/
/// 编辑框接管(与底层互斥)。
#[allow(clippy::too_many_arguments)]
fn draw_frozen_grid_overlay(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    numeric: &[bool],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
    offset_x: f32,
    prev_hscroll: f32,
    geom: &GridGeom,
    actual_widths: &std::cell::Cell<Vec<f32>>,
    outer: egui::Rect,
    mut edits: Option<EditFields<'_>>,
    editing_rendered: &std::cell::Cell<bool>,
) {
    let frozen_n = (*frozen).min(columns.len());
    if frozen_n == 0 || offset_x <= 0.5 {
        return;
    }
    let widths = actual_widths.replace(Vec::new());
    let frozen_ws: Vec<f32> = (0..frozen_n)
        .map(|v| widths.get(1 + v).copied().unwrap_or(120.0))
        .collect();
    let p = theme::c();
    let left = outer.left();
    let top = geom.header_top.unwrap_or(outer.top());
    let bottom = outer.bottom();
    let region_w = ROWNO_W + frozen_ws.iter().sum::<f32>();
    if region_w < 1.0 || !(bottom > top) {
        return;
    }
    let region =
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(region_w, bottom - top));
    let painter = ui.painter().with_clip_rect(region);
    painter.rect_filled(region, 0, p.bg);
    let pointer = ui.input(|i| i.pointer.latest_pos());

    // 钉住的表头
    crate::table_view::paint_rowno_head(
        &painter,
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(ROWNO_W, HEADER_H)),
    );
    let mut hx = left + ROWNO_W;
    for (vw, c) in columns.iter().enumerate().take(frozen_n) {
        let w = frozen_ws[vw];
        let rect = egui::Rect::from_min_size(egui::pos2(hx, top), egui::vec2(w, HEADER_H));
        paint_grid_head(&painter, rect, c, type_at(column_types, vw));
        let resp = ui.interact(rect, egui::Id::new(("qfz-h", id_salt, vw)), Sense::click());
        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
        hx += w;
    }

    // 钉住的数据行(y 用渲染时记录的真实行带;滚动偏移非行高整数倍时不可推算)
    for &(ri, y) in geom.vis_rows.iter() {
        if ri >= rows.len() || y.is_nan() {
            continue;
        }
        if y >= bottom {
            break;
        }
        let rowdata = &rows[ri];
        let hovered = pointer
            .map(|pt| pt.y >= y && pt.y < y + theme::ROW_H && pt.x >= left && pt.x < left + region_w)
            .unwrap_or(false);
        paint_grid_row_bg(
            &painter,
            egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(region_w, theme::ROW_H)),
            ri,
            hovered,
            false,
        );
        paint_grid_rowno(
            &painter,
            egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(ROWNO_W, theme::ROW_H)),
            ri + 1,
        );
        let mut x = left + ROWNO_W;
        for (ci, col) in columns.iter().enumerate().take(frozen_n) {
            let w = frozen_ws[ci];
            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, theme::ROW_H));
            let v = &rowdata[ci.min(rowdata.len().saturating_sub(1))];
            let cp = painter.with_clip_rect(rect);
            let changed = edits
                .as_ref()
                .map(|e| e.changes.iter().any(|(r, c, _)| *r == ri && c == col))
                .unwrap_or(false);
            paint_grid_row_bg(&cp, rect, ri, hovered, changed);
            let mut editing_now = false;
            if let Some(e) = edits.as_ref() {
                editing_now = *e.editing == Some((ri, ci)) && prev_hscroll > 0.5;
            }
            if editing_now {
                editing_rendered.set(true);
                let e = edits.as_mut().unwrap();
                let inner = rect.shrink2(egui::vec2(4.0, 2.0));
                let mut eui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let resp = eui.add(
                    egui::TextEdit::singleline(e.draft)
                        .font(egui::FontId::proportional(theme::FS_DATA))
                        .frame(false)
                        .desired_width(inner.width()),
                );
                resp.request_focus();
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.command);
                if esc {
                    *e.editing = None;
                } else if enter || resp.lost_focus() {
                    *e.commit = Some((ri, ci, e.draft.clone()));
                    *e.editing = None;
                }
            } else {
                // 双击编辑(仅可编辑网格)
                if edits.is_some() {
                    let resp = ui.interact(rect, egui::Id::new(("qfz-c", id_salt, ri, ci)), Sense::click());
                    if resp.double_clicked() {
                        if let Some(e) = edits.as_mut() {
                            let shown = e
                                .changes
                                .iter()
                                .find(|(r, c, _)| *r == ri && c == col)
                                .and_then(|(_, _, nv)| nv.clone())
                                .or_else(|| v.clone())
                                .unwrap_or_default();
                            *e.start_edit = Some((ri, ci, shown));
                        }
                    }
                }
                let shown = edits
                    .as_ref()
                    .and_then(|e| {
                        e.changes
                            .iter()
                            .find(|(r, c, _)| *r == ri && c == col)
                            .and_then(|(_, _, nv)| nv.clone())
                    })
                    .or_else(|| v.clone());
                paint_grid_cell_text(
                    &cp,
                    rect,
                    col,
                    &shown,
                    changed,
                    numeric.get(ci).copied().unwrap_or(false),
                );
            }
            x += w;
        }
    }
    // 与滚动区分界的右缘竖线
    painter.line_segment(
        [
            egui::pos2(left + region_w - 0.5, top),
            egui::pos2(left + region_w - 0.5, bottom),
        ],
        egui::Stroke::new(2.0, p.border),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(spec: &[&[Option<&str>]]) -> Vec<Vec<Option<String>>> {
        spec.iter()
            .map(|r| r.iter().map(|v| v.map(|s| s.to_string())).collect())
            .collect()
    }

    /// PG 的简单查询协议、执行计划、Parquet 预览都没有类型:类型数组为空也不能越界,
    /// 数字列退回按值预判
    #[test]
    fn grid_without_column_types_still_works() {
        let cols = ["id", "name", "zip", "amount"].map(String::from);
        let types: [String; 0] = [];
        let data = rows(&[
            &[Some("1"), Some("张三"), Some("010000"), Some("12.50")],
            &[Some("2"), None, Some("100000"), Some("-3")],
        ]);
        let flags = numeric_flags(&cols, &types, &data);
        assert_eq!(flags, [true, false, false, true]);
        // 列数与类型数不一致时以列数为准
        let short = ["bigint".to_string()];
        assert_eq!(numeric_flags(&cols, &short, &data), [true, false, false, true]);
        let w = fitted_widths_typed(&data, &cols, &types, 800.0);
        assert_eq!(w.len(), cols.len());
        assert!(w.iter().all(|x| *x >= 84.0), "{w:?}");
    }

    /// 有类型时只看类型:存成文本的数字串(varchar)不右对齐
    #[test]
    fn numeric_flags_follow_types_when_present() {
        let cols = ["a", "b", "c"].map(String::from);
        let types = ["varchar", "int unsigned", "decimal"].map(String::from);
        let data = rows(&[&[Some("123"), Some("5"), Some("1.5")]]);
        assert_eq!(numeric_flags(&cols, &types, &data), [false, true, true]);
    }

    /// 按值预判的边界:前导零 / 超长数字串 / 非数字 / 全 NULL / 没有行 都不算数字列
    #[test]
    fn value_inference_rejects_identifiers_and_empty_columns() {
        let cols = ["a", "b", "c", "d", "e"].map(String::from);
        let types: [String; 0] = [];
        let data = rows(&[
            &[Some("0123"), Some("6222020200112345678"), Some("12a"), None, Some("1")],
            &[Some("456"), Some("6222030300987654321"), None, None, Some("2.5")],
        ]);
        assert_eq!(numeric_flags(&cols, &types, &data), [false, false, false, false, true]);
        // 一行数据都没有:无从判断,全部不右对齐
        assert_eq!(numeric_flags(&cols, &types, &[]), [false; 5]);
        assert!(looks_numeric("-3.5e2") && looks_numeric("0.5") && looks_numeric("0") && looks_numeric("123456789012345"));
        for (v, why) in [
            ("", "空"),
            ("NaN", "不是十进制数字"),
            ("inf", "不是十进制数字"),
            ("1,000", "千分位"),
            ("1234567890123456", "16 位"),
            ("6222020200112345678", "卡号"),
        ] {
            assert!(!looks_numeric(v), "{why}: {v:?}");
        }
    }
}
