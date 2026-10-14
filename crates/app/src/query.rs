//! 查询标签:SQL 编辑 + 执行 + 多结果集。

use crate::app::QueryTab;
use crate::table_view::TabAction;
use egui::RichText;
use shujing_core::ConnInfo;

pub enum QueryAction {
    Run { tab_id: String },
    Close { tab_id: String },
}

pub fn draw_query_view(
    conns: &[ConnInfo],
    ctx: &egui::Context,
    q: &mut QueryTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id = format!("query:{}", q.conn_id);

    egui::TopBottomPanel::top("query_toolbar")
        .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("连接");
                let selected = conns
                    .iter()
                    .find(|c| c.id == q.conn_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "选择连接".into());
                egui::ComboBox::from_id_salt("query-conn")
                    .selected_text(selected)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for c in conns {
                            ui.selectable_value(&mut q.conn_id, c.id.clone(), c.name.clone());
                        }
                    });
                if q.running {
                    ui.spinner();
                }
                if ui
                    .add_enabled(
                        !q.running && !q.sql.trim().is_empty() && !q.conn_id.is_empty(),
                        egui::Button::new("▶ 运行 (⌘↵)"),
                    )
                    .clicked()
                {
                    actions.push(TabAction::RunQuery { tab_id: tab_id.clone() });
                }
                if let Some(err) = &q.error {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 120, 120),
                        RichText::new(err).size(11.0),
                    );
                }
            });
        });

    egui::CentralPanel::default()
        .show(ctx, |ui| {
            // SQL 编辑区(上)
            let editor_height = if q.results.is_empty() {
                ui.available_height()
            } else {
                (ui.available_height() * 0.35).max(120.0)
            };
            egui::ScrollArea::vertical()
                .id_salt("sql-editor")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut q.sql)
                            .desired_rows(8)
                            .desired_width(ui.available_width())
                            .font(egui::TextStyle::Monospace),
                    );
                });
            ui.add_space(4.0);
            // ⌘↵ 运行
            if ui.input(|i| {
                i.key_pressed(egui::Key::Enter)
                    && (i.modifiers.command || i.modifiers.ctrl)
            }) {
                actions.push(TabAction::RunQuery { tab_id: tab_id.clone() });
            }

            // 结果集(下)
            if !q.results.is_empty() {
                ui.separator();
                ui.horizontal(|ui| {
                    if q.results.len() > 1 {
                        ui.label(
                            RichText::new(format!(
                                "结果集 {}/{}",
                                q.active_set + 1,
                                q.results.len()
                            ))
                            .size(11.0),
                        );
                        if ui.button("‹").clicked() && q.active_set > 0 {
                            q.active_set -= 1;
                        }
                        if ui.button("›").clicked() && q.active_set + 1 < q.results.len() {
                            q.active_set += 1;
                        }
                    }
                    let r = &q.results[q.active_set.min(q.results.len().saturating_sub(1))];
                    if r.columns.is_empty() {
                        ui.label(
                            RichText::new(format!("OK(影响 {} 行)", r.affected))
                                .size(11.0),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "{} 行{}",
                                r.rows.len(),
                                if r.truncated { "(已截断)" } else { "" }
                            ))
                            .size(11.0)
                            .weak(),
                        );
                    }
                });
                let Some(r) = q.results.get(q.active_set) else { return };
                if r.columns.is_empty() {
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("query-results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("query-grid")
                            .striped(true)
                            .num_columns(r.columns.len())
                            .show(ui, |ui| {
                                for c in &r.columns {
                                    ui.label(RichText::new(c).size(12.0).strong());
                                }
                                ui.end_row();
                                for row in &r.rows {
                                    for v in row {
                                        let text = v
                                            .as_deref()
                                            .map(|s| {
                                                if s.len() > 200 {
                                                    format!("{}…", &s[..200])
                                                } else {
                                                    s.to_string()
                                                }
                                            })
                                            .unwrap_or_else(|| "NULL".into());
                                        ui.label(RichText::new(text).size(12.0));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
        });

    actions
}
