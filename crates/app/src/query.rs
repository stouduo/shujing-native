//! 查询标签:SQL 编辑 + 执行 + 多结果集。

use crate::app::QueryTab;
use crate::table_view::TabAction;
use crate::{grid, icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};
use shujing_core::ConnInfo;

pub fn draw_query_view(
    conns: &[ConnInfo],
    ctx: &egui::Context,
    tab_id: &str,
    q: &mut QueryTab,
    snippets: &[String],
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
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        actions.push(TabAction::CancelQuery {
                            tab_id: tab_id.to_string(),
                        });
                    }
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
                    if !is_redis
                        && theme::button(ui, "格式化", theme::BtnKind::Secondary)
                            .on_hover_text("美化 SQL(关键字大写+换行缩进)")
                            .clicked()
                        && !q.sql.trim().is_empty()
                    {
                        q.sql = shujing_core::sql_format::format_sql(&q.sql);
                    }
                    ui.menu_button(RichText::new("片段").size(12.5).color(p.text2), |ui| {
                            ui.set_min_width(300.0);
                            if !is_redis && !q.sql.trim().is_empty() {
                                if ui.button("☆ 保存当前 SQL 为片段").clicked() {
                                    actions.push(TabAction::SnippetSave {
                                        sql: q.sql.clone(),
                                    });
                                    ui.close_menu();
                                }
                                ui.separator();
                            }
                            if snippets.is_empty() {
                                ui.label(RichText::new("暂无片段").size(12.0).color(p.muted));
                            }
                            for (i, snip) in snippets.iter().enumerate() {
                                let preview: String = snip.chars().take(56).collect();
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(RichText::new(preview).font(theme::mono(11.5)))
                                        .on_hover_text(snip)
                                        .clicked()
                                    {
                                        if !q.sql.trim().is_empty() {
                                            q.sql.push('\n');
                                        }
                                        q.sql.push_str(snip);
                                        ui.close_menu();
                                    }
                                    if ui.small_button(RichText::new("删").size(10.0)).clicked() {
                                        actions.push(TabAction::SnippetDelete { idx: i });
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                    if !is_redis
                        && theme::button(ui, "计划", theme::BtnKind::Secondary)
                            .on_hover_text("EXPLAIN 执行计划(结果进结果区,不进历史)")
                            .clicked()
                    {
                        actions.push(TabAction::ExplainQuery {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    if !is_redis
                        && theme::button(ui, "优化", theme::BtnKind::Secondary)
                            .on_hover_text("SQL 优化分析(EXPLAIN + 规则诊断)")
                            .clicked()
                    {
                        q.optimize = Some(super::app::OptimizeState {
                            running: false,
                            ran: false,
                            plan: None,
                            error: None,
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
                                    .id_salt(format!("sql-editor-{tab_id}"))
                                    .code_editor()
                                    .frame(false)
                                    .margin(egui::Margin::ZERO)
                                    .hint_text(
                                        RichText::new(if is_redis {
                                            "GET key    ⌘↵ 执行命令"
                                        } else {
                                            "SELECT / INSERT / UPDATE / DDL …    ⌘↵ 运行"
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

            // ⌘/:选中行注释切换
            if !is_redis
                && ui.input(|i| i.key_pressed(egui::Key::Slash) && i.modifiers.command)
            {
                if let Some(state) =
                    egui::TextEdit::load_state(ui.ctx(), egui::Id::new(format!("sql-editor-{tab_id}")))
                {
                    if let Some(range) = state.cursor.char_range() {
                        let (a, b) = (range.primary.index, range.secondary.index);
                        let (a, b) = (a.min(b), a.max(b));
                        toggle_comment(&mut q.sql, a, b);
                    }
                }
            }
            if ui.input(|i| {
                i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
            }) {
                if is_redis {
                    actions.push(TabAction::RunRedisCmd {
                        tab_id: tab_id.to_string(),
                    });
                } else {
                    // 有选区只执行选中部分
                    let sel_sql = egui::TextEdit::load_state(
                        ui.ctx(),
                        egui::Id::new(format!("sql-editor-{tab_id}")),
                    )
                    .and_then(|st| st.cursor.char_range())
                    .and_then(|range| {
                        let (a, b) = (range.primary.index, range.secondary.index);
                        let (a, b) = (a.min(b), a.max(b));
                        (a < b).then(|| {
                            let start = q.sql.char_indices().nth(a).map(|(i, _)| i).unwrap_or(q.sql.len());
                            let end = q.sql.char_indices().nth(b).map(|(i, _)| i).unwrap_or(q.sql.len());
                            q.sql[start..end].trim().to_string()
                        })
                    })
                    .filter(|s| !s.is_empty());
                    match sel_sql {
                        Some(sel) => actions.push(TabAction::RunQuerySql {
                            tab_id: tab_id.to_string(),
                            sql: sel,
                        }),
                        None => actions.push(TabAction::RunQuery {
                            tab_id: tab_id.to_string(),
                        }),
                    }
                }
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
                // 回写判定:单表 SELECT + 主键齐全 + 非只读连接
                let read_only = conns
                    .iter()
                    .find(|c| c.id == q.conn_id)
                    .map(|c| c.read_only)
                    .unwrap_or(false);
                let editable = !read_only
                    && q.result_table.is_some()
                    && q.pk_checked
                    && !q.result_pk.is_empty()
                    && q.result_pk.iter().all(|p| r.columns.contains(p));
                if editable {
                    if !q.q_changes.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} 处修改(未保存)",
                                    q.q_changes.len()
                                ))
                                .size(11.5)
                                .color(ORANGE),
                            );
                            if theme::button(ui, "保存修改 ⌘S", theme::BtnKind::Primary)
                                .clicked()
                            {
                                actions.push(TabAction::SaveQueryChanges {
                                    tab_id: tab_id.to_string(),
                                });
                            }
                            if theme::button(ui, "放弃", theme::BtnKind::Secondary).clicked() {
                                q.q_changes.clear();
                                q.q_editing = None;
                            }
                        });
                        ui.add_space(4.0);
                    }
                    let (start, commit) = grid::result_rows_editable(
                        ui,
                        &format!("query-results-{tab_id}"),
                        &r.columns,
                        &r.rows,
                        &q.q_changes,
                        &mut q.q_editing,
                        &mut q.q_draft,
                    );
                    if let Some((ri, ci, old)) = start {
                        q.q_editing = Some((ri, ci));
                        q.q_draft = old;
                    }
                    if let Some((ri, ci, val)) = commit {
                        let col = r.columns[ci].clone();
                        let orig = r.rows[ri][ci].clone();
                        let new_v = if val.is_empty() { None } else { Some(val) };
                        if new_v != orig {
                            q.q_changes.retain(|(r2, c2, _)| !(*r2 == ri && c2 == &col));
                            q.q_changes.push((ri, col, new_v));
                        }
                    }
                    // ⌘S 保存(查询标签激活时)
                    if !q.q_changes.is_empty()
                        && ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command)
                    {
                        actions.push(TabAction::SaveQueryChanges {
                            tab_id: tab_id.to_string(),
                        });
                    }
                } else {
                    grid::result_rows(ui, &format!("query-results-{tab_id}"), &r.columns, &r.rows);
                }
            }
        });

    // 优化分析弹窗
    if q.optimize.is_some() {
        let db_type = conns
            .iter()
            .find(|c| c.id == q.conn_id)
            .map(|c| c.db_type)
            .unwrap_or(shujing_core::DbType::MySql);
        actions.extend(draw_optimize_modal(ctx, tab_id, q, db_type, conns));
    }

    actions
}


/// SQL 优化分析弹窗:静态规则 + EXPLAIN 计划诊断
pub fn draw_optimize_modal(
    ctx: &egui::Context,
    tab_id: &str,
    q: &mut super::app::QueryTab,
    db_type: shujing_core::DbType,
    conns: &[ConnInfo],
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let Some(state) = q.optimize.as_ref() else {
        return actions;
    };
    let p = theme::c();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("sql-optimize"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(740.0);
            theme::modal_title(ui, "SQL 优化分析");
            ui.spacing_mut().item_spacing.y = 8.0;

            // SQL 预览
            let sql = q.sql.trim().to_string();
            egui::Frame::new()
                .fill(p.input_bg)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(crate::icons::clip(&sql, 160))
                                .font(theme::mono(11.5))
                                .color(p.text2),
                        )
                        .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                });

            if state.running {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                    ui.label(RichText::new("EXPLAIN 执行中…").size(12.0).color(p.muted));
                });
            } else if let Some(err) = &state.error {
                ui.label(
                    RichText::new(icons::clip(err, 120))
                        .size(12.0)
                        .color(p.red),
                );
            } else if state.ran {
                // 诊断
                let mut findings = shujing_core::sql_advisor::static_check(&sql);
                if let Some(plan) = &state.plan {
                    findings.extend(shujing_core::sql_advisor::plan_check(db_type, plan));
                }
                let danger = findings.iter().filter(|f| f.level == shujing_core::sql_advisor::Level::Danger).count();
                let warn = findings.iter().filter(|f| f.level == shujing_core::sql_advisor::Level::Warn).count();
                let (grade, gcol) = if danger > 0 {
                    ("需优化", p.red)
                } else if warn > 0 {
                    ("可改进", ORANGE)
                } else {
                    ("良好", p.green)
                };
                ui.horizontal(|ui| {
                    let g = ui.painter().layout_no_wrap(grade.to_string(), theme::bold(13.0), Color32::PLACEHOLDER);
                    let gr = egui::Rect::from_center_size(
                        ui.cursor().left_center() + egui::vec2(g.size().x / 2.0 + 10.0, 0.0),
                        egui::vec2(g.size().x + 20.0, 22.0),
                    );
                    ui.allocate_rect(gr, Sense::hover());
                    ui.painter().rect_filled(gr, egui::CornerRadius::same(6), theme::tint(gcol, 36));
                    ui.painter().galley(
                        egui::pos2(gr.center().x - g.size().x / 2.0, gr.center().y - g.size().y / 2.0),
                        g,
                        gcol,
                    );
                    ui.label(
                        RichText::new(format!(
                            "{danger} 项风险 · {warn} 项警告 · {} 项提示",
                            findings.len() - danger - warn
                        ))
                        .size(11.5)
                        .color(p.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let dialect = match db_type {
                            shujing_core::DbType::Sqlite => "sqlite · EXPLAIN QUERY PLAN",
                            shujing_core::DbType::Postgres => "postgres · EXPLAIN (ANALYZE, BUFFERS)",
                            _ => "mysql · EXPLAIN",
                        };
                        ui.label(RichText::new(dialect).size(11.0).color(p.muted));
                    });
                });
                ui.add_space(2.0);
                ui.label(theme::strong("诊断建议", 12.0).color(p.text2));
                for f in &findings {
                    let (badge, col) = match f.level {
                        shujing_core::sql_advisor::Level::Danger => ("危险", p.red),
                        shujing_core::sql_advisor::Level::Warn => ("警告", ORANGE),
                        shujing_core::sql_advisor::Level::Info => ("提示", p.accent),
                    };
                    ui.horizontal(|ui| {
                        let b = ui.painter().layout_no_wrap(badge.to_string(), theme::bold(9.5), Color32::PLACEHOLDER);
                        let br = egui::Rect::from_center_size(
                            ui.cursor().left_center() + egui::vec2(b.size().x / 2.0 + 5.0, 0.0),
                            egui::vec2(b.size().x + 10.0, 16.0),
                        );
                        ui.allocate_rect(br, Sense::hover());
                        ui.painter().rect_filled(br, egui::CornerRadius::same(4), theme::tint(col, 40));
                        ui.painter().galley(
                            egui::pos2(br.center().x - b.size().x / 2.0, br.center().y - b.size().y / 2.0),
                            b,
                            col,
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&f.title).size(12.0).color(p.text));
                            ui.label(
                                RichText::new(&f.detail).size(11.0).color(p.muted),
                            );
                        });
                    });
                }
                // 执行计划网格
                if let Some(plan) = &state.plan {
                    if !plan.columns.is_empty() {
                        ui.add_space(4.0);
                        ui.label(theme::strong("执行计划", 12.0).color(p.text2));
                        egui::ScrollArea::vertical()
                            .id_salt("optimize-plan-grid")
                            .max_height(180.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                super::grid::result_rows(ui, "optimize-plan", &plan.columns, &plan.rows);
                            });
                    }
                }
            }
            ui.horizontal(|ui| {
                let runnable = !state.running && !sql.is_empty();
                if theme::button(
                    ui,
                    if state.running { "分析中…" } else { "运行分析" },
                    if runnable { theme::BtnKind::Primary } else { theme::BtnKind::Secondary },
                )
                .clicked()
                    && runnable
                {
                    actions.push(TabAction::OptimizeQuery {
                        tab_id: tab_id.to_string(),
                    });
                }
                ui.label(
                    RichText::new("静态规则不执行;EXPLAIN 仅读取计划")
                        .size(11.0)
                        .color(p.muted),
                );
            });
        });
    if modal.should_close() {
        close = true;
    }
    if close {
        q.optimize = None;
    }
    actions
}

/// iOS 橙(警告级)
const ORANGE: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x00);


/// 选中行注释切换:已注释的去 `-- `,未注释的加
fn toggle_comment(sql: &mut String, sel_a: usize, sel_b: usize) {
    let char_at = |s: &str, n: usize| s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len());
    let (byte_a, byte_b) = (char_at(sql, sel_a), char_at(sql, sel_b));
    let line_start = sql[..byte_a].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = sql[byte_b..].find('\n').map(|i| byte_b + i).unwrap_or(sql.len());
    let block = &sql[line_start..line_end];
    let all_commented = block
        .lines()
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim_start().starts_with("--"));
    let mut out = String::with_capacity(sql.len() + 16);
    for line in block.lines() {
        if all_commented {
            // 去一层注释
            if let Some(rest) = line.strip_prefix("-- ") {
                out.push_str(rest);
            } else if let Some(rest) = line.strip_prefix("--") {
                out.push_str(rest);
            } else {
                out.push_str(line);
            }
        } else if line.trim().is_empty() {
            out.push_str(line);
        } else {
            out.push_str("-- ");
            out.push_str(line);
        }
        out.push('\n');
    }
    if !block.ends_with('\n') {
        // 最后一行无换行:上面 push 了多余的 \n,去掉
        if out.ends_with('\n') {
            out.pop();
        }
    }
    *sql = format!("{}{}{}", &sql[..line_start], out, &sql[line_end..]);
}
