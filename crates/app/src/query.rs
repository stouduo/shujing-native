//! 查询标签:SQL 编辑 + 执行 + 多结果集。

use crate::app::QueryTab;
use crate::table_view::TabAction;
use crate::{grid, theme};
use egui::{Align, Layout, RichText};
use shujing_core::ConnInfo;

pub fn draw_query_view(
    conns: &[ConnInfo],
    ctx: &egui::Context,
    tab_id: &str,
    q: &mut QueryTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top("query_toolbar")
        .show_separator_line(false)
        .exact_height(theme::TOOLBAR_H)
        .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let opts: Vec<(String, String)> = conns
                    .iter()
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect();
                let sel_name = conns
                    .iter()
                    .find(|c| c.id == q.conn_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "选择连接".into());
                ui.label(RichText::new("连接").size(theme::FS_AUX).color(p.muted));
                if let Some(new_id) =
                    theme::select(ui, "query-conn-select", &sel_name, &opts, 180.0)
                {
                    q.conn_id = new_id;
                }
                if q.running {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                    ui.label(RichText::new("执行中…").size(theme::FS_AUX).color(p.muted));
                }
                if let Some(err) = &q.error {
                    ui.label(
                        RichText::new(crate::icons::clip(err, 90))
                            .size(theme::FS_AUX)
                            .color(p.red),
                    );
                }
                let is_redis = conns
                    .iter()
                    .find(|c| c.id == q.conn_id)
                    .map(|c| c.db_type == shujing_core::DbType::Redis)
                    .unwrap_or(false);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let runnable = !q.running && !q.sql.trim().is_empty() && !q.conn_id.is_empty();
                    let run = theme::button_shortcut(
                        ui,
                        if is_redis { "执行命令" } else { "运行" },
                        "⌘↵",
                        if runnable {
                            theme::BtnKind::Primary
                        } else {
                            theme::BtnKind::Secondary
                        },
                    );
                    run.clone().on_hover_text(if is_redis {
                        "执行 Redis 命令 (⌘↵)"
                    } else {
                        "运行 SQL (⌘↵)"
                    });
                    if runnable && run.clicked() {
                        actions.push(if is_redis {
                            TabAction::RunRedisCmd {
                                tab_id: tab_id.to_string(),
                            }
                        } else {
                            TabAction::RunQuery {
                                tab_id: tab_id.to_string(),
                            }
                        });
                    }
                    let has_grid = q
                        .results
                        .get(q.active_set)
                        .map(|r| !r.columns.is_empty())
                        .unwrap_or(false);
                    if has_grid
                        && theme::button(ui, "导出 CSV", theme::BtnKind::Secondary).clicked()
                    {
                        actions.push(TabAction::ExportQueryCsv {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    let history = q.history.clone();
                    let hist =
                        ui.menu_button(RichText::new("历史").size(12.5).color(p.text2), |ui| {
                            ui.set_min_width(280.0);
                            if history.is_empty() {
                                ui.label(RichText::new("暂无记录").size(12.0).color(p.muted));
                            }
                            for (i, h) in history.iter().enumerate() {
                                let preview: String = h.chars().take(52).collect();
                                if ui
                                    .button(
                                        RichText::new(format!("{}. {}", i + 1, preview))
                                            .font(theme::mono(11.5)),
                                    )
                                    .on_hover_text(h)
                                    .clicked()
                                {
                                    q.sql = h.clone();
                                    ui.close_menu();
                                }
                            }
                        });
                    hist.response.on_hover_text("执行历史");
                });
            });
            theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(12, 12)),
        )
        .show(ctx, |ui| {
            let is_redis = conns
                .iter()
                .find(|c| c.id == q.conn_id)
                .map(|c| c.db_type == shujing_core::DbType::Redis)
                .unwrap_or(false);
            // 编辑器卡片
            let has_results = !q.results.is_empty();
            let editor_h = if has_results {
                (ui.available_height() * 0.32).clamp(110.0, 260.0)
            } else {
                (ui.available_height() - 4.0).max(180.0)
            };
            // 编辑器属于内容层:用背景色而不是卡片色,避免整块内容区变成灰色
            egui::Frame::new()
                .fill(p.bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_height(editor_h);
                    ui.set_max_height(editor_h);
                    egui::ScrollArea::vertical()
                        .id_salt(format!("sql-editor-scroll-{tab_id}"))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut layouter = |ui: &egui::Ui, sql: &str, wrap_width: f32| {
                                let mut job = crate::sql_highlight::highlight(sql);
                                job.wrap.max_width = wrap_width;
                                ui.fonts(|f| f.layout_job(job))
                            };
                            ui.add(
                                egui::TextEdit::multiline(&mut q.sql)
                                    .code_editor()
                                    .frame(false)
                                    .margin(egui::Margin::ZERO)
                                    .hint_text(
                                        RichText::new(if is_redis {
                                            "GET key    ⌘↵ 执行命令"
                                        } else {
                                            "SELECT …    ⌘↵ 运行"
                                        })
                                        .font(theme::mono(12.0))
                                        .color(p.muted),
                                    )
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(5)
                                    .layouter(&mut layouter),
                            );
                        });
                });

            if ui.input(|i| {
                i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
            }) {
                actions.push(if is_redis {
                    TabAction::RunRedisCmd {
                        tab_id: tab_id.to_string(),
                    }
                } else {
                    TabAction::RunQuery {
                        tab_id: tab_id.to_string(),
                    }
                });
            }

            if q.results.is_empty() {
                return;
            }
            ui.add_space(10.0);

            // 结果集元信息 + 切换
            let r = &q.results[q.active_set.min(q.results.len().saturating_sub(1))];
            ui.horizontal(|ui| {
                if q.results.len() > 1 {
                    let mut go: Option<usize> = None;
                    for i in 0..q.results.len() {
                        let label = format!("结果 {}", i + 1);
                        let selected = i == q.active_set;
                        let btn =
                            egui::Button::new(RichText::new(label).size(11.5).color(if selected {
                                p.text
                            } else {
                                p.text2
                            }))
                            .fill(if selected {
                                theme::tint(p.accent, 48)
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(0.0, 24.0));
                        if ui.add(btn).clicked() {
                            go = Some(i);
                        }
                    }
                    if let Some(i) = go {
                        q.active_set = i;
                    }
                    ui.add_space(6.0);
                }
                if r.columns.is_empty() {
                    ui.label(
                        RichText::new(format!("OK · 影响 {} 行", r.affected))
                            .size(11.5)
                            .color(p.green),
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} 行{} · {} ms",
                            r.rows.len(),
                            if r.truncated { " · 已截断" } else { "" },
                            r.elapsed_ms
                        ))
                        .size(11.5)
                        .color(p.text2),
                    );
                }
            });
            ui.add_space(6.0);
            if !r.columns.is_empty() {
                grid::result_rows(ui, &format!("query-results-{tab_id}"), &r.columns, &r.rows);
            }
        });

    actions
}
