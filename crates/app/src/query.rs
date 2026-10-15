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
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("连接").size(12.0).color(theme::MUTED));
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
                        egui::Button::new(RichText::new("运行 ⌘↵").color(theme::BG).strong())
                            .fill(theme::ACCENT)
                            .corner_radius(egui::CornerRadius::same(9)),
                    )
                    .clicked()
                {
                    actions.push(TabAction::RunQuery { tab_id: tab_id.to_string() });
                }
                if let Some(err) = &q.error {
                    ui.label(RichText::new(icons::clip(err, 140)).size(12.0).color(theme::RED));
                }
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
                    .desired_width(f32::INFINITY)
                    .desired_rows(6)
                    .layouter(&mut layouter),
            );
        });

        // ⌘↵ 运行
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
                        .color(theme::MUTED),
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
                            .color(theme::GREEN),
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} 行{}",
                            r.rows.len(),
                            if r.truncated { "(已截断)" } else { "" }
                        ))
                        .size(12.0)
                        .color(theme::MUTED),
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
