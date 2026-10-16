//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化/横向滚动)。
//! 查询结果页直接复用;表数据页因带编辑/排序单独实现。

use crate::{icons, theme};
use egui_extras::{Column, TableBuilder};

/// 渲染只读结果集(带行号列、行发丝线)
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
) {
    // 宽结果集横向滚动(表格自身 hscroll 硬编码 false);滚动条隐藏(iOS 式)
    let col_widths = crate::table_view::suggested_widths(rows, columns);
    let desired_w = 52.0 + col_widths.iter().sum::<f32>() + 200.0;
    egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));

            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(52.0));
            let ncols = columns.len();
            for (i, _c) in columns.iter().enumerate() {
                tb = tb.column(if i + 1 == ncols {
                    Column::remainder().clip(true)
                } else {
                    Column::auto_with_initial_suggestion(col_widths[i])
                        .at_least(48.0)
                        .at_most(360.0)
                        .resizable(true)
                        .clip(true)
                });
            }
            tb.header(30.0, |mut header| {
                header.col(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("#").size(11.5).color(theme::MUTED));
                    });
                    hairline(ui, theme::BORDER);
                });
                for c in columns {
                    header.col(|ui| {
                        ui.label(egui::RichText::new(c).size(12.5).strong());
                        hairline(ui, theme::BORDER);
                    });
                }
            })
            .body(|mut body| {
                for (ri, rowdata) in rows.iter().enumerate() {
                    body.row(28.0, |mut row| {
                        row.col(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}", ri + 1))
                                        .size(11.0)
                                        .color(theme::MUTED),
                                );
                            });
                        });
                        for v in rowdata {
                            row.col(|ui| {
                                draw_readonly_cell(ui, v);
                            });
                        }
                    });
                }
            });
        });
}

/// 单元格底部发丝线
fn hairline(ui: &egui::Ui, color: egui::Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, color),
    );
}

/// 只读单元格:满格绘制、裁剪超长文本、NULL 弱色
fn draw_readonly_cell(ui: &mut egui::Ui, v: &Option<String>) {
    let text = icons::cell_text(v, 2000);
    let color = icons::cell_text_color(v, false);
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    let font = if v.is_none() {
        egui::FontId::monospace(11.5)
    } else {
        egui::FontId::monospace(12.0)
    };
    let galley = painter.layout_no_wrap(text, font, color);
    let pos = egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(pos, galley, color);
}
