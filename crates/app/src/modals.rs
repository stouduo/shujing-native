//! 功能弹窗:⌘P 快速打开 / CSV 导入向导 / 表统计 / 新建表。
//! 统一使用 egui::Modal(背景压暗 + 居中)+ theme 控件。

use crate::app::{BgEvent, ShujingApp};
use crate::net;
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
    // "表名 条件" 模式:候选按表名部分过滤(整句 contains 永远匹配不到)
    let filter_word = {
        let r = q.query.trim();
        match r.split_once(' ') {
            Some((a, _)) if !r.to_lowercase().starts_with("select") => a.to_lowercase(),
            _ => q.query.trim().to_lowercase(),
        }
    };
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
                if t.kind == "table"
                    && (filter_word.is_empty() || t.name.to_lowercase().contains(&filter_word))
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
    let mut picked_cond: Option<String> = None;
    let mut run_sql: Option<String> = None;
    let mut new_conn = false;

    // 解析模式(与 dbclient QuickOpen 一致):
    // ① SQL 语句(select/with/show/desc/describe/explain/pragma 开头)→ 直接执行
    // ② `表名 条件…` → 打开表并套用条件
    // ③ 纯表名 → 过滤/直接打开
    let raw = q.query.trim().to_string();
    let lower = raw.to_lowercase();
    // 词边界:showcase/withdraw/description 这类表名不能被当成 SQL 关键字前缀
    let is_sql = ["select", "with", "show", "desc", "describe", "explain", "pragma"].iter().any(
        |k| {
            lower == *k
                || (lower.starts_with(k)
                    && lower[k.len()..]
                        .chars()
                        .next()
                        .map(|c| c.is_whitespace())
                        .unwrap_or(false))
        },
    );
    let table_cond = if !is_sql && raw.contains(' ') {
        raw.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string()))
    } else {
        None
    };
    if let Some((tname, cond)) = &table_cond {
        if let Some(hit) = candidates.iter().find(|(_, n, _, _)| n == tname) {
            picked = Some((hit.0.clone(), hit.1.clone(), hit.2.clone()));
            picked_cond = Some(cond.clone());
        }
    }
    // enter 在下方 modal 内读取;SQL 直跑的判定挪到 picked 使用处


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
            if is_sql && enter && !raw.is_empty() {
                run_sql = Some(raw.clone());
            }
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
                            if row_resp.clicked() || (is_sel && enter && picked.is_none()) {
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
    if let Some(sql) = run_sql {
        // SQL 直接执行:开查询标签并运行(继承激活标签的连接;
        // 此前把 SQL 文本当连接 id 传入,标签永远"连接未建立")
        let _ = sql;
        app.quick_open = None;
        app.open_query_tab(None);
        let idx = app.tabs.len() - 1;
        if let Some(t) = app.tabs.get_mut(idx) {
            if let crate::app::TabKind::Query(q2) = &mut t.kind {
                q2.sql = raw.clone();
            }
        }
        let id = app.tabs[idx].id.clone();
        app.run_query(&id);
        return;
    }
    if let Some((conn, table, db)) = picked {
        app.quick_open = None;
        // 模式②的条件只对得上首词表:用户点了别的候选行时丢弃条件
        let cond_ok = picked_cond.is_some() && {
            raw.split_once(' ')
                .map(|(a, _)| a == table)
                .unwrap_or(false)
        };
        let picked_cond = if cond_ok { picked_cond } else { None };
        let had = app.tabs.iter().any(|t| {
            matches!(&t.kind, crate::app::TabKind::Table(tt)
                if tt.conn_id == conn && tt.table == table)
        });
        app.open_table(&conn, &table, db);
        // `表名 条件` 模式:套用条件到筛选栏
        if let Some(cond) = picked_cond {
            let mysql = crate::app::mysql_quote(&app.conns, &conn);
            let target = if had {
                app.tabs.iter().position(|t| {
                    matches!(&t.kind, crate::app::TabKind::Table(tt)
                        if tt.conn_id == conn && tt.table == table)
                })
            } else {
                Some(app.active_tab)
            };
            if let Some(i) = target {
                if let Some(t) = app.tabs.get_mut(i) {
                    if let crate::app::TabKind::Table(tt) = &mut t.kind {
                        tt.free_where = cond.clone();
                    }
                }
                let id = app.tabs[i].id.clone();
                app.apply_filter_bar(&id);
            }
        }
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
                theme::modal_title(ui, &format!("导入表格 → {}", im.table));
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
                    let can_import = !im.headers.is_empty() && !im.running;
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
            let parsed = if path.to_lowercase().ends_with(".xlsx") {
                shujing_core::excel::parse_xlsx(&path)
            } else {
                shujing_core::composite::parse_csv(&path)
            };
            match parsed {
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
    // key = "conn/db/table" → 标题带上表名与库
    let title = {
        let parts: Vec<&str> = key.split('/').collect();
        match parts.as_slice() {
            [_, db, tbl] => format!("表统计 · {tbl} @ {db}"),
            _ => "表统计".to_string(),
        }
    };
    let modal = egui::Modal::new(egui::Id::new("stats-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(360.0);
            theme::modal_title(ui, &title);
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
        let mysql = app
            .conns
            .iter()
            .find(|c| c.id == st.conn_id)
            .map(|c| c.db_type == shujing_core::DbType::MySql)
            .unwrap_or(false);
        let full_sql = format!(
            "CREATE TABLE {} (\n{}\n)",
            shujing_core::edits::quote_ident_pub(&st.database, &st.name.trim(), mysql),
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
                    msg: String::new(),
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

/// 全局数据搜索(⌘⇧F):在所选连接全部表的所有列中找关键词
pub fn draw_global_search(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.global_search.is_none() {
        return;
    }
    let p = theme::c();
    let online: Vec<(String, String)> = app
        .trees
        .iter()
        .filter(|(_, t)| t.online)
        .filter_map(|(id, _)| app.conns.iter().find(|c| &c.id == id).map(|c| (c.id.clone(), c.name.clone())))
        .collect();
    // 打开时未选连接:默认第一个在线的
    if app.global_search.as_ref().unwrap().conn_id.is_empty() {
        if let Some((id, _)) = online.first() {
            app.global_search.as_mut().unwrap().conn_id = id.clone();
        }
    }
    // 库列表/展开库先快照(gs 可变借用期间读不了 app.trees)
    let cur_conn = app.global_search.as_ref().unwrap().conn_id.clone();
    let conn_dbs: Vec<String> = app
        .trees
        .get(&cur_conn)
        .map(|t| t.databases.clone())
        .unwrap_or_default();
    let conn_expanded: Option<String> = app
        .trees
        .get(&cur_conn)
        .and_then(|t| t.expanded_db.clone());
    let mut do_search = false;
    let mut goto: Option<(Option<String>, String, String)> = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("global-search"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(660.0);
            theme::modal_title(ui, "全局搜索数据");
            ui.spacing_mut().item_spacing.y = 8.0;
            let gs = app.global_search.as_mut().unwrap();

            ui.horizontal(|ui| {
                // 连接选择
                let sel_name = online
                    .iter()
                    .find(|(id, _)| *id == gs.conn_id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| if online.is_empty() { "无在线连接".into() } else { "选择连接".into() });
                if let Some(cid) = theme::select(ui, "gs-conn", &sel_name, &online, 170.0) {
                    gs.conn_id = cid;
                    gs.db = None;
                    gs.searched = false;
                    gs.hits.clear();
                }
                // 库范围(多库连接):默认侧栏展开的库;换连接后下一帧列表随新连接刷新
                let dbs: Vec<(String, String)> = conn_dbs
                    .iter()
                    .map(|d| (d.clone(), d.clone()))
                    .collect();
                if !dbs.is_empty() {
                    let sel_db = gs
                        .db
                        .clone()
                        .or(conn_expanded.clone())
                        .unwrap_or_else(|| "(连接默认库)".into());
                    if let Some(db) = theme::select(ui, "gs-db", &sel_db, &dbs, 140.0) {
                        gs.db = Some(db);
                        gs.searched = false;
                        gs.hits.clear();
                    }
                }
                ui.add_space(4.0);
                // 关键词
                let resp = ui.add_sized(
                    [ui.available_width() - 70.0, 28.0],
                    egui::TextEdit::singleline(&mut gs.keyword)
                        .hint_text(RichText::new("关键词,回车搜索所有表…").size(12.0).color(p.muted))
                        .font(egui::FontId::proportional(12.5)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_search = true;
                }
            });
            ui.label(
                RichText::new("在所选库的全部表的所有列中查找(逐表扫描,大库可能较慢)")
                    .size(11.0)
                    .color(p.muted),
            );
            ui.add_space(2.0);
            // 结果区
            egui::Frame::new()
                .fill(p.input_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.set_min_height(140.0);
                    if gs.searching {
                        ui.centered_and_justified(|ui| {
                            ui.vertical(|ui| {
                                ui.add_space(30.0);
                                ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                            });
                        });
                    } else if let Some(err) = &gs.error {
                        ui.label(RichText::new(icons::clip(err, 100)).size(12.0).color(p.red));
                    } else if gs.searched {
                        if gs.hits.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText::new("没有命中").size(12.5).color(p.muted));
                            });
                        } else {
                            ui.label(
                                RichText::new(format!("命中 {} 处", gs.hits.len()))
                                    .size(11.5)
                                    .color(p.muted),
                            );
                            let hits = gs.hits.clone();
                            let kw = gs.keyword.trim().to_string();
                            for h in &hits {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 26.0),
                                    egui::Sense::click(),
                                );
                                if resp.hovered() {
                                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), p.row_hover);
                                }
                                let g = icons::text_galley(ui.painter(), h.table.clone(), theme::mono(12.0), Color32::PLACEHOLDER, 130.0);
                                ui.painter().galley(egui::pos2(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, p.accent);
                                let g = icons::text_galley(ui.painter(), h.column.clone(), theme::mono(11.5), Color32::PLACEHOLDER, 130.0);
                                ui.painter().galley(egui::pos2(rect.left() + 148.0, rect.center().y - g.size().y / 2.0), g, p.green);
                                let preview: String = h
                                    .row
                                    .iter()
                                    .map(|v| v.clone().unwrap_or_else(|| "NULL".into()))
                                    .collect::<Vec<_>>()
                                    .join(" | ");
                                let g = icons::text_galley(ui.painter(), preview, theme::mono(11.5), Color32::PLACEHOLDER, rect.width() - 300.0);
                                ui.painter().galley(egui::pos2(rect.left() + 288.0, rect.center().y - g.size().y / 2.0), g, p.text2);
                                if resp.clicked() {
                                    goto = Some((h.database.clone(), h.table.clone(), kw.clone()));
                                }
                            }
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("输入关键词后回车;点击命中行打开对应表并自动筛选")
                                    .size(12.0)
                                    .color(p.muted),
                            );
                        });
                    }
                });
        });
    // 回车即可搜索,弹层外不再补按钮
    let _ = &modal;
    if do_search {
        let gs = app.global_search.as_mut().unwrap();
        let kw = gs.keyword.trim().to_string();
        if !kw.is_empty() && !gs.conn_id.is_empty() {
            if gs.db.is_none() {
                gs.db = app
                    .trees
                    .get(&gs.conn_id)
                    .and_then(|t| t.expanded_db.clone());
            }
            let db = gs.db.clone();
            gs.searching = true;
            gs.error = None;
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            let cid = gs.conn_id.clone();
            net::spawn(async move {
                let result = service
                    .search_all_tables(&cid, db.as_deref(), &kw, 50)
                    .await;
                let _ = tx.send(BgEvent::SearchDone { result });
            });
        }
    }
    if let Some((db, table, kw)) = goto {
        let conn_id = app.global_search.as_ref().unwrap().conn_id.clone();
        app.global_search = None;
        app.open_table(&conn_id, &table, db);
        // 应用关键词筛选
        let mysql = crate::app::mysql_quote(&app.conns, &conn_id);
        if let Some(t) = app.tabs.get_mut(app.active_tab) {
            if let crate::app::TabKind::Table(tt) = &mut t.kind {
                tt.filter = kw.clone();
                tt.active_filter = Some(crate::app::build_like_filter(&tt.columns, &kw, mysql));
            }
        }
        let id = app.tabs[app.active_tab].id.clone();
        app.load_rows(&id);
    } else if close || modal.should_close() {
        app.global_search = None;
    }
}


/// 危险操作确认弹窗(重命名/清空/删除表/复制表)
pub fn draw_confirm_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.confirm.is_none() {
        return;
    }
    let p = theme::c();
    let mut cancel = false;
    let mut proceed = false;
    let modal = egui::Modal::new(egui::Id::new("confirm-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(420.0);
            let c = app.confirm.as_ref().unwrap();
            theme::modal_title(ui, &c.title);
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(RichText::new(&c.message).size(12.5).color(p.text));
            let need_input = c.expect.is_some();
            let c = app.confirm.as_mut().unwrap();
            if need_input {
                ui.add_space(2.0);
                let resp = ui.add_sized(
                    [420.0, 28.0],
                    egui::TextEdit::singleline(&mut c.input)
                        .hint_text(RichText::new("输入名称以确认").size(12.0).color(p.muted))
                        .font(theme::mono(12.5)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    proceed = true;
                }
            }
            theme::modal_footer(ui, |ui| {
                // {v} 占位需要用户输入:留空时禁止确认(否则把字面 {v} 发给数据库)
                let need_input = c.sql.contains("{v}") && c.input.trim().is_empty();
                let kind = if need_input {
                    theme::BtnKind::Secondary
                } else {
                    theme::BtnKind::Primary
                };
                if theme::button(ui, "确认", kind).clicked() && !need_input {
                    proceed = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
    if proceed {
        let c = app.confirm.as_ref().unwrap();
        let ok = match &c.expect {
            Some(expect) => c.input.trim() == expect.as_str(),
            None => true,
        };
        if ok {
            let action = c.action.clone();
            let input_ok = c.input.trim().to_string();
            if let Some(conn_id) = action.as_deref().and_then(|a| a.strip_prefix("delete-conn:")) {
                app.confirm = None;
                app.delete_connection(conn_id);
                app.toast_error(format!("已删除连接 {}", input_ok));
            } else if let Some(rest) =
                action.as_deref().and_then(|a| a.strip_prefix("delete-rows:"))
            {
                app.confirm = None;
                // 格式 delete-rows:<tab_id>:<gen>:<ris csv>;gen 不符说明弹窗期间数据已刷新
                let mut it = rest.splitn(3, ':');
                let tid = it.next().unwrap_or("");
                let gen = it.next().and_then(|g| g.parse::<u64>().ok()).unwrap_or(u64::MAX);
                let ris_csv = it.next().unwrap_or("");
                let ris: Vec<usize> =
                    ris_csv.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if !tid.is_empty() && !ris.is_empty() {
                    crate::table_view::run_delete_rows(app, tid, gen, &ris);
                }
            } else if let Some(conn_id) =
                action.as_deref().and_then(|a| a.strip_prefix("disconnect-conn:"))
            {
                app.confirm = None;
                app.trees.remove(conn_id);
                app.close_tabs_for_connection(conn_id);
                let service = app.service.clone();
                let cid = conn_id.to_string();
                net::spawn(async move {
                    let _ = service.disconnect(&cid).await;
                });
                app.toast_error("已断开连接".to_string());
            } else {
                let conn_id = c.conn_id.clone();
                let mysql = crate::app::mysql_quote(&app.conns, &conn_id);
                let sql = c.resolved_sql(mysql);
                let ddl_msg = c.title.clone();
                app.confirm = None;
                let tx = app.event_tx.clone();
                let service = app.service.clone();
                net::spawn(async move {
                    let result = service
                        .run_sql(&conn_id, sql, Some(1))
                        .await
                        .map(|_| ());
                    let _ = tx.send(BgEvent::DdlDone {
                        result,
                        msg: ddl_msg,
                    });
                });
            }
        }
    } else if cancel || modal.should_close() {
        app.confirm = None;
    }
}


/// 列统计弹窗(当前页)
pub fn draw_col_stats_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    // 只在有统计数据的激活表标签上显示
    let Some(tab) = app.tabs.get(app.active_tab) else { return };
    let Some((col, rows)) = tab
        .table_data()
        .and_then(|t| t.col_stats.clone())
    else {
        return;
    };
    let p = theme::c();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("col-stats"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(360.0);
            theme::modal_title(ui, &format!("列统计 · {col}"));
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new("基于当前页数据").size(11.0).color(p.muted));
            ui.add_space(4.0);
            for (k, v) in &rows {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).size(12.0).color(p.muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(v)
                                .font(theme::mono(12.5))
                                .color(p.text),
                        );
                    });
                });
                theme::hairline_bottom(ui);
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "关闭", theme::BtnKind::Primary).clicked() {
                    close = true;
                }
            });
        });
    if close || modal.should_close() {
        if let Some(t) = app.tabs.get_mut(app.active_tab) {
            if let Some(tt) = t.table_data_mut() {
                tt.col_stats = None;
            }
        }
    }
}


/// 外观设置弹窗(强调色)
pub fn draw_settings_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("settings-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(360.0);
            theme::modal_title(ui, "外观设置");
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(RichText::new("强调色").size(11.0).color(p.muted));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (i, name) in theme::ACCENTS.iter().enumerate() {
                    let sw = egui::Rect::from_center_size(
                        egui::pos2(ui.cursor().left() + 13.0, ui.cursor().center().y),
                        egui::vec2(26.0, 26.0),
                    );
                    let _ = ui.allocate_space(egui::vec2(26.0, 26.0));
                    let resp =
                        ui.interact(sw, egui::Id::new(("accent", i)), Sense::click());
                    let c2 = if theme::is_dark() {
                        theme::accent_color_dark(i)
                    } else {
                        theme::accent_color_light(i)
                    };
                    ui.painter().rect_filled(sw, egui::CornerRadius::same(13), c2);
                    if theme::accent_idx() == i {
                        ui.painter().rect_stroke(
                            sw.expand(2.0),
                            egui::CornerRadius::same(15),
                            egui::Stroke::new(1.5, p.text),
                            egui::StrokeKind::Inside,
                        );
                    }
                    resp.clone().on_hover_text(*name);
                    if resp.clicked() {
                        theme::set_accent(i);
                        app.save_theme_settings();
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("主题").size(11.0).color(p.muted));
            ui.horizontal(|ui| {
                // 跟随系统开关(手动点 ☀/☾ 会自动解除跟随)
                let on = app.follow_system_theme;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(38.0, 22.0), Sense::click());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(11),
                    if on { p.accent } else { p.input_bg },
                );
                let knob_x = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
                ui.painter().circle_filled(
                    egui::pos2(knob_x, rect.center().y),
                    8.0,
                    p.text,
                );
                resp.clone().on_hover_text("系统切深浅色时应用自动跟随");
                if resp.clicked() {
                    app.follow_system_theme = !app.follow_system_theme;
                    if app.follow_system_theme {
                        // 打开即对齐一次当前系统外观
                        let dark = crate::system_prefers_dark();
                        app.set_theme_dark_inner(ui.ctx(), dark);
                    }
                    app.save_theme_settings();
                }
                ui.label(
                    RichText::new(if on { "跟随系统外观" } else { "手动(侧栏 ☀/☾ 切换)" })
                        .size(12.0)
                        .color(p.text2),
                );
            });
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "完成", theme::BtnKind::Primary).clicked() {
                    close = true;
                }
            });
        });
    if close || modal.should_close() {
        app.settings_open = false;
    }
}


/// 库筛选弹窗:勾选该连接在侧栏显示哪些库(持久化 db_filters.json)
pub fn draw_db_filter_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.db_filter.is_none() {
        return;
    }
    let p = theme::c();
    let mut apply: Option<Vec<String>> = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("db-filter"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(380.0);
            theme::modal_title(ui, "筛选显示的库");
            ui.spacing_mut().item_spacing.y = 8.0;
            let all_count = app.db_filter.as_ref().map(|f| f.all.len()).unwrap_or(0);
            // 搜索 + 全选/全不选
            ui.horizontal(|ui| {
                let st = app.db_filter.as_mut().unwrap();
                theme::search_box(ui, "db-filter-search", &mut st.search, "搜索库", 200.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("全不选").clicked() {
                        let kw = st.search.trim().to_lowercase();
                        if kw.is_empty() {
                            st.selected.clear();
                        } else {
                            st.selected.retain(|s| !s.to_lowercase().contains(&kw));
                        }
                    }
                    if ui.small_button("全选").clicked() {
                        let kw = st.search.trim().to_lowercase();
                        for d in &st.all {
                            if kw.is_empty() || d.to_lowercase().contains(&kw) {
                                if !st.selected.contains(d) {
                                    st.selected.push(d.clone());
                                }
                            }
                        }
                    }
                });
            });
            ui.label(
                RichText::new(format!(
                    "已选 {}/{} · 未勾选的库不在侧栏显示",
                    app.db_filter.as_ref().map(|f| f.selected.len()).unwrap_or(0),
                    all_count
                ))
                .size(11.0)
                .color(p.muted),
            );
            // 复选列表
            egui::Frame::new()
                .fill(p.input_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("db-filter-list")
                        .max_height(300.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let st = app.db_filter.as_mut().unwrap();
                            let kw = st.search.trim().to_lowercase();
                            let all = st.all.clone();
                            let mut any = false;
                            for d in &all {
                                if !kw.is_empty() && !d.to_lowercase().contains(&kw) {
                                    continue;
                                }
                                any = true;
                                let mut checked = st.selected.contains(d);
                                if ui.checkbox(&mut checked, d).changed() {
                                    if checked {
                                        if !st.selected.contains(d) {
                                            st.selected.push(d.clone());
                                        }
                                    } else {
                                        st.selected.retain(|s| s != d);
                                    }
                                }
                            }
                            if !any {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("无匹配库").size(12.0).color(p.muted),
                                    );
                                });
                            }
                        });
                });
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "完成", theme::BtnKind::Primary).clicked() {
                    let st = app.db_filter.as_ref().unwrap();
                    apply = Some(st.selected.clone());
                }
                ui.add_space(6.0);
                if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if let Some(selected) = apply {
        let conn_id = app.db_filter.as_ref().unwrap().conn_id.clone();
        app.db_filter = None;
        app.apply_db_filter(&conn_id, selected);
    } else if close || modal.should_close() {
        app.db_filter = None;
    }
}
