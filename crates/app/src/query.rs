//! 查询标签:SQL 编辑 + 执行 + 多结果集。

use crate::app::QueryTab;
use crate::table_view::TabAction;
use crate::{grid, icons, theme};
use egui::RichText;
use shujing_core::ConnInfo;

pub fn draw_query_view(
    conns: &[ConnInfo],
    ctx: &egui::Context,
    tab_id: &str,
    q: &mut QueryTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();

    egui::TopBottomPanel::top("query_toolbar")
        .frame(
            egui::Frame::new()
                .fill(theme::c().panel)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("连接").size(12.0).color(theme::c().muted));
                let selected = conns
                    .iter()
                    .find(|c| c.id == q.conn_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "选择连接".into());
                egui::ComboBox::from_id_salt("query-conn")
                    .selected_text(RichText::new(selected).size(12.5).color(theme::c().text))
                    .width(150.0)
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
                        egui::Button::new(RichText::new("运行 ⌘回车").color(theme::c().bg).strong())
                            .fill(theme::c().accent)
                            .corner_radius(egui::CornerRadius::same(9)),
                    )
                    .clicked()
                {
                    actions.push(TabAction::RunQuery { tab_id: tab_id.to_string() });
                }
                if let Some(err) = &q.error {
                    ui.label(RichText::new(icons::clip(err, 140)).size(12.0).color(theme::c().red));
                }
                let has_grid = q
                    .results
                    .get(q.active_set)
                    .map(|r| !r.columns.is_empty())
                    .unwrap_or(false);
                if has_grid && crate::theme::text_button(ui, "导出 CSV") {
                    actions.push(TabAction::ExportQueryCsv { tab_id: tab_id.to_string() });
                }
                let history = q.history.clone();
                let hist_menu = ui.menu_button(
                    RichText::new("历史").size(13.0).color(theme::c().accent),
                    |ui| {
                    if history.is_empty() {
                        ui.label(RichText::new("暂无记录").size(12.0).color(theme::c().muted));
                    }
                    for (i, h) in history.iter().enumerate() {
                        let preview: String = h.chars().take(48).collect();
                        if ui
                            .button(RichText::new(format!("{}. {}", i + 1, preview)).size(12.0))
                            .on_hover_text(h)
                            .clicked()
                        {
                            q.sql = h.clone();
                            ui.close_menu();
                        }
                    }
                });
                // 下拉小三角(painter 直绘,▾ 缺字形)
                let r = hist_menu.response.rect;
                let c0 = egui::pos2(r.right() - 8.0, r.center().y + 1.0);
                let s = 3.0;
                let pts = vec![
                    egui::pos2(c0.x - s, c0.y - s / 2.0),
                    egui::pos2(c0.x + s, c0.y - s / 2.0),
                    egui::pos2(c0.x, c0.y + s),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    pts,
                    theme::c().accent,
                    egui::Stroke::NONE,
                ));
            });
            crate::theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        // SQL 编辑区(有结果时压缩到上部 1/3)
        let editor_h = if q.results.is_empty() {
            (ui.available_height() - 8.0).max(160.0)
        } else {
            (ui.available_height() * 0.32).clamp(110.0, 260.0)
        };
        ui.allocate_ui(egui::vec2(ui.available_width(), editor_h), |ui| {
            let mut layouter = |ui: &egui::Ui, sql: &str, wrap_width: f32| {
                let mut job = crate::sql_highlight::highlight(sql);
                job.wrap.max_width = wrap_width;
                ui.fonts(|f| f.layout_job(job))
            };
            ui.add(
                egui::TextEdit::multiline(&mut q.sql)
                    .code_editor()
                    .hint_text(RichText::new("输入 SQL…   ⌘回车 运行").color(theme::c().muted))
                    .desired_width(f32::INFINITY)
                    .desired_rows(6)
                    .layouter(&mut layouter),
            );
        });

        // ⌘回车 运行
        if ui.input(|i| {
            i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
        }) {
            actions.push(TabAction::RunQuery { tab_id: tab_id.to_string() });
        }

        // 结果集(下)
        if !q.results.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if q.results.len() > 1 {
                    ui.label(
                        RichText::new(format!(
                            "结果集 {}/{}",
                            q.active_set + 1,
                            q.results.len()
                        ))
                        .size(12.0)
                        .color(theme::c().muted),
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
                            .size(12.0)
                            .color(theme::c().green),
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} 行{}",
                            r.rows.len(),
                            if r.truncated { "(已截断)" } else { "" }
                        ))
                        .size(12.0)
                        .color(theme::c().muted),
                    );
                }
            });
            ui.add_space(2.0);
            if let Some(r) = q.results.get(q.active_set) {
                if !r.columns.is_empty() {
                    grid::result_rows(ui, "query-results", &r.columns, &r.rows);
                }
            }
        }
    });

    actions
}
