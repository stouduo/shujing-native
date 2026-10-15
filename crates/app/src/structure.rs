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
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.table).strong().size(14.0));
                if let Some(db) = &s.database {
                    ui.label(RichText::new(db).size(11.0).color(theme::MUTED));
                }
                if s.loading {
                    ui.spinner();
                }
                if let Some(err) = &s.error {
                    ui.label(RichText::new(icons::clip(err, 120)).size(11.0).color(theme::RED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("刷新").clicked() && !s.loading {
                        actions.push(TabAction::Reload { tab_id: tab_id.to_string() });
                    }
                    if let Some(d) = &s.data {
                        if ui.button("复制 DDL").clicked() {
                            ui.ctx().copy_text(d.ddl.clone());
                        }
                    }
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(data) = &s.data else {
            ui.centered_and_justified(|ui| {
                let msg = if s.loading {
                    "加载中…".to_string()
                } else {
                    s.error.clone().unwrap_or_else(|| "无数据".into())
                };
                ui.label(RichText::new(msg).color(theme::MUTED));
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
                        .color(theme::TEXT2),
                );
                ui.add_space(2.0);
                egui::Grid::new("struct-cols")
                    .striped(true)
                    .num_columns(6)
                    .spacing([14.0, 5.0])
                    .show(ui, |ui| {
                        // 表头
                        for h in ["名称", "类型", "可空", "键", "默认值", "注释"] {
                            ui.label(RichText::new(h).size(11.5).strong().color(theme::MUTED));
                        }
                        ui.end_row();
                        for c in &data.columns {
                            let is_pk = c.key == "PRI";
                            ui.label(
                                RichText::new(&c.name)
                                    .size(12.5)
                                    .monospace()
                                    .color(if is_pk { theme::ACCENT } else { theme::TEXT }),
                            );
                            ui.label(RichText::new(&c.data_type).size(12.5).monospace().color(theme::TEXT2));
                            ui.label(
                                RichText::new(if c.nullable { "YES" } else { "NO" })
                                    .size(12.0)
                                    .color(if c.nullable { theme::MUTED } else { theme::TEXT2 }),
                            );
                            if is_pk {
                                icons::key_badge(ui);
                            } else {
                                ui.label(RichText::new(c.key.clone()).size(12.0).color(theme::MUTED));
                            }
                            ui.label(
                                RichText::new(c.default.clone().unwrap_or_else(|| "NULL".into()))
                                    .size(12.0)
                                    .monospace()
                                    .color(theme::TEXT2),
                            );
                            ui.label(RichText::new(&c.comment).size(12.0).color(theme::MUTED));
                            ui.end_row();
                        }
                    });

                ui.add_space(14.0);
                ui.label(
                    RichText::new(format!("索引 ({})", data.indexes.len()))
                        .size(12.5)
                        .strong()
                        .color(theme::TEXT2),
                );
                ui.add_space(2.0);
                for ix in &data.indexes {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&ix.name).size(12.5).monospace().color(theme::TEXT));
                        ui.label(RichText::new(&ix.columns).size(12.0).color(theme::MUTED));
                        if ix.unique {
                            icons::chip(ui, "UNIQUE", theme::ACCENT);
                        }
                    });
                }

                ui.add_space(14.0);
                ui.label(RichText::new("DDL (建表语句)").size(12.5).strong().color(theme::TEXT2));
                ui.add_space(4.0);
                egui::Frame::default()
                    .fill(theme::PANEL)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("ddl-scroll")
                            .max_height(340.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(data.ddl.clone()).size(12.0).monospace().color(theme::TEXT2));
                            });
                    });
            });
    });

    actions
}
