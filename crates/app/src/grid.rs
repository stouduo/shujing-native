//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化/横向滚动)。
//! 查询结果页直接复用;视觉与表数据页一致(32pt 行高、36pt 表头、等宽数据)。

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

/// 渲染只读结果集(带行号列、行发丝线)
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
) {
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let col_widths = crate::table_view::fitted_widths(rows, columns, ui.available_width());
    let desired_w = 48.0 + col_widths.iter().sum::<f32>();
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
                            && p.y > content.top() + theme::HEADER_H
                            && p.x < content.right() - 16.0
                    })
                    .unwrap_or(false);
                if drag_pan && in_body {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    if dx.abs() > 0.1 {
                        ui.scroll_with_delta(egui::vec2(-dx, 0.0));
                    }
                }
            }
            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(false)
                .sense(Sense::hover())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(48.0));
            // initial(而非 exact):exact 把 width_range 钉死在 w..=w,拖拽调宽无效
            for (i, _c) in columns.iter().enumerate() {
                tb = tb
                    .column(Column::initial(col_widths[i]).resizable(true).clip(true).at_least(40.0));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(theme::HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
                    if geom.header_top.is_none() {
                        geom.header_top = Some(rect.top());
                    }
                    ui.painter().rect_filled(rect, 0, p.panel);
                    ui.painter().text(
                        egui::pos2(rect.right() - 14.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        "#",
                        egui::FontId::proportional(11.0),
                        p.muted,
                    );
                    header_line(ui, p.border);
                });
                for (vw, c) in columns.iter().enumerate() {
                    header.col(|ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0, p.panel);
                        let font = egui::FontId::proportional(theme::FS_HEADER);
                        let g = icons::text_galley(
                            ui.painter(),
                            c.clone(),
                            font,
                            p.text2,
                            rect.width() - 24.0,
                        );
                        ui.painter().galley(
                            egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
                            g,
                            p.text2,
                        );
                        let resp = ui.allocate_rect(rect, Sense::click());
                        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
                        header_line(ui, p.border);
                    });
                }
                header.col(|ui| {
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, p.panel);
                    header_line(ui, p.border);
                });
            })
            .body(|body| {
                actual_widths.set(body.widths().to_vec());
                // 行悬停:每格用自身矩形判定(跨行累积并工会点亮悬停行下方全部行)
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let row_y = std::cell::Cell::new(f32::NAN);
                    let rowdata = &rows[ri];
                    let mut hovered = false;
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        row_y.set(rect.top());
                        hovered = ui
                            .input(|i| i.pointer.latest_pos())
                            .map(|pt| {
                                pt.y >= rect.top()
                                    && pt.y < rect.bottom()
                                    && pt.x >= table_left
                                    && pt.x <= table_right
                            })
                            .unwrap_or(false);
                        if hovered {
                            ui.painter().rect_filled(rect, 0, p.row_hover);
                        }
                        ui.painter().text(
                            egui::pos2(rect.right() - 14.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format!("{}", ri + 1),
                            egui::FontId::proportional(11.0),
                            p.muted,
                        );
                        row_line(ui, p.row_line);
                    });
                    for (ci, col) in columns.iter().enumerate() {
                        let v = &rowdata[ci];
                        row.col(|ui| {
                            let rect = ui.max_rect();
                            hovered = ui
                                .input(|i| i.pointer.latest_pos())
                                .map(|pt| {
                                    pt.y >= rect.top()
                                        && pt.y < rect.bottom()
                                        && pt.x >= table_left
                                        && pt.x <= table_right
                                })
                                .unwrap_or(false);
                            let painter = ui.painter().with_clip_rect(rect);
                            if hovered {
                                painter.rect_filled(rect, 0, p.row_hover);
                            }
                            if let Some((label, on)) = badge_value(col, v) {
                                paint_badge(
                                    &painter,
                                    rect.left() + 12.0,
                                    rect.center().y,
                                    &label,
                                    on,
                                );
                            } else {
                                let color = icons::cell_text_color(v, false);
                                let g = icons::text_galley(
                                    &painter,
                                    icons::cell_text(v, 4000),
                                    theme::mono(theme::FS_DATA),
                                    color,
                                    rect.width() - 24.0,
                                );
                                painter.galley(
                                    egui::pos2(
                                        rect.left() + 12.0,
                                        rect.center().y - g.size().y / 2.0,
                                    ),
                                    g,
                                    color,
                                );
                            }
                            row_line(ui, p.row_line);
                        });
                    }
                    // 末尾占位列:延续行背景/行线
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        if hovered {
                            ui.painter().rect_filled(rect, 0, p.row_hover);
                        }
                        row_line(ui, p.row_line);
                    });
                    geom.vis_rows.push((ri, row_y.get()));
                });
            });
        });
    store_hscroll(ui.ctx(), id_salt, hout.state.offset.x);
    draw_frozen_grid_overlay(
        ui,
        id_salt,
        columns,
        rows,
        frozen,
        hout.state.offset.x,
        prev_hscroll,
        &geom,
        &actual_widths,
        outer,
        None,
    );
    ui.spacing_mut().item_spacing = old_spacing;
}

fn header_line(ui: &egui::Ui, color: Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, color),
    );
}

fn row_line(ui: &egui::Ui, color: Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, color),
    );
}

/// 可编辑结果集(查询回写):双击进入编辑,改动以 (行号,列名,新值) 记在 changes。
/// 返回 (请求开始编辑的格, 提交的格);开始编辑由调用方置 draft,提交由调用方记 changes。
pub fn result_rows_editable(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
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
    let col_widths = crate::table_view::fitted_widths(rows, columns, ui.available_width());
    let desired_w = 48.0 + col_widths.iter().sum::<f32>();
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.command);
    let editing_active = editing.is_some();
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
                            && p.y > content.top() + theme::HEADER_H
                            && p.x < content.right() - 16.0
                    })
                    .unwrap_or(false);
                if drag_pan && in_body {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    if dx.abs() > 0.1 {
                        ui.scroll_with_delta(egui::vec2(-dx, 0.0));
                    }
                }
            }
            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(false)
                .sense(Sense::hover())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(48.0));
            // initial(而非 exact):exact 把 width_range 钉死在 w..=w,拖拽调宽无效
            for (i, _c) in columns.iter().enumerate() {
                tb = tb
                    .column(Column::initial(col_widths[i]).resizable(true).clip(true).at_least(40.0));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(theme::HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
                    if geom.header_top.is_none() {
                        geom.header_top = Some(rect.top());
                    }
                    ui.painter().rect_filled(rect, 0, p.panel);
                    ui.painter().text(
                        egui::pos2(rect.right() - 14.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        "#",
                        egui::FontId::proportional(11.0),
                        p.muted,
                    );
                    header_line(ui, p.border);
                });
                for (vw, c) in columns.iter().enumerate() {
                    header.col(|ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0, p.panel);
                        let g = icons::text_galley(
                            ui.painter(),
                            c.clone(),
                            egui::FontId::proportional(theme::FS_HEADER),
                            p.text2,
                            rect.width() - 24.0,
                        );
                        ui.painter().galley(
                            egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
                            g,
                            p.text2,
                        );
                        let resp = ui.allocate_rect(rect, Sense::click());
                        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
                        header_line(ui, p.border);
                    });
                }
                header.col(|ui| {
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, p.panel);
                    header_line(ui, p.border);
                });
            })
            .body(|body| {
                actual_widths.set(body.widths().to_vec());
                // 行悬停:每格用自身矩形判定(跨行累积并工会点亮悬停行下方全部行)
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let row_y = std::cell::Cell::new(f32::NAN);
                    let rowdata = &rows[ri];
                    let mut hovered = false;
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        row_y.set(rect.top());
                        hovered = ui
                            .input(|i| i.pointer.latest_pos())
                            .map(|pt| {
                                pt.y >= rect.top()
                                    && pt.y < rect.bottom()
                                    && pt.x >= table_left
                                    && pt.x <= table_right
                            })
                            .unwrap_or(false);
                        if hovered {
                            ui.painter().rect_filled(rect, 0, p.row_hover);
                        }
                        ui.painter().text(
                            egui::pos2(rect.right() - 14.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format!("{}", ri + 1),
                            egui::FontId::proportional(11.0),
                            p.muted,
                        );
                        row_line(ui, p.row_line);
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
                            hovered = ui
                                .input(|i| i.pointer.latest_pos())
                                .map(|pt| {
                                    pt.y >= rect.top()
                                        && pt.y < rect.bottom()
                                        && pt.x >= table_left
                                        && pt.x <= table_right
                                })
                                .unwrap_or(false);
                            let painter = ui.painter().with_clip_rect(rect);
                            let changed = changes.iter().any(|(r, c, _)| *r == ri && c == col);
                            if hovered || changed {
                                painter.rect_filled(
                                    rect,
                                    0,
                                    if changed { p.change_bg } else { p.row_hover },
                                );
                            }
                            if is_editing {
                                let inner = rect.shrink2(egui::vec2(4.0, 2.0));
                                let mut eui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(inner)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                let resp = eui.add(
                                    egui::TextEdit::singleline(draft)
                                        .font(theme::mono(theme::FS_DATA))
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
                                if let Some((label, on)) = badge_value(col, v) {
                                    paint_badge(
                                        &painter,
                                        rect.left() + 12.0,
                                        rect.center().y,
                                        &label,
                                        on,
                                    );
                                } else {
                                    let shown = changes
                                        .iter()
                                        .find(|(r, c, _)| *r == ri && c == col)
                                        .and_then(|(_, _, nv)| nv.clone())
                                        .or_else(|| v.clone());
                                    let color = icons::cell_text_color(&shown, changed);
                                    let g = icons::text_galley(
                                        &painter,
                                        icons::cell_text(&shown, 4000),
                                        theme::mono(theme::FS_DATA),
                                        color,
                                        rect.width() - 24.0,
                                    );
                                    painter.galley(
                                        egui::pos2(
                                            rect.left() + 12.0,
                                            rect.center().y - g.size().y / 2.0,
                                        ),
                                        g,
                                        color,
                                    );
                                }
                            }
                            row_line(ui, p.row_line);
                        });
                    }
                    // 末尾占位列:延续行背景/行线
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        if hovered {
                            ui.painter().rect_filled(rect, 0, p.row_hover);
                        }
                        row_line(ui, p.row_line);
                    });
                    geom.vis_rows.push((ri, row_y.get()));
                });
            });
        });
    store_hscroll(ui.ctx(), id_salt, hout.state.offset.x);
    draw_frozen_grid_overlay(
        ui,
        id_salt,
        columns,
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
    );
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
        if vw + 1 < vis_len && ui.button("冻结到此列").clicked() {
            act = Some(0);
            ui.close_menu();
        }
        if *frozen > 0 && ui.button("取消冻结列").clicked() {
            act = Some(1);
            ui.close_menu();
        }
    });
    if let Some(a) = act {
        *frozen = if a == 0 { vw + 1 } else { 0 };
    }
}

/// 查询结果网格的冻结覆盖层:横向滚动时把行号+前 N 列钉在视口左侧重绘。
/// edits=Some(可编辑网格)时支持改动高亮/双击编辑/编辑框接管(与底层互斥)。
#[allow(clippy::too_many_arguments)]
fn draw_frozen_grid_overlay(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
    offset_x: f32,
    prev_hscroll: f32,
    geom: &GridGeom,
    actual_widths: &std::cell::Cell<Vec<f32>>,
    outer: egui::Rect,
    mut edits: Option<EditFields<'_>>,
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
    let region_w = 48.0 + frozen_ws.iter().sum::<f32>();
    if region_w < 1.0 || !(bottom > top) {
        return;
    }
    let region =
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(region_w, bottom - top));
    let painter = ui.painter().with_clip_rect(region);
    painter.rect_filled(region, 0, p.bg);
    let pointer = ui.input(|i| i.pointer.latest_pos());

    // 钉住的表头
    let header_rect =
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(region_w, theme::HEADER_H));
    painter.rect_filled(header_rect, 0, p.panel);
    painter.text(
        egui::pos2(left + 48.0 - 14.0, header_rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "#",
        egui::FontId::proportional(11.0),
        p.muted,
    );
    let mut hx = left + 48.0;
    for (vw, c) in columns.iter().enumerate().take(frozen_n) {
        let w = frozen_ws[vw];
        let rect = egui::Rect::from_min_size(egui::pos2(hx, top), egui::vec2(w, theme::HEADER_H));
        let cp = painter.with_clip_rect(rect);
        let g = icons::text_galley(
            &cp,
            c.clone(),
            egui::FontId::proportional(theme::FS_HEADER),
            p.text2,
            rect.width() - 24.0,
        );
        cp.galley(
            egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
            g,
            p.text2,
        );
        let resp = ui.interact(rect, egui::Id::new(("qfz-h", id_salt, vw)), Sense::click());
        grid_header_freeze_menu(&resp, vw, columns.len(), frozen);
        hx += w;
    }
    painter.line_segment(
        [
            egui::pos2(left, header_rect.bottom() - 0.5),
            egui::pos2(header_rect.right(), header_rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, p.border),
    );

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
        let band = egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(region_w, theme::ROW_H));
        if hovered {
            painter.rect_filled(band, 0, p.row_hover);
        }
        painter.text(
            egui::pos2(left + 48.0 - 14.0, y + theme::ROW_H / 2.0),
            egui::Align2::RIGHT_CENTER,
            format!("{}", ri + 1),
            egui::FontId::proportional(11.0),
            p.muted,
        );
        painter.line_segment(
            [
                egui::pos2(left, y + theme::ROW_H - 0.5),
                egui::pos2(left + region_w, y + theme::ROW_H - 0.5),
            ],
            egui::Stroke::new(1.0, p.row_line),
        );
        let mut x = left + 48.0;
        for (ci, col) in columns.iter().enumerate().take(frozen_n) {
            let w = frozen_ws[ci];
            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, theme::ROW_H));
            let v = &rowdata[ci.min(rowdata.len().saturating_sub(1))];
            let cp = painter.with_clip_rect(rect);
            let changed = edits
                .as_ref()
                .map(|e| e.changes.iter().any(|(r, c, _)| *r == ri && c == col))
                .unwrap_or(false);
            if hovered || changed {
                cp.rect_filled(
                    rect,
                    0,
                    if changed { p.change_bg } else { p.row_hover },
                );
            }
            let mut editing_now = false;
            if let Some(e) = edits.as_ref() {
                editing_now = *e.editing == Some((ri, ci)) && prev_hscroll > 0.5;
            }
            if editing_now {
                let e = edits.as_mut().unwrap();
                let inner = rect.shrink2(egui::vec2(4.0, 2.0));
                let mut eui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let resp = eui.add(
                    egui::TextEdit::singleline(e.draft)
                        .font(theme::mono(theme::FS_DATA))
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
                if let Some((label, on)) = badge_value(col, v) {
                    paint_badge(&cp, rect.left() + 12.0, rect.center().y, &label, on);
                } else {
                    let shown = edits
                        .as_ref()
                        .and_then(|e| {
                            e.changes
                                .iter()
                                .find(|(r, c, _)| *r == ri && c == col)
                                .and_then(|(_, _, nv)| nv.clone())
                        })
                        .or_else(|| v.clone());
                    let color = icons::cell_text_color(&shown, changed);
                    let g = icons::text_galley(
                        &cp,
                        icons::cell_text(&shown, 4000),
                        theme::mono(theme::FS_DATA),
                        color,
                        rect.width() - 24.0,
                    );
                    cp.galley(
                        egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
                        g,
                        color,
                    );
                }
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
