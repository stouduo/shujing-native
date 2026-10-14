//! 结构页:字段/索引/DDL。

use crate::app::StructureTab;
use crate::table_view::TabAction;
use egui::RichText;

pub fn draw_structure_view(ctx: &egui::Context, s: &mut StructureTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id = format!("{}|{}", s.conn_id, s.table);

    egui::TopBottomPanel::top("struct_toolbar")
        .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.table).strong().size(14.0));
                if let Some(db) = &s.database {
                    ui.label(RichText::new(db).weak().size(11.0));
                }
                if s.loading {
                    ui.spinner();
                }
                if let Some(err) = &s.error {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 120, 120),
                        RichText::new(err).size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("刷新").clicked() && !s.loading {
                        actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                    }
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(data) = &s.data else {
            ui.centered_and_justified(|ui| {
                ui.label(if s.loading { "加载中…" } else { "无数据" });
            });
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("struct-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading(RichText::new(format!("字段 ({})", data.columns.len())).size(13.0));
                egui::Grid::new("struct-cols")
                    .striped(true)
                    .num_columns(7)
                    .show(ui, |ui| {
                        for c in &data.columns {
                            let mut name = RichText::new(&c.name).size(12.0).monospace();
                            if c.key == "PRI" {
                                name = name.color(egui::Color32::from_rgb(133, 135, 246));
                            }
                            ui.label(name);
                            ui.label(RichText::new(&c.data_type).size(12.0).monospace());
                            ui.label(RichText::new(if c.nullable { "YES" } else { "NO" }).size(12.0));
                            ui.label(RichText::new(c.key.clone()).size(12.0));
                            ui.label(
                                RichText::new(
                                    c.default.clone().unwrap_or_else(|| "NULL".into()),
                                )
                                .size(12.0),
                            );
                            ui.label(RichText::new(c.comment.clone()).size(12.0));
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.heading(RichText::new(format!("索引 ({})", data.indexes.len())).size(13.0));
                for ix in &data.indexes {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&ix.name).size(12.0).monospace());
                        ui.label(RichText::new(&ix.columns).size(12.0).weak());
                        if ix.unique {
                            ui.label(RichText::new("UNIQUE").size(11.0).color(egui::Color32::from_rgb(133, 135, 246)));
                        }
                    });
                }
                ui.add_space(12.0);
                ui.heading(RichText::new("DDL (建表语句)").size(13.0));
                egui::CollapsingHeader::new(RichText::new("展开/收起").size(12.0))
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("ddl-scroll")
                            .max_height(360.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(data.ddl.clone()).size(12.0).monospace(),
                                );
                            });
                    });
            });
    });

    actions
}
