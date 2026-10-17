//! 结构页:字段/索引/DDL。

use crate::app::StructureTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::RichText;

pub fn draw_structure_view(
    ctx: &egui::Context,
    tab_id: &str,
    s: &mut StructureTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();

    egui::TopBottomPanel::top("struct_toolbar")
        .frame(
            egui::Frame::new()
                .fill(theme::c().panel)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.table).strong().size(15.0));
                if let Some(db) = &s.database {
                    ui.label(RichText::new(db).size(12.0).color(theme::c().muted));
                }
                if s.loading {
                    ui.spinner();
                }
                if let Some(err) = &s.error {
                    ui.label(RichText::new(icons::clip(err, 120)).size(12.0).color(theme::c().red));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !s.loading && crate::theme::text_button(ui, "刷新") {
                        actions.push(TabAction::Reload { tab_id: tab_id.to_string() });
                    }
                    if let Some(d) = &s.data {
                        if crate::theme::text_button(ui, "复制 DDL") {
                            ui.ctx().copy_text(d.ddl.clone());
                        }
                    }
                });
            });
            crate::theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(data) = &s.data else {
            ui.centered_and_justified(|ui| {
                let msg = if s.loading {
                    "加载中…".to_string()
                } else {
                    s.error.clone().unwrap_or_else(|| "无数据".into())
                };
                ui.label(RichText::new(msg).color(theme::c().muted));
            });
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("struct-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("字段 ({})", data.columns.len()))
                        .size(12.5)
                        .strong()
                        .color(theme::c().text2),
                );
                ui.add_space(4.0);
                let cols = data.columns.clone();
                egui::ScrollArea::horizontal()
                    .id_salt("struct-hscroll")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width().max(720.0));
                        use egui_extras::{Column, TableBuilder};
                        let mut tb = TableBuilder::new(ui)
                            .id_salt("struct-table")
                            .striped(false)
                            .vscroll(false)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Column::initial(180.0).at_least(120.0).resizable(true).clip(true))
                            .column(Column::initial(120.0).at_least(80.0).resizable(true).clip(true))
                            .column(Column::exact(56.0))
                            .column(Column::exact(64.0))
                            .column(Column::initial(110.0).at_least(70.0).resizable(true).clip(true))
                            .column(Column::remainder().clip(true));
                        tb.header(32.0, |mut header| {
                            for h in ["名称", "类型", "可空", "键", "默认值", "注释"] {
                                header.col(|ui| {
                                    ui.painter().rect_filled(
                                        ui.max_rect().expand2(egui::vec2(4.0, 0.0)),
                                        0,
                                        theme::c().panel,
                                    );
                                    ui.label(RichText::new(h).size(12.0).strong().color(theme::c().muted));
                                    hairline_h(ui, theme::c().border);
                                });
                            }
                        })
                        .body(|mut body| {
                            for c in &cols {
                                let is_pk = c.key == "PRI";
                                body.row(30.0, |mut row| {
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(&c.name)
                                                .size(12.5)
                                                .monospace()
                                                .color(if is_pk { theme::c().accent } else { theme::c().text }),
                                        );
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(&c.data_type)
                                                .size(12.0)
                                                .monospace()
                                                .color(theme::c().text2),
                                        );
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(if c.nullable { "YES" } else { "NO" })
                                                .size(12.0)
                                                .color(if c.nullable { theme::c().muted } else { theme::c().text2 }),
                                        );
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                    row.col(|ui| {
                                        if is_pk {
                                            icons::key_badge(ui);
                                        } else {
                                            ui.label(RichText::new(&c.key).size(12.0).color(theme::c().muted));
                                        }
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(c.default.clone().unwrap_or_else(|| "null".into()))
                                                .size(12.0)
                                                .monospace()
                                                .color(theme::c().text2),
                                        );
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                    row.col(|ui| {
                                        ui.label(RichText::new(&c.comment).size(12.0).color(theme::c().muted));
                                        hairline_h(ui, theme::c().row_line);
                                    });
                                });
                            }
                        });
                    });

                ui.add_space(18.0);
                ui.label(
                    RichText::new(format!("索引 ({})", data.indexes.len()))
                        .size(12.5)
                        .strong()
                        .color(theme::c().text2),
                );
                ui.add_space(4.0);
                for ix in &data.indexes {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&ix.name)
                                .size(12.5)
                                .monospace()
                                .color(theme::c().text),
                        );
                        ui.label(RichText::new(&ix.columns).size(12.0).color(theme::c().muted));
                        if ix.unique {
                            icons::chip(ui, "UNIQUE", theme::c().accent);
                        }
                    });
                    hairline_h(ui, theme::c().row_line);
                    ui.add_space(4.0);
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new("DDL")
                        .size(12.5)
                        .strong()
                        .color(theme::c().text2),
                );
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(theme::c().input_bg)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("ddl-scroll")
                            .max_height(320.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(data.ddl.clone())
                                            .size(12.0)
                                            .monospace()
                                            .color(theme::c().text2),
                                    )
                                    .selectable(true),
                                );
                            });
                    });
            });
    });

    actions
}

/// 当前行底部发丝线
fn hairline_h(ui: &egui::Ui, color: egui::Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, color),
    );
}
