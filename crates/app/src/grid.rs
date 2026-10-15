//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化)。
//! 查询结果页直接复用;表数据页因带编辑/排序单独实现。

use crate::{icons, theme};
use egui_extras::{Column, TableBuilder};

/// 渲染只读结果集(带行号列)
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
) {
    let mut tb = TableBuilder::new(ui)
        .id_salt(id_salt)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .vscroll(true)
        .column(Column::exact(52.0));
    let ncols = columns.len();
    for (i, _c) in columns.iter().enumerate() {
        tb = tb.column(if i + 1 == ncols {
            Column::remainder().clip(true)
        } else {
            Column::initial(160.0)
                .resizable(true)
                .clip(true)
                .at_least(48.0)
        });
    }
    tb.header(26.0, |mut header| {
        header.col(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("#").size(12.0).color(theme::MUTED));
            });
        });
        for c in columns {
            header.col(|ui| {
                ui.label(egui::RichText::new(c).size(13.0).strong());
            });
        }
    })
    .body(|mut body| {
        for (ri, rowdata) in rows.iter().enumerate() {
            body.row(24.0, |mut row| {
                row.col(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{}", ri + 1)).size(12.0).color(theme::MUTED),
                        );
                    });
                });
                for v in rowdata {
                    row.col(|ui| draw_readonly_cell(ui, v));
                }
            });
        }
    });
}

/// 只读单元格:满格绘制、裁剪超长文本、NULL 弱色
fn draw_readonly_cell(ui: &mut egui::Ui, v: &Option<String>) {
    let text = icons::cell_text(v, 2000);
    let color = icons::cell_text_color(v, false);
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(12.0),
        color,
    );
    let pos = egui::Pos2::new(
        rect.left() + 8.0,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(pos, galley, color);
}
