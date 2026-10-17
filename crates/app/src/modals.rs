//! 功能弹窗:⌘P 快速打开 / CSV 导入向导 / 表统计 / 新建表。
//! 统一使用 egui::Modal(背景压暗 + 居中)+ theme 控件。

use crate::app::{NewTableState, ShujingApp};
use crate::icons;
use crate::theme;
use egui::{Color32, RichText, Sense};
use std::cell::Cell;

/// ⌘P 快速打开:过滤当前在线连接的表,回车打开数据
pub fn draw_quick_open(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some(mut q) = app.quick_open.take() else {
        return;
    };
    let p = theme::c();

    // 候选:所有在线树的表(排序稳定,避免 HashMap 迭代乱序)
    let query = q.query.to_lowercase();
    let mut candidates: Vec<(String, String, Option<String>, String)> = Vec::new();
    for (cid, tree) in &app.trees {
        if !tree.online {
            continue;
        }
        let conn_name = app
            .conns
            .iter()
            .find(|c| &c.id == cid)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| cid.clone());
        for (db, tbls) in &tree.tables_by_db {
            for t in tbls {
                if t.kind == "table" && (query.is_empty() || t.name.to_lowercase().contains(&query))
                {
                    candidates.push((
                        cid.clone(),
                        t.name.clone(),
                        Some(db.clone()),
                        conn_name.clone(),
                    ));
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    candidates.truncate(14);
    if !candidates.is_empty() {
        q.sel = q.sel.min(candidates.len() - 1);
    } else {
        q.sel = 0;
    }

    let mut picked: Option<(String, String, Option<String>)> = None;
    let mut new_conn = false;

    let modal = egui::Modal::new(egui::Id::new("quick-open-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(520.0);
            theme::modal_title(ui, "快速打开表");
            let resp = theme::search_box(
                ui,
                "quick-open-search",
                &mut q.query,
                "输入表名过滤…",
                520.0,
            );
            resp.request_focus();
            ui.add_space(10.0);

            let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
            let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !candidates.is_empty() {
                if down {
                    q.sel = (q.sel + 1).min(candidates.len() - 1);
                }
                if up {
                    q.sel = q.sel.saturating_sub(1);
                }
            }

            if candidates.is_empty() {
                let any_online = app.trees.values().any(|t| t.online);
                ui.add_space(6.0);
                if any_online {
                    ui.label(RichText::new("无匹配表").size(12.0).color(p.muted));
                } else {
                    ui.label(
                        RichText::new("暂无在线连接,先连接一个数据库")
                            .size(12.0)
                            .color(p.muted),
                    );
                    ui.add_space(8.0);
                    if theme::button(ui, "新建连接", theme::BtnKind::Secondary).clicked() {
                        new_conn = true;
                    }
                }
            } else {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, (cid, name, db, conn_name)) in candidates.iter().enumerate() {
                            let is_sel = i == q.sel;
                            let (rect, row_resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 32.0),
                                Sense::click(),
                            );
                            if is_sel {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(6),
                                    theme::tint(p.accent, 48),
                                );
                            } else if row_resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(6),
                                    p.row_hover,
                                );
                            }
                            icons::paint_table(
                                ui.painter(),
                                egui::Rect::from_center_size(
                                    egui::pos2(rect.left() + 16.0, rect.center().y),
                                    egui::vec2(14.0, 14.0),
                                ),
                                if is_sel { p.accent } else { p.text2 },
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + 32.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                name,
                                egui::FontId::proportional(13.0),
                                if is_sel { p.text } else { p.cell_text },
                            );
                            let loc = format!("{conn_name} / {}", db.clone().unwrap_or_default());
                            let g = ui.painter().layout_no_wrap(
                                loc,
                                egui::FontId::proportional(11.0),
                                Color32::PLACEHOLDER,
                            );
                            ui.painter().galley(
                                egui::pos2(
                                    rect.right() - g.size().x - 10.0,
                                    rect.center().y - g.size().y / 2.0,
                                ),
                                g,
                                p.muted,
                            );
                            if row_resp.clicked() || (is_sel && enter) {
                                picked = Some((cid.clone(), name.clone(), db.clone()));
                            }
                        }
                    });
                if picked.is_none() && enter {
                    if let Some((cid, name, db, _)) = candidates.get(q.sel) {
                        picked = Some((cid.clone(), name.clone(), db.clone()));
                    }
                }
            }
        });

    if new_conn {
        app.quick_open = None;
        app.conn_modal = Some(crate::app::ConnModal::new());
        return;
    }
    if let Some((conn, table, db)) = picked {
        app.quick_open = None;
        app.open_table(&conn, &table, db);
        return;
    }
    if modal.should_close() {
        app.quick_open = None;
    } else {
        app.quick_open = Some(q);
    }
}

/// CSV 导入向导:选文件列映射到目标表列 → 导入
pub fn draw_import_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.import.is_none() {
        return;
    }
    let p = theme::c();
    let mut parse_now = false;
    let mut do_import = false;
    let mut close = false;
    {
        let Some(im) = app.import.as_mut() else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("import-modal"))
            .frame(theme::modal_frame())
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                theme::modal_title(ui, &format!("导入 CSV → {}", im.table));
                ui.label(
                    RichText::new(format!("文件: {}", icons::clip(&im.path, 62)))
                        .size(11.0)
                        .color(p.muted),
                );
                ui.add_space(12.0);

                if im.headers.is_empty() {
                    ui.label(
                        RichText::new("读取文件后,把每个 CSV 列映射到目标表列")
                            .size(12.0)
                            .color(p.text2),
                    );
                    ui.add_space(10.0);
                    if theme::button(ui, "读取文件预览", theme::BtnKind::Primary).clicked() {
                        parse_now = true;
                    }
                } else {
                    ui.label(
                        RichText::new("列映射(源 CSV 列 → 目标表列;留空跳过该列)")
                            .size(11.0)
                            .color(p.muted),
                    );
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("import-map")
                                .num_columns(3)
                                .spacing([12.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("CSV 列").size(11.0).color(p.muted));
                                    ui.label(RichText::new("→").size(11.0).color(p.muted));
                                    ui.label(RichText::new("目标表列").size(11.0).color(p.muted));
                                    ui.end_row();
                                    for (i, h) in im.headers.clone().into_iter().enumerate() {
                                        ui.label(
                                            RichText::new(h).font(theme::mono(12.0)).color(p.text2),
                                        );
                                        ui.label(RichText::new("→").size(11.0).color(p.muted));
                                        let cur = im.col_map.get(i).cloned().flatten();
                                        let mut opts: Vec<(String, String)> =
                                            vec![("-1".to_string(), "(跳过)".to_string())];
                                        for (ti, name) in im.target_cols.iter().enumerate() {
                                            opts.push((ti.to_string(), name.clone()));
                                        }
                                        let cur_label = cur
                                            .and_then(|ci| im.target_cols.get(ci).cloned())
                                            .unwrap_or_else(|| "(跳过)".to_string());
                                        if let Some(v) = theme::select(
                                            ui,
                                            &format!("imap-{i}"),
                                            &cur_label,
                                            &opts,
                                            200.0,
                                        ) {
                                            while im.col_map.len() <= i {
                                                im.col_map.push(None);
                                            }
                                            im.col_map[i] = v.parse::<usize>().ok();
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }

                if let Some(err) = &im.batch_errors {
                    ui.add_space(8.0);
                    ui.label(RichText::new(icons::clip(err, 80)).size(11.5).color(p.red));
                }

                theme::modal_footer(ui, |ui| {
                    let can_import = !im.headers.is_empty();
                    let kind = if can_import {
                        theme::BtnKind::Primary
                    } else {
                        theme::BtnKind::Secondary
                    };
                    if theme::button(ui, "开始导入", kind).clicked() && can_import {
                        do_import = true;
                    }
                    ui.add_space(6.0);
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            close = true;
        }
    }

    if parse_now {
        if let Some(im) = app.import.as_mut() {
            let path = im.path.clone();
            match shujing_core::composite::parse_csv(&path) {
                Ok((headers, _rows)) => {
                    im.col_map = headers
                        .iter()
                        .map(|h| {
                            im.target_cols
                                .iter()
                                .position(|c| c.eq_ignore_ascii_case(h))
                        })
                        .collect();
                    im.headers = headers;
                    im.batch_errors = None;
                }
                Err(e) => im.batch_errors = Some(e),
            }
        }
    }
    if do_import {
        if let Some(im) = app.import.as_mut() {
            im.batch_errors = None;
        }
        app.start_import();
        return;
    }
    if close {
        app.import = None;
    }
}

/// 表统计弹窗
pub fn draw_stats_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some((key, data, err)) = app.stats.clone() else {
        return;
    };
    let p = theme::c();
    let open = Cell::new(true);
    let _ = &key;
    let modal = egui::Modal::new(egui::Id::new("stats-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(360.0);
            theme::modal_title(ui, "表统计");
            match (data, err) {
                (Some((rows, data_len, idx_len)), _) => {
                    egui::Grid::new("stats-grid")
                        .num_columns(2)
                        .spacing([24.0, 10.0])
                        .show(ui, |ui| {
                            stat_row(ui, "行数", &icons::thousands(rows));
                            if let Some(d) = data_len {
                                stat_row(ui, "数据大小", &fmt_bytes(d));
                            }
                            if let Some(i) = idx_len {
                                stat_row(ui, "索引大小", &fmt_bytes(i));
                            }
                        });
                }
                (None, Some(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(p.red));
                }
                (None, None) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(13.0).color(p.muted));
                        ui.label(RichText::new("统计中…").size(12.0).color(p.muted));
                    });
                }
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "关闭", theme::BtnKind::Secondary).clicked() {
                    open.set(false);
                }
            });
        });
    if modal.should_close() {
        open.set(false);
    }
    if !open.get() {
        app.stats = None;
    }
}

fn stat_row(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(RichText::new(k).size(12.0).color(theme::c().muted));
    ui.label(
        RichText::new(v)
            .font(theme::mono(13.0))
            .color(theme::c().cell_text),
    );
    ui.end_row();
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.2} GB", n as f64 / (1 << 30) as f64)
    } else if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / (1 << 10) as f64)
    } else {
        format!("{n} B")
    }
}

/// 新建表弹窗:表名 + 列定义
pub fn draw_new_table_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.new_table.is_none() {
        return;
    }
    let p = theme::c();
    let mut create = false;
    let mut close = false;
    let conn_label = app
        .new_table
        .as_ref()
        .and_then(|n| {
            app.conns
                .iter()
                .find(|c| c.id == n.conn_id)
                .map(|c| c.name.clone())
                .or_else(|| Some(n.conn_id.clone()))
        })
        .unwrap_or_default();
    {
        let Some(st) = app.new_table.as_mut() else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("new-table-modal"))
            .frame(theme::modal_frame())
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                theme::modal_title(
                    ui,
                    &format!(
                        "新建表  {}{}",
                        conn_label,
                        st.database
                            .as_deref()
                            .map(|d| format!(" · {d}"))
                            .unwrap_or_default()
                    ),
                );
                theme::form_field(ui, "表名", "new-table-name", &mut st.name, 520.0, false);
                ui.add_space(10.0);
                ui.label(
                    RichText::new("列定义(CREATE TABLE 内容,不含表名)")
                        .size(11.0)
                        .color(p.muted),
                );
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(p.bg)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut st.sql)
                                .code_editor()
                                .frame(false)
                                .margin(egui::Margin::ZERO)
                                .hint_text(
                                    RichText::new("id BIGINT PRIMARY KEY,\nname VARCHAR(128)")
                                        .font(theme::mono(12.0))
                                        .color(p.muted),
                                )
                                .desired_width(f32::INFINITY)
                                .desired_rows(7),
                        );
                    });
                theme::modal_footer(ui, |ui| {
                    let ok = !st.name.trim().is_empty() && !st.sql.trim().is_empty();
                    if theme::button(
                        ui,
                        "创建",
                        if ok {
                            theme::BtnKind::Primary
                        } else {
                            theme::BtnKind::Secondary
                        },
                    )
                    .clicked()
                        && ok
                    {
                        create = true;
                    }
                    ui.add_space(6.0);
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            close = true;
        }
    }

    if create {
        let Some(st) = app.new_table.as_ref() else {
            return;
        };
        let full_sql = format!(
            "CREATE TABLE {} (\n{}\n)",
            shujing_core::edits::quote_ident_pub(&st.database, &st.name.trim()),
            st.sql.trim().trim_end_matches(';')
        );
        let conn = st.conn_id.clone();
        let info = app.conns.iter().find(|c| c.id == conn).cloned();
        if let Some(info) = info {
            let service = app.service.clone();
            let tx = app.event_tx.clone();
            crate::net::spawn(async move {
                let r = shujing_core::Service::run_sql_standalone(&service, &info, &full_sql).await;
                let _ = tx.send(crate::app::BgEvent::DdlDone {
                    result: r.map(|_| ()),
                });
            });
        }
        return;
    }
    if close {
        app.new_table = None;
    }
}

/// 供侧栏"新建表"入口使用
pub fn open_new_table(app: &mut ShujingApp, conn_id: &str, database: Option<String>) {
    app.new_table = Some(NewTableState {
        conn_id: conn_id.to_string(),
        database,
        name: String::new(),
        sql: String::new(),
    });
}
