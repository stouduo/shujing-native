//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化/横向滚动)。
//! 查询结果页直接复用;视觉与表数据页一致(32pt 行高、36pt 表头、等宽数据)。

use crate::table_view::{badge_value, paint_badge};
use crate::{icons, theme};
use egui::{Color32, Sense};
use egui_extras::{Column, TableBuilder};

/// 渲染只读结果集(带行号列、行发丝线)
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
) {
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let col_widths = crate::table_view::fitted_widths(rows, columns, ui.available_width());
    let desired_w = 48.0 + col_widths.iter().sum::<f32>();
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    // 结果网格和表数据页共用同一套横向滚动条策略:常驻、实体、高可见度
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.9;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.15;
    ui.spacing_mut().scroll.interact_handle_opacity = 1.0;
    ui.spacing_mut().scroll.active_handle_opacity = 1.0;
    ui.spacing_mut().scroll.bar_width = 12.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
    egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));
            // 拖拽平移:按住结果网格左右拖 = 横向滚动(只读网格无编辑态)
            {
                let drag_pan = ui.input(|i| {
                    i.pointer.button_down(egui::PointerButton::Primary)
                        && i.pointer.is_decidedly_dragging()
                });
                if drag_pan {
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
            // 内容宽度 + 末尾占位列:窄网格铺满,宽网格溢出并出现横向滚动条
            for (i, _c) in columns.iter().enumerate() {
                tb = tb.column(Column::exact(col_widths[i]).resizable(true).clip(true));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(theme::HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
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
                for c in columns {
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
                let mut row_band: Option<(f32, f32)> = None;
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let rowdata = &rows[ri];
                    let mut hovered = false;
                    row.col(|ui| {
                        let band =
                            row_band.get_or_insert((ui.max_rect().top(), ui.max_rect().bottom()));
                        band.0 = band.0.min(ui.max_rect().top());
                        band.1 = band.1.max(ui.max_rect().bottom());
                        hovered = ui
                            .input(|i| i.pointer.latest_pos())
                            .map(|pt| {
                                pt.y >= band.0
                                    && pt.y < band.1
                                    && pt.x >= table_left
                                    && pt.x <= table_right
                            })
                            .unwrap_or(false);
                        let rect = ui.max_rect();
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
                });
            });
        });
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
    // 结果网格和表数据页共用同一套横向滚动条策略:常驻、实体、高可见度
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.9;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.15;
    ui.spacing_mut().scroll.interact_handle_opacity = 1.0;
    ui.spacing_mut().scroll.active_handle_opacity = 1.0;
    ui.spacing_mut().scroll.bar_width = 12.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
    egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));
            // 拖拽平移(编辑态除外)
            if !editing_active {
                let drag_pan = ui.input(|i| {
                    i.pointer.button_down(egui::PointerButton::Primary)
                        && i.pointer.is_decidedly_dragging()
                });
                if drag_pan {
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
            // 内容宽度 + 末尾占位列:窄网格铺满,宽网格溢出并出现横向滚动条
            for (i, _c) in columns.iter().enumerate() {
                tb = tb.column(Column::exact(col_widths[i]).resizable(true).clip(true));
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            tb.header(theme::HEADER_H, |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
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
                for c in columns {
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
                let mut row_band: Option<(f32, f32)> = None;
                body.rows(theme::ROW_H, rows.len(), |mut row| {
                    let ri = row.index();
                    let rowdata = &rows[ri];
                    let mut hovered = false;
                    row.col(|ui| {
                        let band =
                            row_band.get_or_insert((ui.max_rect().top(), ui.max_rect().bottom()));
                        band.0 = band.0.min(ui.max_rect().top());
                        band.1 = band.1.max(ui.max_rect().bottom());
                        hovered = ui
                            .input(|i| i.pointer.latest_pos())
                            .map(|pt| {
                                pt.y >= band.0
                                    && pt.y < band.1
                                    && pt.x >= table_left
                                    && pt.x <= table_right
                            })
                            .unwrap_or(false);
                        let rect = ui.max_rect();
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
                            let painter = ui.painter().with_clip_rect(rect);
                            let is_editing = *editing == Some((ri, ci));
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
                                        .desired_width(inner.width().max(80.0)),
                                );
                                if enter || resp.lost_focus() {
                                    commit = Some((ri, ci, draft.clone()));
                                    *editing = None;
                                }
                            } else {
                                let resp = ui.allocate_rect(rect, Sense::click());
                                if resp.double_clicked() {
                                    start_edit = Some((ri, ci, v.clone().unwrap_or_default()));
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
                });
            });
        });
    ui.spacing_mut().item_spacing = old_spacing;
    (start_edit, commit)
}
