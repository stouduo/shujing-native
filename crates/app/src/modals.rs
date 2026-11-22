//! 功能弹窗:⌘P 快速打开 / CSV 导入向导 / 表统计 / 新建表。
//! 统一使用 egui::Modal(背景压暗 + 居中)+ theme 控件。
use crate::app::{BgEvent, ShujingApp};

const DIFF_ORANGE: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x00);
use crate::icons;
use crate::net;
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
    // `表名 条件` 的精确匹配要在截断前做(否则同名表落到 14 名外时条件被丢弃)
    let full_candidates = candidates.clone();
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
    let is_sql = [
        "select", "with", "show", "desc", "describe", "explain", "pragma",
    ]
    .iter()
    .any(|k| {
        lower == *k
            || (lower.starts_with(k)
                && lower[k.len()..]
                    .chars()
                    .next()
                    .map(|c| c.is_whitespace())
                    .unwrap_or(false))
    });
    let table_cond = if !is_sql && raw.contains(' ') {
        raw.split_once(' ')
            .map(|(a, b)| (a.to_string(), b.to_string()))
    } else {
        None
    };
    if let Some((tname, cond)) = &table_cond {
        if let Some(hit) = full_candidates.iter().find(|(_, n, _, _)| n == tname) {
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
            theme::modal_title(ui, ui_kit::t!("Quick Open Table", "快速打开表"));
            let resp = theme::search_box(
                ui,
                "quick-open-search",
                &mut q.query,
                ui_kit::t!("Type a table name to filter…", "输入表名过滤…"),
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
                    ui.label(
                        RichText::new(ui_kit::t!("No matching tables", "无匹配表"))
                            .size(12.0)
                            .color(p.muted),
                    );
                } else {
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "No online connections, connect to a database first",
                            "暂无在线连接,先连接一个数据库"
                        ))
                        .size(12.0)
                        .color(p.muted),
                    );
                    ui.add_space(8.0);
                    if theme::button(ui, ui_kit::t!("New Connection", "新建连接"), theme::BtnKind::Secondary)
                        .clicked()
                    {
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
        // 库也要比对:多库连接下同名表会套错库的标签
        let had = app.tabs.iter().any(|t| {
            matches!(&t.kind, crate::app::TabKind::Table(tt)
                    if tt.conn_id == conn && tt.table == table && tt.database == db)
        });
        app.open_table(&conn, &table, db.clone());
        // `表名 条件` 模式:套用条件到筛选栏
        if let Some(cond) = picked_cond {
            let mysql = crate::app::mysql_quote(&app.conns, &conn);
            let target = if had {
                app.tabs.iter().position(|t| {
                    matches!(&t.kind, crate::app::TabKind::Table(tt)
                        if tt.conn_id == conn && tt.table == table && tt.database == db)
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
                theme::modal_title(ui, &format!("{} → {}", ui_kit::t!("Import Data", "导入表格"), im.table));
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        ui_kit::t!("File", "文件"),
                        icons::clip(&im.path, 62)
                    ))
                    .size(11.0)
                    .color(p.muted),
                );
                ui.add_space(12.0);
                if im.headers.is_empty() {
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "Read the file, then map each CSV column to a target table column",
                            "读取文件后,把每个 CSV 列映射到目标表列"
                        ))
                        .size(12.0)
                        .color(p.text2),
                    );
                    ui.add_space(10.0);
                    if theme::button(ui, ui_kit::t!("Read File & Preview", "读取文件预览"), theme::BtnKind::Primary)
                        .clicked()
                    {
                        parse_now = true;
                    }
                } else {
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "Column mapping (source CSV column → target table column; leave blank to skip)",
                            "列映射(源 CSV 列 → 目标表列;留空跳过该列)"
                        ))
                        .size(11.0)
                        .color(p.muted),
                    );
                    ui.add_space(8.0);
                    // 上限按屏幕高度给足(12 组映射 ~380px 在 300 上限里被压小,
                    // 细滚动条几乎不可见=体验上"截断";小窗仍有界)
                    egui::ScrollArea::vertical()
                        .max_height(
                            (ui.ctx().screen_rect().height() * 0.52).clamp(200.0, 560.0),
                        )
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("import-map")
                                .num_columns(3)
                                .spacing([12.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(ui_kit::t!("CSV Column", "CSV 列"))
                                            .size(11.0)
                                            .color(p.muted),
                                    );
                                    ui.label(RichText::new("→").size(11.0).color(p.muted));
                                    ui.label(
                                        RichText::new(ui_kit::t!("Target Column", "目标表列"))
                                            .size(11.0)
                                            .color(p.muted),
                                    );
                                    ui.end_row();
                                    for (i, h) in im.headers.clone().into_iter().enumerate() {
                                        ui.label(
                                            RichText::new(h).font(theme::mono(12.0)).color(p.text2),
                                        );
                                        ui.label(RichText::new("→").size(11.0).color(p.muted));
                                        let cur = im.col_map.get(i).cloned().flatten();
                                        let mut opts: Vec<(String, String)> = vec![(
                                            "-1".to_string(),
                                            ui_kit::t!("(Skip)", "(跳过)").to_string(),
                                        )];
                                        for (ti, name) in im.target_cols.iter().enumerate() {
                                            opts.push((ti.to_string(), name.clone()));
                                        }
                                        let cur_label = cur
                                            .and_then(|ci| im.target_cols.get(ci).cloned())
                                            .unwrap_or_else(|| ui_kit::t!("(Skip)", "(跳过)").to_string());
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
                if im.parsing {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "Parsing… (large files may take a few seconds)",
                            "解析中…(大文件可能需要数秒)"
                        ))
                        .size(11.5)
                        .color(p.muted),
                    );
                }
                if let Some(err) = &im.batch_errors {
                    ui.add_space(8.0);
                    ui.label(RichText::new(icons::clip(err, 80)).size(11.5).color(p.red));
                }
                theme::modal_footer(ui, |ui| {
                    let can_import = !im.headers.is_empty() && !im.running && !im.parsing;
                    let kind = if can_import {
                        theme::BtnKind::Primary
                    } else {
                        theme::BtnKind::Secondary
                    };
                    if theme::button(ui, ui_kit::t!("Start Import", "开始导入"), kind).clicked()
                        && can_import
                    {
                        do_import = true;
                    }
                    ui.add_space(6.0);
                    if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            close = true;
        }
    }
    if parse_now {
        // 预览解析放后台:大文件(数百 MB)同步解析会冻结 UI 数十秒
        if let Some(im) = app.import.as_mut() {
            if !im.parsing {
                im.parsing = true;
                im.batch_errors = None;
                let path = im.path.clone();
                let tx = app.event_tx.clone();
                crate::net::spawn_blocking(move || {
                    let result = if path.to_lowercase().ends_with(".xlsx") {
                        shujing_core::excel::parse_xlsx(&path).map(|(h, _)| h)
                    } else {
                        shujing_core::composite::parse_csv(&path).map(|(h, _)| h)
                    };
                    let _ = tx.send(crate::app::BgEvent::ImportParsed { result });
                });
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
            [_, db, tbl] => format!("{} · {tbl} @ {db}", ui_kit::t!("Table Stats", "表统计")),
            _ => ui_kit::t!("Table Stats", "表统计").to_string(),
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
                            stat_row(ui, ui_kit::t!("Rows", "行数"), &icons::thousands(rows));
                            if let Some(d) = data_len {
                                stat_row(ui, ui_kit::t!("Data Size", "数据大小"), &fmt_bytes(d));
                            }
                            if let Some(i) = idx_len {
                                stat_row(ui, ui_kit::t!("Index Size", "索引大小"), &fmt_bytes(i));
                            }
                        });
                }
                (None, Some(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(p.red));
                }
                (None, None) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(13.0).color(p.muted));
                        ui.label(
                            RichText::new(ui_kit::t!("Computing…", "统计中…"))
                                .size(12.0)
                                .color(p.muted),
                        );
                    });
                }
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
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
                        "{}  {}{}",
                        ui_kit::t!("New Table", "新建表"),
                        conn_label,
                        st.database
                            .as_deref()
                            .map(|d| format!(" · {d}"))
                            .unwrap_or_default()
                    ),
                );
                theme::form_field(
                    ui,
                    ui_kit::t!("Table Name", "表名"),
                    "new-table-name",
                    &mut st.name,
                    520.0,
                    false,
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(ui_kit::t!(
                        "Column definitions (CREATE TABLE body, without the table name)",
                        "列定义(CREATE TABLE 内容,不含表名)"
                    ))
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
                        ui_kit::t!("Create", "创建"),
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
                    if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
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
        .filter_map(|(id, _)| {
            app.conns
                .iter()
                .find(|c| &c.id == id)
                .map(|c| (c.id.clone(), c.name.clone()))
        })
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
    let conn_expanded: Option<String> =
        app.trees.get(&cur_conn).and_then(|t| t.expanded_db.clone());
    let mut do_search = false;
    let mut goto: Option<(Option<String>, String, String)> = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("global-search"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(660.0);
            theme::modal_title(ui, ui_kit::t!("Global Data Search", "全局搜索数据"));
            ui.spacing_mut().item_spacing.y = 8.0;
            let gs = app.global_search.as_mut().unwrap();
            ui.horizontal(|ui| {
                // 连接选择
                let sel_name = online
                    .iter()
                    .find(|(id, _)| *id == gs.conn_id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| {
                        if online.is_empty() {
                            ui_kit::t!("No online connections", "无在线连接").into()
                        } else {
                            ui_kit::t!("Select a connection", "选择连接").into()
                        }
                    });
                if let Some(cid) = theme::select(ui, "gs-conn", &sel_name, &online, 170.0) {
                    gs.conn_id = cid;
                    gs.db = None;
                    gs.searched = false;
                    gs.hits.clear();
                }
                // 库范围(多库连接):默认侧栏展开的库;「全部库」遍历连接全部非系统库
                let all_label = ui_kit::t!("All Databases", "全部库").to_string();
                let mut dbs: Vec<(String, String)> =
                    vec![(all_label.clone(), "__all__".into())];
                dbs.extend(conn_dbs.iter().map(|d| (d.clone(), d.clone())));
                {
                    let sel_db = if gs.all_dbs {
                        all_label.clone()
                    } else {
                        gs.db
                            .clone()
                            .or(conn_expanded.clone())
                            .unwrap_or_else(|| ui_kit::t!("(default db)", "(连接默认库)").into())
                    };
                    if let Some(v) = theme::select(ui, "gs-db", &sel_db, &dbs, 160.0) {
                        if v == "__all__" {
                            gs.all_dbs = true;
                            gs.db = None;
                        } else {
                            gs.all_dbs = false;
                            gs.db = Some(v);
                        }
                        gs.searched = false;
                        gs.hits.clear();
                    }
                }
                ui.add_space(4.0);
                // 关键词
                let resp = ui.add_sized(
                    [ui.available_width() - 70.0, 28.0],
                    egui::TextEdit::singleline(&mut gs.keyword)
                        .hint_text(
                            RichText::new(ui_kit::t!(
                                "Keyword, press Enter to search all tables…",
                                "关键词,回车搜索所有表…"
                            ))
                                .size(12.0)
                                .color(p.muted),
                        )
                        .font(egui::FontId::proportional(12.5)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_search = true;
                }
            });
            ui.label(
                RichText::new(ui_kit::t!(
                    "Search every column of every table in the selected databases (scans table by table, may be slow on large databases)",
                    "在所选库的全部表的所有列中查找(逐表扫描,大库可能较慢)"
                ))
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
                    // 恒定高度(命中再多也不撑高):Modal 居中锚定,结果异步到达
                    // 撑高弹窗会把顶部的搜索条件行整体上移,按压中的点击落空
                    ui.set_min_height(220.0);
                    ui.set_max_height(220.0);
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
                                ui.label(
                                    RichText::new(ui_kit::t!("No matches", "没有命中"))
                                        .size(12.5)
                                        .color(p.muted),
                                );
                            });
                        } else {
                            let hits_label = if ui_kit::i18n::is_english() {
                                format!("Keys Matched: {}", gs.hits.len())
                            } else {
                                format!("命中 {} 处", gs.hits.len())
                            };
                            ui.label(RichText::new(hits_label).size(11.5).color(p.muted));
                            let hits = gs.hits.clone();
                            let kw = gs.keyword.trim().to_string();
                            egui::ScrollArea::vertical()
                                .id_salt("gs-hits")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for h in &hits {
                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), 26.0),
                                            egui::Sense::click(),
                                        );
                                        if resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                egui::CornerRadius::same(6),
                                                p.row_hover,
                                            );
                                        }
                                        let g = icons::text_galley(
                                            ui.painter(),
                                            h.table.clone(),
                                            theme::mono(12.0),
                                            Color32::PLACEHOLDER,
                                            130.0,
                                        );
                                        ui.painter().galley(
                                            egui::pos2(
                                                rect.left() + 8.0,
                                                rect.center().y - g.size().y / 2.0,
                                            ),
                                            g,
                                            p.accent,
                                        );
                                        let g = icons::text_galley(
                                            ui.painter(),
                                            h.column.clone(),
                                            theme::mono(11.5),
                                            Color32::PLACEHOLDER,
                                            130.0,
                                        );
                                        ui.painter().galley(
                                            egui::pos2(
                                                rect.left() + 148.0,
                                                rect.center().y - g.size().y / 2.0,
                                            ),
                                            g,
                                            p.green,
                                        );
                                        let preview: String = h
                                            .row
                                            .iter()
                                            .map(|v| v.clone().unwrap_or_else(|| "NULL".into()))
                                            .collect::<Vec<_>>()
                                            .join(" | ");
                                        let g = icons::text_galley(
                                            ui.painter(),
                                            preview,
                                            theme::mono(11.5),
                                            Color32::PLACEHOLDER,
                                            rect.width() - 300.0,
                                        );
                                        ui.painter().galley(
                                            egui::pos2(
                                                rect.left() + 288.0,
                                                rect.center().y - g.size().y / 2.0,
                                            ),
                                            g,
                                            p.text2,
                                        );
                                        if resp.clicked() {
                                            goto = Some((
                                                h.database.clone(),
                                                h.table.clone(),
                                                kw.clone(),
                                            ));
                                        }
                                    }
                                });
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new(ui_kit::t!(
                                    "Type a keyword and press Enter; click a match to open its table with the filter applied",
                                    "输入关键词后回车;点击命中行打开对应表并自动筛选"
                                ))
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
            let all_dbs = gs.all_dbs;
            // 在途防抖:重复回车不再并发发起(旧结果由身份校验丢弃)
            if gs.searching {
                do_search = false;
            } else {
                gs.searching = true;
                gs.error = None;
                let tx = app.event_tx.clone();
                let service = app.service.clone();
                let cid = gs.conn_id.clone();
                let kw2 = kw.clone();
                net::spawn(async move {
                    let result = if all_dbs {
                        service.search_all_databases(&cid, &kw2, 50).await
                    } else {
                        service.search_all_tables(&cid, db.as_deref(), &kw2, 50).await
                    };
                    let _ = tx.send(BgEvent::SearchDone {
                        conn_id: cid,
                        db,
                        all_dbs,
                        keyword: kw2,
                        result,
                    });
                });
            }
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
                // active_filter 约定存原始关键词:load_rows 会再包一层 build_like_filter,
                // 此前存已构建片段 → 双重构建匹配不到行;未打开的表 columns 空还会拼出 WHERE ()
                tt.active_filter = Some(kw.clone());
                tt.filter = kw.clone();
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
            // 输入框渲染条件:expect 匹配确认(删表)或 {v} 占位需替换(重命名/复制表)。
            // 此前只看 expect,重命名/复制表的输入框根本不渲染 → 确认按钮永久禁用死锁
            let need_input = c.expect.is_some() || c.sql.contains("{v}");
            let c = app.confirm.as_mut().unwrap();
            if need_input {
                ui.add_space(2.0);
                let resp = ui.add_sized(
                    [420.0, 28.0],
                    egui::TextEdit::singleline(&mut c.input)
                        .hint_text(
                            RichText::new(ui_kit::t!("Type the name to confirm", "输入名称以确认"))
                                .size(12.0)
                                .color(p.muted),
                        )
                        .font(theme::mono(12.5)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    proceed = true;
                }
            }
            // expect 匹配确认(删表等):输入与期望不符给红字提示,不静默吞
            if let Some(exp) = &c.expect {
                if !c.input.trim().is_empty() && c.input.trim() != exp {
                    ui.add_space(4.0);
                    let mismatch = if ui_kit::i18n::is_english() {
                        format!("Input does not match the name \"{exp}\"")
                    } else {
                        format!("输入与名称「{exp}」不一致")
                    };
                    ui.label(RichText::new(mismatch).size(11.0).color(p.red));
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
                if theme::button(ui, ui_kit::t!("Confirm", "确认"), kind).clicked() && !need_input {
                    proceed = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
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
            if let Some(conn_id) = action
                .as_deref()
                .and_then(|a| a.strip_prefix("delete-conn:"))
            {
                app.confirm = None;
                app.delete_connection(conn_id);
                let toast = if ui_kit::i18n::is_english() {
                    format!("Connection {input_ok} deleted")
                } else {
                    format!("已删除连接 {}", input_ok)
                };
                app.toast_error(toast);
            } else if let Some(rest) = action
                .as_deref()
                .and_then(|a| a.strip_prefix("delete-rows:"))
            {
                app.confirm = None;
                // 格式 delete-rows:<tab_id>:<gen>:<ris csv>;gen 不符说明弹窗期间数据已刷新
                let mut it = rest.splitn(3, ':');
                let tid = it.next().unwrap_or("");
                let gen = it
                    .next()
                    .and_then(|g| g.parse::<u64>().ok())
                    .unwrap_or(u64::MAX);
                let ris_csv = it.next().unwrap_or("");
                let ris: Vec<usize> = ris_csv
                    .split(',')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect();
                if !tid.is_empty() && !ris.is_empty() {
                    crate::table_view::run_delete_rows(app, tid, gen, &ris);
                }
            } else if let Some(rest) = action
                .as_deref()
                .and_then(|a| a.strip_prefix("mongo-del:"))
            {
                app.confirm = None;
                let mut it = rest.splitn(2, '|');
                let tid = it.next().unwrap_or("").to_string();
                let id = it.next().unwrap_or("").to_string();
                app.run_mongo_delete(&tid, &id);
            } else if let Some(tid) = action
                .as_deref()
                .and_then(|a| a.strip_prefix("save-changes:"))
            {
                app.confirm = None;
                // SQL 预览已确认:直通执行本次保存(不再二次弹预览)
                app.skip_edit_preview = true;
                crate::table_view::execute_tab_action(
                    app,
                    ctx,
                    crate::table_view::TabAction::SaveChanges {
                        tab_id: tid.to_string(),
                    },
                );
            } else if let Some(conn_id) = action
                .as_deref()
                .and_then(|a| a.strip_prefix("disconnect-conn:"))
            {
                app.confirm = None;
                app.trees.remove(conn_id);
                app.close_tabs_for_connection(conn_id);
                let service = app.service.clone();
                let cid = conn_id.to_string();
                net::spawn(async move {
                    let _ = service.disconnect(&cid).await;
                });
                app.toast_error(ui_kit::t!("Disconnected", "已断开连接").to_string());
            } else {
                let conn_id = c.conn_id.clone();
                let mysql = crate::app::mysql_quote(&app.conns, &conn_id);
                let sql = c.resolved_sql(mysql);
                let ddl_msg = c.title.clone();
                // 危险 SQL 必须带库:run_sql 走粘滞会话库,多库连接下可能落到残留库
                let db = c.database.clone();
                app.confirm = None;
                let tx = app.event_tx.clone();
                let service = app.service.clone();
                net::spawn(async move {
                    let result = match db.as_deref() {
                        Some(d) => service
                            .run_sql_db(&conn_id, sql, Some(1), Some(d))
                            .await
                            .map(|_| ()),
                        None => service.run_sql(&conn_id, sql, Some(1)).await.map(|_| ()),
                    };
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
    let Some(tab) = app.tabs.get(app.active_tab) else {
        return;
    };
    let Some((col, rows)) = tab.table_data().and_then(|t| t.col_stats.clone()) else {
        return;
    };
    let p = theme::c();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("col-stats"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(360.0);
            theme::modal_title(ui, &format!("{} · {col}", ui_kit::t!("Column Stats", "列统计")));
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                RichText::new(ui_kit::t!("Based on current page data", "基于当前页数据"))
                    .size(11.0)
                    .color(p.muted),
            );
            ui.add_space(4.0);
            for (k, v) in &rows {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).size(12.0).color(p.muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(v).font(theme::mono(12.5)).color(p.text));
                    });
                });
                theme::hairline_bottom(ui);
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Primary).clicked() {
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
/// Mongo 文档编辑弹窗(插入/编辑;JSON 多行编辑器)
pub fn draw_mongo_doc_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.mongo_doc.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut save = false;
    let modal = egui::Modal::new(egui::Id::new("mongo-doc"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(640.0);
            let Some(st) = app.mongo_doc.as_mut() else { return };
            let title = match &st.doc_id {
                Some(id) => format!(
                    "{} · {}",
                    ui_kit::t!("Edit Document", "编辑文档"),
                    icons::clip(id, 30)
                ),
                None => format!("{} · {}", ui_kit::t!("Insert Document", "插入文档"), st.collection),
            };
            theme::modal_title(ui, &title);
            ui.label(
                RichText::new(ui_kit::t!(
                    "BSON document as JSON; write ObjectId as {\"$oid\": \"hex\"}",
                    "BSON 文档 JSON;ObjectId 写作 {\"$oid\": \"hex\"}"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(4.0);
            theme::form_field_multiline(ui, "", "mongo-doc-json", &mut st.json, 620.0, 14);
            if let Some(err) = &st.error {
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    if st.running {
                        ui_kit::t!("Saving…", "保存中…")
                    } else {
                        ui_kit::t!("Save", "保存")
                    },
                    theme::BtnKind::Primary,
                )
                .clicked()
                    && !st.running
                {
                    save = true;
                }
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if save {
        app.run_mongo_save();
    }
    if close || modal.should_close() {
        app.mongo_doc = None;
    }
}

/// Mongo 索引管理弹窗(列表 + 内嵌创建表单;删除走二次确认按钮)
pub fn draw_mongo_indexes_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.mongo_indexes.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut create = false;
    let mut drop_name: Option<String> = None;
    let mut add_row = false;
    let mut remove_row: Option<usize> = None;
    let modal = egui::Modal::new(egui::Id::new("mongo-indexes"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(700.0);
            let Some(st) = app.mongo_indexes.as_mut() else { return };
            let title = format!(
                "{} · {}",
                ui_kit::t!("Manage Indexes", "索引管理"),
                st.collection
            );
            theme::modal_title(ui, &title);
            ui.label(
                RichText::new(ui_kit::t!(
                    "TTL indexes delete documents after expireAfterSeconds; the _id index cannot be dropped",
                    "TTL 索引按过期秒数自动删除文档;_id 索引不可删除"
                ))
                .size(10.5)
                .color(p.muted),
            );
            ui.add_space(6.0);
            if st.running {
                ui.label(
                    RichText::new(ui_kit::t!("Loading…", "加载中…"))
                        .size(11.5)
                        .color(p.text2),
                );
            } else if st.items.is_empty() && st.error.is_none() {
                ui.label(
                    RichText::new(ui_kit::t!("No indexes", "暂无索引"))
                        .size(11.5)
                        .color(p.muted),
                );
            }
            if let Some(err) = &st.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(11.5).color(p.red));
            }
            if !st.items.is_empty() {
                egui::Grid::new("mongo-idx-grid")
                    .num_columns(4)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(ui_kit::t!("Keys", "键"))
                                .size(10.5)
                                .color(p.muted),
                        );
                        ui.label(
                            RichText::new(ui_kit::t!("Unique", "唯一"))
                                .size(10.5)
                                .color(p.muted),
                        );
                        ui.label(
                            RichText::new(ui_kit::t!("TTL (s)", "TTL(秒)"))
                                .size(10.5)
                                .color(p.muted),
                        );
                        ui.label(RichText::new("").size(10.5));
                        ui.end_row();
                        for it in st.items.clone() {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    icons::clip(&it.name, 30),
                                    it.keys
                                ))
                                .font(theme::mono(11.5))
                                .color(p.text),
                            );
                            ui.label(
                                RichText::new(if it.unique {
                                    ui_kit::t!("Yes", "是")
                                } else {
                                    "—"
                                })
                                .size(11.5)
                                .color(if it.unique { DIFF_ORANGE } else { p.muted }),
                            );
                            ui.label(
                                RichText::new(
                                    it.ttl
                                        .map(|t| t.to_string())
                                        .unwrap_or_else(|| "—".into()),
                                )
                                .font(theme::mono(11.5))
                                .color(p.text),
                            );
                            if it.name == "_id_" {
                                ui.label(RichText::new("").size(11.5));
                            } else {
                                let armed = st.drop_arm.as_deref() == Some(it.name.as_str());
                                let label = if armed {
                                    ui_kit::t!("Click again to confirm", "再点一次确认")
                                } else {
                                    ui_kit::t!("Drop", "删除")
                                };
                                if theme::button(ui, label, theme::BtnKind::Secondary).clicked()
                                    && !st.busy
                                {
                                    if armed {
                                        drop_name = Some(it.name.clone());
                                    } else {
                                        st.drop_arm = Some(it.name.clone());
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                RichText::new(ui_kit::t!("Create Index", "创建索引"))
                    .size(11.5)
                    .color(p.text2),
            );
            ui.add_space(4.0);
            // 键行:字段名 + 方向切换 + 删行(横向一行)
            let row_count = st.rows.len();
            for (ri, (fname, desc)) in st.rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    theme::field(ui, &format!("mongo-idx-f{ri}"), fname, 200.0, false);
                    let dir = if *desc { "desc ↓" } else { "asc ↑" };
                    if theme::button(ui, dir, theme::BtnKind::Secondary).clicked() {
                        *desc = !*desc;
                    }
                    if row_count > 1
                        && theme::button(ui, "×", theme::BtnKind::Secondary).clicked()
                    {
                        remove_row = Some(ri);
                    }
                });
            }
            if theme::button(ui, ui_kit::t!("＋ Key", "＋ 键行"), theme::BtnKind::Secondary).clicked() {
                add_row = true;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                theme::checkbox(ui, &mut st.unique, &ui_kit::t!("Unique", "唯一"));
                ui.add_space(14.0);
                ui.label(
                    RichText::new(ui_kit::t!("TTL (s, empty = none)", "TTL(秒,空=不设)"))
                        .size(11.0)
                        .color(p.muted),
                );
                theme::field(ui, "mongo-idx-ttl", &mut st.ttl, 90.0, false);
                ui.add_space(14.0);
                ui.label(
                    RichText::new(ui_kit::t!("Name (empty = auto)", "索引名(空=自动)"))
                        .size(11.0)
                        .color(p.muted),
                );
                theme::field(ui, "mongo-idx-name", &mut st.name, 160.0, false);
            });
            if let Some(err) = &st.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(11.5).color(p.red));
            }
            theme::modal_footer(ui, |ui| {
                let label = if st.busy || st.running {
                    ui_kit::t!("Working…", "处理中…")
                } else {
                    ui_kit::t!("Create", "创建")
                };
                if theme::button(ui, label, theme::BtnKind::Primary).clicked() && !st.busy && !st.running
                {
                    create = true;
                }
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    // 行增删与删除确认在可变借出结束后统一应用(同帧 widget id 复用防点击错位)
    if let (Some(st), Some(ri)) = (app.mongo_indexes.as_mut(), remove_row) {
        st.rows.remove(ri);
    }
    if let (Some(st), true) = (app.mongo_indexes.as_mut(), add_row) {
        st.rows.push((String::new(), false));
    }
    if app.mongo_indexes.is_some() {
        if let Some(n) = drop_name {
            app.run_mongo_drop_index(&n);
        }
    }
    if create {
        app.run_mongo_create_index();
    }
    if close || modal.should_close() {
        app.mongo_indexes = None;
    }
}

/// 「结果存为新表」命名弹窗(查询结果集 → CREATE TABLE + INSERT)
pub fn draw_save_result_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.save_result_modal.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut commit = false;
    let modal = egui::Modal::new(egui::Id::new("save-result-table"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(420.0);
            let Some((_, name)) = app.save_result_modal.as_mut() else {
                return;
            };
            theme::modal_title(ui, ui_kit::t!("Save Result as New Table", "结果存为新表"));
            ui.label(
                RichText::new(ui_kit::t!(
                    "Creates the table with inferred column types and inserts all rows",
                    "按推断的列类型建表并写入全部数据行"
                ))
                .size(10.5)
                .color(p.muted),
            );
            ui.add_space(6.0);
            theme::form_field(
                ui,
                ui_kit::t!("Table name", "表名"),
                "save-result-table-name",
                name,
                380.0,
                false,
            );
            ui.label(
                RichText::new(ui_kit::t!(
                    "Letters/digits/underscore only, must not start with a digit",
                    "仅限字母/数字/下划线,不能以数字开头"
                ))
                .size(10.5)
                .color(p.muted),
            );
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Create", "创建"), theme::BtnKind::Primary).clicked()
                {
                    commit = true;
                }
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary)
                    .clicked()
                {
                    close = true;
                }
            });
        });
    if close || modal.should_close() {
        app.save_result_modal = None;
    }
    if commit {
        let (tab_id, name) = app.save_result_modal.clone().unwrap_or_default();
        app.save_result_as_table_commit(&tab_id, &name);
    }
}

/// Redis 服务器信息弹窗(INFO 分区块 + 慢日志)
pub fn draw_redis_info_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.redis_info.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut load_slow = false;
    let modal = egui::Modal::new(egui::Id::new("redis-info"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            let Some(st) = app.redis_info.as_mut() else {
                return;
            };
            theme::modal_title(ui, ui_kit::t!("Redis Server Info", "Redis 服务器信息"));
            if st.loading {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.muted));
                    ui.label(RichText::new(ui_kit::t!("Loading…", "加载中…")).size(11.5).color(p.muted));
                });
            }
            if let Some(err) = &st.error {
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            // INFO 区块(键空间/内存/客户端优先可读展示;其余紧凑两列)
            if !st.sections.is_empty() {
                let mut shown_slow_header = false;
                egui::ScrollArea::vertical()
                    .id_salt("redis-info-scroll")
                    .max_height((ui.available_height() - 70.0).max(220.0))
                    .show(ui, |ui| {
                        for sec in &st.sections {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(&sec.name)
                                    .font(theme::bold(12.0))
                                    .color(p.text2),
                            );
                            if sec.name == "Keyspace" {
                                // db0:keys=12,expires=0,avg_ttl=0 → 一行一库
                                for (k, v) in &sec.items {
                                    ui.label(
                                        RichText::new(format!("{}: {}", k, v))
                                            .font(theme::mono(11.5))
                                            .color(p.text),
                                    );
                                }
                                continue;
                            }
                            egui::Grid::new(format!("redis-info-g-{}", sec.name))
                                .num_columns(2)
                                .spacing([14.0, 3.0])
                                .show(ui, |ui| {
                                    for (k, v) in sec.items.iter().take(60) {
                                        ui.label(RichText::new(k).size(11.0).color(p.muted));
                                        ui.label(
                                            RichText::new(icons::clip(v, 48))
                                                .font(theme::mono(11.0))
                                                .color(p.text),
                                        );
                                        ui.end_row();
                                    }
                                });
                            // Memory 区块后插慢日志按钮(一次性)
                            if sec.name == "Memory" && !shown_slow_header && !st.slowlog_loaded {
                                shown_slow_header = true;
                                ui.add_space(4.0);
                                if theme::button(
                                    ui,
                                    ui_kit::t!("Load Slowlog (20)", "加载慢日志(20 条)"),
                                    theme::BtnKind::Secondary,
                                )
                                .clicked()
                                {
                                    load_slow = true;
                                }
                            }
                        }
                        // 慢日志
                        if st.slowlog_loaded {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.label(
                                RichText::new(ui_kit::t!("Slowlog (latest 20)", "慢日志(最近 20 条)"))
                                    .font(theme::bold(12.0))
                                    .color(p.text2),
                            );
                            if st.slowlog.is_empty() {
                                ui.label(
                                    RichText::new(ui_kit::t!("No slow queries", "没有慢查询"))
                                        .size(11.5)
                                        .color(p.muted),
                                );
                            }
                            egui::Grid::new("redis-slowlog-g")
                                .num_columns(3)
                                .spacing([12.0, 3.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new(ui_kit::t!("Time", "耗时")).size(10.5).color(p.muted));
                                    ui.label(RichText::new(ui_kit::t!("Command", "命令")).size(10.5).color(p.muted));
                                    ui.label(RichText::new(ui_kit::t!("Args", "参数")).size(10.5).color(p.muted));
                                    ui.end_row();
                                    for e in &st.slowlog {
                                        ui.label(
                                            RichText::new(format!("{} ms", e.duration_us / 1000))
                                                .font(theme::mono(11.0))
                                                .color(if e.duration_us > 100_000 { DIFF_ORANGE } else { p.text }),
                                        );
                                        ui.label(RichText::new(&e.command).size(11.5).color(p.text));
                                        ui.label(RichText::new(icons::clip(&e.args, 40)).font(theme::mono(11.0)).color(p.text2));
                                        ui.end_row();
                                    }
                                });
                        }
                    });
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if load_slow {
        app.redis_fetch_slowlog();
    }
    if close || modal.should_close() {
        app.redis_info = None;
    }
}

/// Mongo 批量导入弹窗(粘贴/文件读入 JSON 数组)
pub fn draw_mongo_import_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.mongo_import.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut import = false;
    let mut picked: Option<String> = None;
    let modal = egui::Modal::new(egui::Id::new("mongo-import"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(640.0);
            let Some(st) = app.mongo_import.as_mut() else { return };
            let title = format!(
                "{} · {}",
                ui_kit::t!("Import Documents (JSON)", "批量导入文档(JSON)"),
                st.collection
            );
            theme::modal_title(ui, &title);
            ui.label(
                RichText::new(if st.src_csv {
                    ui_kit::t!(
                        "CSV: first row = key names; integers/floats/true/false/empty are auto-typed; a duplicate _id aborts the run at that batch",
                        "CSV:首行=键名;整数/浮点/true/false/空值自动推断;_id 重复会在该批次中断"
                    )
                } else {
                    ui_kit::t!(
                        "Top-level JSON array, each item one document; a duplicate _id aborts the run at that batch",
                        "顶层 JSON 数组,每项一个文档;_id 重复会在该批次中断"
                    )
                })
                .size(10.5)
                .color(p.muted),
            );
            ui.add_space(4.0);
            theme::form_field_multiline(ui, "", "mongo-import-json", &mut st.json, 620.0, 14);
            if theme::button(
                ui,
                ui_kit::t!("Open File…", "选择文件…"),
                theme::BtnKind::Secondary,
            )
            .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json", "ndjson"])
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    picked = Some(path.display().to_string());
                }
            }
            if let Some(err) = &st.error {
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            theme::modal_footer(ui, |ui| {
                let label = if st.running {
                    ui_kit::t!("Importing…", "导入中…")
                } else {
                    ui_kit::t!("Import", "导入")
                };
                if theme::button(ui, label, theme::BtnKind::Primary).clicked() && !st.running {
                    import = true;
                }
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if let (Some(st), Some(path)) = (app.mongo_import.as_mut(), picked) {
        st.src_csv = std::path::Path::new(&path)
            .extension()
            .map(|e| e.eq_ignore_ascii_case("csv"))
            .unwrap_or(false);
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                st.json = s;
                st.error = None;
            }
            Err(e) => st.error = Some(format!("{}: {e}", ui_kit::t!("Read failed", "读取失败"))),
        }
    }
    if import {
        app.run_mongo_import();
    }
    if close || modal.should_close() {
        app.mongo_import = None;
    }
}

/// 连接备份口令弹窗(导出=输入+确认;导入=单口令)
pub fn draw_conn_crypto_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.conn_crypto.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::new("conn-crypto"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(460.0);
            let Some(st) = app.conn_crypto.as_mut() else { return };
            let title = if st.export {
                ui_kit::t!("Export Connections (Encrypted Backup)", "导出连接(加密备份)")
            } else {
                ui_kit::t!("Import Connections (Encrypted Backup)", "导入连接(加密备份)")
            };
            theme::modal_title(ui, title);
            ui.label(
                RichText::new(if st.export {
                    ui_kit::t!(
                        "The backup contains all connections with plaintext passwords, encrypted with AES-256-GCM using the passphrase below; a lost passphrase cannot be recovered",
                        "备份含全部连接与明文密码,用下方口令 AES-256-GCM 加密;口令丢失不可解"
                    )
                    .to_string()
                } else {
                    ui_kit::t!(
                        "Enter the passphrase set during export; connections with the same name and type are skipped",
                        "输入导出时设置的口令;同名同类型连接跳过"
                    )
                    .to_string()
                })
                .size(11.0)
                .color(p.muted),
            );
            ui.add_space(6.0);
            theme::form_field(
                ui,
                ui_kit::t!("Passphrase (at least 8 characters)", "口令(至少 8 位)"),
                "cc-pass",
                &mut st.passphrase,
                440.0,
                true,
            );
            if st.export {
                theme::form_field(
                    ui,
                    ui_kit::t!("Confirm Passphrase", "确认口令"),
                    "cc-confirm",
                    &mut st.confirm,
                    440.0,
                    true,
                );
            }
            if let Some(err) = &st.error {
                ui.add_space(4.0);
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    if st.export {
                        ui_kit::t!("Export", "导出")
                    } else {
                        ui_kit::t!("Import", "导入")
                    },
                    theme::BtnKind::Primary,
                )
                    .clicked()
                {
                    run = true;
                }
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run {
        app.run_conn_crypto();
    }
    if close || modal.should_close() {
        app.conn_crypto = None;
        app.pending_crypto_path = None;
    }
}

/// Redis 批量删除弹窗:pattern 预填 → 计数 → 确认删除
pub fn draw_redis_batch_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.redis_batch_del.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut count = false;
    let mut del = false;
    let modal = egui::Modal::new(egui::Id::new("redis-batch"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(520.0);
            let Some(st) = app.redis_batch_del.as_mut() else { return };
            theme::modal_title(ui, &format!("{} · db{}", ui_kit::t!("Batch Delete Keys", "批量删除 key"), st.db));
            let pattern_line = if ui_kit::i18n::is_english() {
                format!("Pattern: {} (full SCAN cursor, limit 10,000)", st.pattern)
            } else {
                format!("pattern: {}(SCAN 全量游标,上限 1 万)", st.pattern)
            };
            ui.label(RichText::new(pattern_line).size(11.5).color(p.muted));
            ui.add_space(4.0);
            if st.deleting {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                    ui.label(RichText::new(ui_kit::t!("Deleting…", "删除中…")).size(12.0).color(p.muted));
                });
            } else if let Some(n) = st.counted {
                let confirm_line = if ui_kit::i18n::is_english() {
                    format!("Will delete {n} keys, this cannot be undone! Type the pattern to confirm:")
                } else {
                    format!("将删除 {n} 个 key,不可恢复!输入 pattern 确认:")
                };
                ui.label(RichText::new(confirm_line).size(12.0).color(DIFF_ORANGE));
                let mut v = n.to_string();
                let resp = ui.add_sized(
                    [200.0, 26.0],
                    egui::TextEdit::singleline(&mut v).font(theme::mono(12.0)),
                );
                let _ = resp;
                if v == n.to_string() {
                    if theme::button(ui, ui_kit::t!("Confirm Delete", "确认删除"), theme::BtnKind::Danger).clicked() {
                        del = true;
                    }
                } else {
                    ui.label(
                        RichText::new(ui_kit::t!("Type the matching count to enable delete", "输入匹配数量后才能删除"))
                            .size(10.5)
                            .color(p.muted),
                    );
                }
            } else {
                if theme::button(ui, ui_kit::t!("Count First", "先计数"), theme::BtnKind::Secondary).clicked() {
                    count = true;
                }
                ui.label(
                    RichText::new(ui_kit::t!("See how many keys would be deleted before deciding", "先看会删掉多少再决定"))
                        .size(10.5)
                        .color(p.muted),
                );
            }
            if let Some(err) = &st.error {
                ui.add_space(4.0);
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if count {
        app.run_redis_batch_count();
    }
    if del {
        app.run_redis_batch_delete();
    }
    if close || modal.should_close() {
        app.redis_batch_del = None;
    }
}

/// 数据传输弹窗(同类型连接;建表+分批搬运)
/// 备份数据库弹窗:库输入 + 进度 + 完成统计(打开文件夹)
pub fn draw_backup_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some(st) = app.backup.clone() else { return };
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let mut reveal = st.result.as_ref().and_then(|r| r.as_ref().ok()).map(|(_, _, path, _)| path.clone());
    let reveal = reveal.clone();
    let modal = egui::Modal::new(egui::Id::new("backup-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(460.0);
            theme::modal_title(ui, ui_kit::t!("Backup Database", "备份数据库"));
            // 表单体可滚动:小屏视口下超高内容不再被裁(footer 固定可见)
            egui::ScrollArea::vertical()
                .id_salt("backup-modal-scroll")
                .auto_shrink([false, false])
                .max_height(ui.ctx().screen_rect().height() - 150.0)
                .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                RichText::new(ui_kit::t!(
                    "Writes a single SQL file (DDL + data INSERTs) to ~/Documents/数镜备份",
                    "生成单文件 SQL 备份(DDL + 数据 INSERT),保存到 ~/Documents/数镜备份"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            theme::form_field(ui, ui_kit::t!("Connection", "连接"), "backup-conn", &mut st.conn_name.clone(), 430.0, false);
            let mut db = st.db.clone();
            theme::form_field(ui, ui_kit::t!("Database", "库"), "backup-db", &mut db, 430.0, false);
            if db != st.db && !st.running {
                if let Some(b) = app.backup.as_mut() {
                    b.db = db;
                }
            }

            if st.running {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.accent));
                    ui.label(
                        RichText::new(format!(
                            "{} {}/{} · {}",
                            ui_kit::t!("Backing up", "备份中"),
                            st.done,
                            st.total,
                            st.cur_table
                        ))
                            .size(11.5)
                            .color(p.text2),
                    );
                });
            } else if let Some(res) = &st.result {
                ui.add_space(4.0);
                match res {
                    Ok((tables, rows, path, bytes)) => {
                        ui.label(
                            RichText::new(format!(
                                "✓ {} {} · {} {} · {}",
                                tables,
                                ui_kit::t!("tables", "张表"),
                                rows,
                                ui_kit::t!("rows", "行"),
                                crate::app::fmt_bytes(*bytes)
                            ))
                                .size(12.0)
                                .color(p.green),
                        );
                        ui.label(
                            RichText::new(icons::clip(path, 60)).size(10.5).color(p.text2),
                        );
                    }
                    Err(e) => {
                        ui.label(RichText::new(icons::clip(e, 110)).size(11.5).color(p.red));
                    }
                }
            }

            // 最近备份(打开弹窗时扫描,最多 20 条)
            if !st.recent.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(ui_kit::t!("Recent Backups", "最近备份")).size(11.0).color(p.muted));
                egui::ScrollArea::vertical()
                    .id_salt("backup-recent")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for f in &st.recent {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 18.0),
                                egui::Sense::click(),
                            );
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(4),
                                if resp.hovered() { p.hover } else { egui::Color32::TRANSPARENT },
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + 8.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                f.name.clone(),
                                egui::FontId::monospace(10.5),
                                p.text2,
                            );
                            ui.painter().text(
                                egui::pos2(rect.right() - 8.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                crate::app::fmt_bytes(f.bytes),
                                egui::FontId::proportional(10.0),
                                p.muted,
                            );
                            if resp.clicked() {
                                #[cfg(target_os = "macos")]
                                let _ = std::process::Command::new("open").arg("-R").arg(&f.path).spawn();
                            }
                            resp.on_hover_text(&f.path);
                        }
                    });
            }

            });

            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    if st.running {
                        ui_kit::t!("Backing up…", "备份中…")
                    } else if st.result.is_some() {
                        ui_kit::t!("Backup Again", "再次备份")
                    } else {
                        ui_kit::t!("Start Backup", "开始备份")
                    },
                    theme::BtnKind::Primary,
                )
                .clicked()
                {
                    run = true;
                }
                if let Some(path) = &reveal {
                    if theme::button(ui, ui_kit::t!("Reveal in Finder", "打开文件夹"), theme::BtnKind::Secondary)
                        .clicked()
                    {
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
                        #[cfg(not(target_os = "macos"))]
                        let _ = path;
                    }
                }
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run && !st.running {
        app.run_backup();
    }
    if close {
        if app.backup.as_ref().map(|b| b.running) != Some(true) {
            app.backup = None;
        }
    }
    let _ = modal.should_close();
}

/// 插件处理弹窗:行集 JSON → WASM 插件(fuel 沙箱)→ 输出预览/保存
pub fn draw_plugin_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some(pr) = app.plugin_run.clone() else { return };
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let mut save = false;
    let modal = egui::Modal::new(egui::Id::new("plugin-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(620.0);
            theme::modal_title(ui, ui_kit::t!("Process with Plugin", "用插件处理结果"));
            ui.label(
                RichText::new(ui_kit::t!(
                    "Runs a WASM sandbox plugin (fuel-limited, no I/O) over the current result set as JSON",
                    "对当前结果集 JSON 运行 WASM 沙箱插件(燃料限额,无任何 IO 权限)"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(6.0);
            let ok_plugins: Vec<usize> = (0..pr.plugins.len())
                .filter(|i| pr.plugins[*i].ok)
                .collect();
            ui.horizontal(|ui| {
                ui.label(RichText::new(ui_kit::t!("Plugin", "插件")).size(11.5).color(p.text2));
                egui::ComboBox::from_id_salt("plugin-sel")
                    .width(360.0)
                    .selected_text(if ok_plugins.is_empty() {
                        ui_kit::t!("(none)", "(无可用插件)").to_string()
                    } else {
                        let i = ok_plugins[pr.sel.min(ok_plugins.len() - 1)];
                        format!("{} v{}", pr.plugins[i].name, pr.plugins[i].version)
                    })
                    .show_ui(ui, |ui| {
                        for (k, i) in ok_plugins.iter().enumerate() {
                            ui.selectable_value(
                                &mut app.plugin_run.as_mut().unwrap().sel,
                                k,
                                format!("{} v{}", pr.plugins[*i].name, pr.plugins[*i].version),
                            );
                        }
                    });
                if let Some(i) = ok_plugins.get(pr.sel.min(ok_plugins.len().saturating_sub(1))).copied() {
                    if !pr.plugins[i].description.is_empty() {
                        ui.label(
                            RichText::new(icons::clip(&pr.plugins[i].description, 30))
                                .size(10.5)
                                .color(p.muted),
                        );
                    }
                }
            });
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    ui_kit::t!("Input (result set JSON)", "输入(结果集 JSON)"),
                    icons::clip(&pr.input, 90)
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(6.0);
            if pr.running {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.accent));
                    ui.label(RichText::new(ui_kit::t!("Running…", "运行中…")).size(11.5).color(p.text2));
                });
            } else if let Some(e) = &pr.error {
                ui.label(RichText::new(icons::clip(e, 110)).size(11.5).color(p.red));
            } else if let Some(out) = &pr.output {
                ui.label(RichText::new(ui_kit::t!("Output", "输出")).size(11.0).color(p.muted));
                egui::ScrollArea::vertical()
                    .id_salt("plugin-output")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut out.as_str())
                                .font(theme::mono(11.0))
                                .desired_width(600.0)
                                .interactive(false),
                        );
                    });
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    if pr.running {
                        ui_kit::t!("Running…", "运行中…")
                    } else {
                        ui_kit::t!("Run Plugin", "运行插件")
                    },
                    theme::BtnKind::Primary,
                )
                .clicked()
                {
                    run = true;
                }
                if let Some(out) = pr.output.clone() {
                    if theme::button(ui, ui_kit::t!("Copy", "复制"), theme::BtnKind::Secondary).clicked() {
                        ui.output_mut(|o| o.copied_text = out);
                    }
                    if theme::button(ui, ui_kit::t!("Save…", "保存…"), theme::BtnKind::Secondary).clicked() {
                        save = true;
                    }
                }
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run && !pr.running && !pr.plugins.is_empty() {
        app.run_plugin();
    }
    if save {
        app.save_plugin_output();
    }
    if close {
        if app.plugin_run.as_ref().map(|x| x.running) != Some(true) {
            app.plugin_run = None;
        }
    }
    let _ = modal.should_close();
}

/// 查询历史弹窗:过滤 + 列表(新在前),点击回填来源标签编辑器
pub fn draw_history_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some(h) = app.query_history.clone() else { return };
    let p = theme::c();
    let mut close = false;
    let mut pick: Option<String> = None;
    let mut clear = false;
    let modal = egui::Modal::new(egui::Id::new("query-history-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(720.0);
            theme::modal_title(ui, ui_kit::t!("Query History", "查询历史"));
            // 表单体可滚动:小屏视口下超高内容不再被裁(footer 固定可见)
            egui::ScrollArea::vertical()
                .id_salt("history-modal-scroll")
                .auto_shrink([false, false])
                .max_height(ui.ctx().screen_rect().height() - 150.0)
                .show(ui, |ui| {
            ui.label(
                RichText::new(ui_kit::t!(
                    "Persisted locally (~/.shujing-native/query_history.jsonl, latest 500); click a row to fill the editor",
                    "本地持久化(~/.shujing-native/query_history.jsonl,保留 500 条);点击行回填编辑器"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(6.0);
            let f = app.query_history.as_mut().map(|x| x.filter.clone()).unwrap_or_default();
            let mut f = f;
            theme::search_box(
                ui,
                "query-history-filter",
                &mut f,
                ui_kit::t!("Filter by SQL or connection…", "按 SQL 或连接过滤…"),
                700.0,
            );
            if let Some(hm) = app.query_history.as_mut() {
                hm.filter = f;
            }
            ui.add_space(6.0);
            let filter_low = h.filter.to_lowercase();
            let shown: Vec<&shujing_core::history::HistoryEntry> = h
                .items
                .iter()
                .filter(|it| {
                    filter_low.is_empty()
                        || it.sql.to_lowercase().contains(&filter_low)
                        || it.conn.to_lowercase().contains(&filter_low)
                        || it.db.to_lowercase().contains(&filter_low)
                })
                .take(200)
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("query-history-list")
                .max_height(380.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if shown.is_empty() {
                        ui.label(
                            RichText::new(ui_kit::t!("No history yet", "暂无历史"))
                                .size(12.0)
                                .color(p.muted),
                        );
                    }
                    for it in shown {
                        let time = {
                            shujing_core::history::format_ts_local(it.ts)
                        };
                        let head = if it.db.is_empty() {
                            format!("{} · {}", time, it.conn)
                        } else {
                            format!("{} · {} · {}", time, it.conn, it.db)
                        };
                        let status_icon = if it.ok { "✓" } else { "✗" };
                        let status_color = if it.ok { p.green } else { p.red };
                        let first_line: String = it
                            .sql
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .trim()
                            .chars()
                            .take(70)
                            .collect();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 40.0),
                            egui::Sense::click(),
                        );
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(6),
                            if resp.hovered() { p.hover } else { egui::Color32::TRANSPARENT },
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 10.0, rect.top() + 11.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{} {}", status_icon, head),
                            egui::FontId::proportional(10.5),
                            status_color,
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 10.0, rect.bottom() - 11.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{} {}", first_line, if it.ok { format!("({} rows)", it.rows) } else { format!("({})", it.err) }),
                            egui::FontId::monospace(10.5),
                            p.text2,
                        );
                        if resp.clicked() {
                            pick = Some(it.sql.clone());
                        }
                        ui.add_space(2.0);
                    }
                });
            ui.add_space(4.0);
            });

            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    ui_kit::t!("Clear History", "清空历史"),
                    theme::BtnKind::Secondary,
                )
                .clicked()
                {
                    clear = true;
                }
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if let Some(sql) = pick {
        app.pick_history(sql);
        return;
    }
    if clear {
        let hp = app.config_dir.join("query_history.jsonl");
        let _ = std::fs::remove_file(&hp);
        let tid = app.query_history.as_ref().map(|h| h.tab_id.clone()).unwrap_or_default();
        app.open_query_history(&tid);
    }
    if close {
        app.query_history = None;
    }
    let _ = modal.should_close();
}

/// 插件管理器面板:transform(WASM)与 driver(子进程)统一列表 + 状态/版本/说明
pub fn draw_plugin_manager_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if !app.plugin_manager_open {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut rescan = false;
    let drivers = shujing_core::plugins::scan_drivers(&app.config_dir.join("plugins"));
    let modal = egui::Modal::new(egui::Id::new("plugin-manager-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(680.0);
            theme::modal_title(ui, ui_kit::t!("Plugin Manager", "插件管理"));
            ui.label(
                RichText::new(ui_kit::t!(
                    "Transform plugins (WASM sandbox) and driver plugins (sidecar agent) share the plugins/ directory",
                    "transform 插件(WASM 沙箱)与 driver 插件(sidecar agent)共用 plugins/ 目录"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(8.0);

            ui.label(theme::strong(ui_kit::t!("Transform (WASM)", "处理型(WASM)"), 12.5).color(p.text2));
            ui.add_space(2.0);
            if app.plugins.plugins.is_empty() {
                ui.label(
                    RichText::new(ui_kit::t!("(none installed)", "(未安装)")).size(11.5).color(p.muted),
                );
            }
            for it in &app.plugins.plugins {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(6),
                    if resp.hovered() { p.hover } else { egui::Color32::TRANSPARENT },
                );
                let dot = if it.ok { p.green } else { p.red };
                ui.painter().circle_filled(egui::pos2(rect.left() + 10.0, rect.center().y), 4.0, dot);
                ui.painter().text(
                    egui::pos2(rect.left() + 22.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{} v{}  ·  {}", it.name, it.version, it.description),
                    egui::FontId::proportional(11.5),
                    p.text,
                );
                if let Some(e) = &it.err {
                    ui.painter().text(
                        egui::pos2(rect.right() - 8.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        icons::clip(e, 40),
                        egui::FontId::proportional(10.0),
                        p.red,
                    );
                }
                resp.on_hover_text(format!(
                    "{}\n{}",
                    ui_kit::t!(
                        "Result-set transform (input JSON, output JSON)",
                        "结果集处理(输入 JSON,输出 JSON)"
                    ),
                    it.dir.display()
                ));
            }

            ui.add_space(10.0);
            ui.label(theme::strong(ui_kit::t!("Driver (sidecar)", "驱动型(子进程)"), 12.5).color(p.text2));
            ui.add_space(2.0);
            if drivers.is_empty() {
                ui.label(
                    RichText::new(ui_kit::t!("(none installed)", "(未安装)")).size(11.5).color(p.muted),
                );
            }
            for d in &drivers {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(6),
                    if resp.hovered() { p.hover } else { egui::Color32::TRANSPARENT },
                );
                ui.painter().circle_filled(egui::pos2(rect.left() + 10.0, rect.center().y), 4.0, p.accent);
                let types = d.db_types.join("/");
                ui.painter().text(
                    egui::pos2(rect.left() + 22.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!(
                        "{} v{}  ·  {} {}",
                        d.name,
                        d.version,
                        ui_kit::t!("provides", "提供类型"),
                        types
                    ),
                    egui::FontId::proportional(11.5),
                    p.text,
                );
                resp.on_hover_text(format!("{}\n{}", d.description, d.dir.display()));
            }

            ui.add_space(6.0);
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Rescan", "重新扫描"), theme::BtnKind::Primary).clicked() {
                    rescan = true;
                }
                if theme::button(ui, ui_kit::t!("Open Folder", "打开文件夹"), theme::BtnKind::Secondary).clicked() {
                    let dir = app.config_dir.join("plugins");
                    let _ = std::fs::create_dir_all(&dir);
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if rescan {
        app.reload_plugins();
    }
    if close {
        app.plugin_manager_open = false;
    }
    let _ = modal.should_close();
}

pub fn draw_transfer_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.transfer.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::new("transfer-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            let Some(st) = app.transfer.as_mut() else { return };
            let src_type = app
                .conns
                .iter()
                .find(|c| c.id == st.src_conn)
                .map(|c| c.db_type)
                .unwrap_or(shujing_core::DbType::MySql);
            theme::modal_title(ui, ui_kit::t!("Data Transfer", "数据传输"));
            let src_line = if ui_kit::i18n::is_english() {
                format!(
                    "Source: {} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            } else {
                format!(
                    "源:{} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            };
            ui.label(RichText::new(src_line).size(11.5).color(p.muted));
            ui.add_space(6.0);
            ui.label(
                RichText::new(ui_kit::t!("Target connection (same type only)", "目标连接(仅同类型)"))
                    .size(11.0)
                    .color(p.muted),
            );
            let conn_opts: Vec<(String, String)> = app
                .conns
                .iter()
                .filter(|c| c.id != st.src_conn && c.db_type == src_type)
                .map(|c| (c.id.clone(), format!("{} ({})", c.name, c.id)))
                .collect();
            if conn_opts.is_empty() {
                ui.label(
                    RichText::new(ui_kit::t!(
                        "No other connection of the same type — add a target connection in the sidebar first",
                        "没有同类型的其他连接——先在侧栏添加目标连接"
                    ))
                        .size(12.0)
                        .color(DIFF_ORANGE),
                );
            } else {
                let cur = st.dst_conn.clone();
                if let Some(v) = theme::select(ui, "tf-dst-conn", &cur, &conn_opts, 320.0) {
                    st.dst_conn = v;
                }
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Target Database", "目标库"), "tf-dst-db", &mut st.dst_db, 200.0, false);
                });
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Target Table", "目标表名"), "tf-dst-table", &mut st.dst_table, 200.0, false);
                });
            });
            ui.add_space(2.0);
            {
                let mut wd = st.with_data;
                if theme::checkbox(ui, &mut wd, ui_kit::t!("Copy data (unchecked = structure only)", "搬运数据(不勾=只建结构)")).changed() {
                    st.with_data = wd;
                }
            }
            // 字段映射(可选):加载源结构后逐列改目标列名;留空=同构直插自动建表
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let has_map = st.mapping.is_some();
                if theme::button(
                    ui,
                    if has_map {
                        ui_kit::t!("Clear Mapping", "清除映射")
                    } else {
                        ui_kit::t!("Field Mapping…", "字段映射…")
                    },
                    theme::BtnKind::Secondary,
                )
                .clicked()
                {
                    if has_map {
                        st.mapping = None;
                    } else if let Some(cols) = &st.map_srcs {
                        st.mapping = Some(cols.iter().cloned().map(|c| (c, String::new())).collect());
                    } else {
                        st.error = Some(
                            ui_kit::t!(
                                "Source structure still loading, try again",
                                "源结构还在加载,稍后再点"
                            )
                            .to_string(),
                        );
                    }
                }
                if st.mapping.is_some() {
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "Target table must already exist; empty target column = skip",
                            "映射模式不自动建表(目标表须已存在);目标列留空=跳过该源列"
                        ))
                        .size(10.5)
                        .color(p.muted),
                    );
                }
            });
            if let Some(map) = &mut st.mapping {
                egui::ScrollArea::vertical()
                    .id_salt("tf-map-scroll")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        egui::Grid::new("tf-map-grid")
                            .num_columns(3)
                            .spacing([10.0, 3.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new(ui_kit::t!("Source", "源列")).size(10.5).color(p.muted));
                                ui.label(RichText::new(ui_kit::t!("→ Target", "→ 目标列")).size(10.5).color(p.muted));
                                ui.label(RichText::new("").size(10.5));
                                ui.end_row();
                                for (src, dst) in map.iter_mut() {
                                    ui.label(RichText::new(src.clone()).font(theme::mono(11.5)).color(p.text));
                                    theme::field(ui, &format!("tf-map-{}", src), dst, 160.0, false);
                                    ui.end_row();
                                }
                            });
                    });
            }
            {
                let mut df = st.drop_first;
                let r = theme::checkbox(
                    ui,
                    &mut df,
                    ui_kit::t!("Drop target table first (DROP IF EXISTS, destructive)", "先删除目标表(DROP IF EXISTS,破坏性)"),
                );
                let changed = r.changed();
                r.on_hover_text(ui_kit::t!(
                    "Required when the target already has a table with the same name, otherwise creating it fails",
                    "目标已存在同名表时必须勾选,否则建表失败"
                ));
                if changed {
                    st.drop_first = df;
                }
            }
            if let Some(err) = &st.error {
                ui.add_space(4.0);
                ui.label(RichText::new(icons::clip(err, 110)).size(11.5).color(p.red));
            }
            ui.label(
                RichText::new(ui_kit::t!(
                    "Transfer is idempotent and can be rerun: after a failure, check Drop target table first and retry",
                    "传输幂等可重跑:失败后先勾选删除目标表再重试"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            theme::modal_footer(ui, |ui| {
                if theme::button(
                    ui,
                    if st.running {
                        ui_kit::t!("Transferring…", "传输中…")
                    } else {
                        ui_kit::t!("Start Transfer", "开始传输")
                    },
                    theme::BtnKind::Primary,
                )
                .clicked()
                    && !st.running
                {
                    run = true;
                }
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run {
        app.run_transfer();
    }
    if close || modal.should_close() {
        app.transfer = None;
    }
}

/// 数据对比弹窗(按主键逐行;目标侧同步 SQL)
pub fn draw_data_diff_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.data_diff.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::new("data-diff"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(720.0);
            let Some(st) = app.data_diff.as_mut() else { return };
            theme::modal_title(ui, ui_kit::t!("Data Compare", "数据对比"));
            let src_line = if ui_kit::i18n::is_english() {
                format!(
                    "Source: {} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            } else {
                format!(
                    "源:{} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            };
            ui.label(RichText::new(src_line).size(11.5).color(p.muted));
            ui.add_space(6.0);
            ui.label(
                RichText::new(ui_kit::t!("Target (same type connection)", "目标(同类型连接)"))
                    .size(11.0)
                    .color(p.muted),
            );
            let conn_opts: Vec<(String, String)> = app
                .conns
                .iter()
                .filter(|c| c.id != st.src_conn)
                .map(|c| (c.id.clone(), format!("{} ({})", c.name, c.id)))
                .collect();
            let cur = st.dst_conn.clone();
            if let Some(v) = theme::select(ui, "dd-dst-conn", &cur, &conn_opts, 320.0) {
                st.dst_conn = v;
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Target Database", "目标库"), "dd-dst-db", &mut st.dst_db, 200.0, false);
                });
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Target Table", "目标表名"), "dd-dst-table", &mut st.dst_table, 200.0, false);
                });
            });
            ui.add_space(4.0);
            if theme::button(
                ui,
                if st.running {
                    ui_kit::t!("Comparing…", "对比中…")
                } else {
                    ui_kit::t!("Run Compare", "运行对比")
                },
                theme::BtnKind::Primary,
            )
                .clicked()
                && !st.running
            {
                run = true;
            }
            if let Some(err) = &st.error {
                ui.add_space(4.0);
                ui.label(RichText::new(icons::clip(err, 120)).size(12.0).color(p.red));
            }
            if let Some(d) = &st.result {
                ui.add_space(6.0);
                let summary = if ui_kit::i18n::is_english() {
                    format!(
                        "Source {} rows · Target {} rows · only in source {} · only in target {} · changed {}",
                        d.src_rows,
                        d.dst_rows,
                        d.only_src.len(),
                        d.only_dst.len(),
                        d.changed.len()
                    )
                } else {
                    format!(
                        "源 {} 行 · 目标 {} 行 · 源独有 {} · 目标独有 {} · 差异 {}",
                        d.src_rows,
                        d.dst_rows,
                        d.only_src.len(),
                        d.only_dst.len(),
                        d.changed.len()
                    )
                };
                ui.label(RichText::new(summary).size(11.5).color(p.text2));
                if d.truncated {
                    let limit_line = if ui_kit::i18n::is_english() {
                        format!(
                            "Compare limit reached ({} rows), the result may be incomplete",
                            d.only_src.len().max(d.dst_rows)
                        )
                    } else {
                        format!("已达对比上限({} 行),结果可能不完整", d.only_src.len().max(d.dst_rows))
                    };
                    ui.label(RichText::new(limit_line).size(10.5).color(DIFF_ORANGE));
                }
                if !d.changed.is_empty() {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("dd-changes")
                        .max_height(ui.ctx().screen_rect().height() * 0.26)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("dd-grid")
                                .num_columns(4)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    for r in d.changed.iter().take(100) {
                                        for (i, (col, sv, dv)) in r.columns.iter().enumerate() {
                                            if i == 0 {
                                                ui.label(
                                                    RichText::new(format!("pk {}", r.pk))
                                                        .font(theme::mono(11.5))
                                                        .color(DIFF_ORANGE),
                                                );
                                            }
                                            ui.label(RichText::new(col).font(theme::mono(11.5)).color(p.text));
                                            ui.label(RichText::new(icons::clip(sv, 24)).font(theme::mono(11.5)).color(p.green));
                                            ui.label(RichText::new(icons::clip(dv, 24)).font(theme::mono(11.5)).color(p.red));
                                            ui.end_row();
                                        }
                                    }
                                });
                        });
                }
                if !d.sync_sql.is_empty() {
                    ui.add_space(4.0);
                    let sync_label = if ui_kit::i18n::is_english() {
                        format!("Target-side sync SQL ({} statements)", d.sync_sql.len())
                    } else {
                        format!("目标侧同步 SQL({} 条)", d.sync_sql.len())
                    };
                    ui.label(RichText::new(sync_label).size(11.0).color(p.muted));
                    let sql_text = d.sync_sql.join("\n");
                    egui::Frame::new()
                        .fill(p.input_bg)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("dd-sql")
                                .max_height(ui.ctx().screen_rect().height() * 0.24)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let mut job = crate::sql_highlight::highlight(&sql_text);
                                    job.wrap.max_width = ui.available_width();
                                    let galley = ui.fonts(|f| f.layout_job(job));
                                    let sz = galley.size();
                                    ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                                    ui.allocate_space(sz);
                                });
                        });
                    ui.horizontal(|ui| {
                        if theme::button(ui, ui_kit::t!("Copy SQL", "复制 SQL"), theme::BtnKind::Secondary).clicked() {
                            ui.ctx().copy_text(sql_text);
                        }
                        ui.label(
                            RichText::new(ui_kit::t!(
                                "SQL is displayed only, not executed; review and run it manually in the query tab",
                                "SQL 只展示不执行;在查询页人工审阅后运行"
                            ))
                                .size(10.5)
                                .color(p.muted),
                        );
                    });
                }
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run {
        app.run_data_diff();
    }
    if close || modal.should_close() {
        app.data_diff = None;
    }
}

/// AI 助手弹窗:生成/解释/优化三模式,历史记录展示,结果可插入编辑器
pub fn draw_ai_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.ai_assist.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let mut insert_sql: Option<String> = None;
    let modal = egui::Modal::new(egui::Id::new("ai-assist"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(640.0);
            let Some(st) = app.ai_assist.as_mut() else { return };
            theme::modal_title(ui, ui_kit::t!("AI SQL Assistant", "AI SQL 助手"));
            // 模式分段(运行中锁定)
            ui.horizontal(|ui| {
                for (m, label) in [
                    (0u8, ui_kit::t!("Generate SQL", "生成 SQL")),
                    (1, ui_kit::t!("Explain", "解释")),
                    (2, ui_kit::t!("Optimize", "优化建议")),
                    (3, ui_kit::t!("Fix Error", "修复报错")),
                ] {
                    if theme::button(
                        ui,
                        label,
                        if st.mode == m {
                            theme::BtnKind::Primary
                        } else {
                            theme::BtnKind::Secondary
                        },
                    )
                    .clicked()
                        && !st.running
                    {
                        st.mode = m;
                        st.history.clear();
                        st.error = None;
                        if m != 0 {
                            st.input = app
                                .tabs
                                .iter()
                                .find(|t| t.id == st.tab_id)
                                .and_then(|t| match &t.kind {
                                    crate::app::TabKind::Query(q) => {
                                        Some(q.sql.trim().to_string())
                                    }
                                    _ => None,
                                })
                                .unwrap_or_default();
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.ai_config.api_key.is_empty() {
                        ui.label(
                            RichText::new(ui_kit::t!("API Key not configured (Settings → AI)", "未配置 API Key(设置 → AI)"))
                                .size(10.5)
                                .color(DIFF_ORANGE),
                        );
                    } else {
                        ui.label(
                            RichText::new(&app.ai_config.model).size(10.5).color(p.muted),
                        );
                    }
                });
            });
            ui.add_space(4.0);
            let hint = match st.mode {
                0 => ui_kit::t!(
                    "Describe your need, e.g. users with more than 3 orders in the last 7 days",
                    "描述需求,如:查最近 7 天下单超过 3 次的用户"
                ),
                1 => ui_kit::t!("SQL to explain (loaded from the editor)", "要解释的 SQL(已带出编辑器内容)"),
                2 => ui_kit::t!("SQL to optimize (loaded from the editor)", "要优化的 SQL(已带出编辑器内容)"),
                _ => ui_kit::t!(
                    "SQL + error message (loaded from the editor's last error)",
                    "SQL + 报错信息(已带出编辑器内容与最近报错)"
                ),
            };
            theme::form_field_multiline(ui, hint, "ai-input", &mut st.input, 620.0, 4);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if theme::button(
                    ui,
                    if st.running {
                        ui_kit::t!("Requesting…", "请求中…")
                    } else {
                        ui_kit::t!("Send", "发送")
                    },
                    theme::BtnKind::Primary,
                )
                .clicked()
                    && !st.running
                {
                    run = true;
                }
                if let Some(err) = &st.error {
                    ui.label(
                        RichText::new(icons::clip(err, 80)).size(11.0).color(p.red),
                    );
                }
            });
            // 历史结果(最新在前)
            for (i, (title, content)) in st.history.iter().enumerate().rev() {
                ui.add_space(6.0);
                ui.label(RichText::new(title).size(11.0).color(p.muted));
                egui::Frame::new()
                    .fill(p.input_bg)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(format!("ai-out-{i}"))
                            .max_height(ui.ctx().screen_rect().height() * 0.3)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let is_sql = title == "生成 SQL";
                                let text = if is_sql {
                                    // 展示剥出的纯 SQL
                                    shujing_core::ai::extract_sql(content).clone()
                                } else {
                                    content.clone()
                                };
                                let mut job = if is_sql {
                                    crate::sql_highlight::highlight(&text)
                                } else {
                                    let mut j = egui::text::LayoutJob::default();
                                    j.append(
                                        &text,
                                        0.0,
                                        egui::TextFormat::simple(
                                            egui::FontId::proportional(12.0),
                                            p.text2,
                                        ),
                                    );
                                    j
                                };
                                job.wrap.max_width = ui.available_width();
                                let galley = ui.fonts(|f| f.layout_job(job));
                                let sz = galley.size();
                                ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                                ui.allocate_space(sz);
                            });
                    });
                if title == "生成 SQL" {
                    ui.horizontal(|ui| {
                        if theme::button(ui, ui_kit::t!("Insert to Editor", "插入编辑器"), theme::BtnKind::Secondary).clicked() {
                            insert_sql =
                                Some(shujing_core::ai::extract_sql(content));
                        }
                        if theme::button(ui, ui_kit::t!("Copy", "复制"), theme::BtnKind::Secondary).clicked() {
                            ui.ctx()
                                .copy_text(shujing_core::ai::extract_sql(content));
                        }
                    });
                }
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run {
        app.run_ai_assist();
    }
    if let Some(sql) = insert_sql {
        // 插入当前查询标签编辑器(替换全文;用户仍可撤销/编辑)
        if let Some(st) = app.ai_assist.as_ref() {
            let tid = st.tab_id.clone();
            if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == tid) {
                if let crate::app::TabKind::Query(q) = &mut tab.kind {
                    q.sql = sql;
                    q.results.clear();
                }
            }
        }
    }
    if close || modal.should_close() {
        app.ai_assist = None;
    }
}

/// 结构对比弹窗(源表右键进入):目标三选 + 运行 + 差异清单 + 对齐 ALTER 预览
pub fn draw_schema_diff_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.schema_diff.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::new("schema-diff"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(720.0);
            // 弹窗期间借走状态,结束前写回
            let Some(st) = app.schema_diff.as_mut() else { return };
            theme::modal_title(ui, ui_kit::t!("Schema Compare", "结构对比"));
            let src_line = if ui_kit::i18n::is_english() {
                format!(
                    "Source: {} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            } else {
                format!(
                    "源:{} · {} · {}",
                    st.src_conn,
                    st.src_db.as_deref().unwrap_or(ui_kit::t!("(default db)", "(默认库)")),
                    st.src_table
                )
            };
            ui.label(RichText::new(src_line).size(11.5).color(p.muted));
            ui.add_space(8.0);
            ui.label(
                RichText::new(ui_kit::t!("Target (align the target table to the source)", "目标(把目标表对齐到源)"))
                    .size(11.0)
                    .color(p.muted),
            );
            let conn_opts: Vec<(String, String)> = app
                .conns
                .iter()
                .map(|c| (c.id.clone(), format!("{} ({})", c.name, c.id)))
                .collect();
            let cur = st.dst_conn.clone();
            if let Some(v) = theme::select(ui, "diff-dst-conn", &cur, &conn_opts, 300.0) {
                st.dst_conn = v;
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Database (blank = default)", "库(空=默认)"), "diff-dst-db", &mut st.dst_db, 200.0, false);
                });
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    theme::form_field(ui, ui_kit::t!("Table", "表名"), "diff-dst-table", &mut st.dst_table, 200.0, false);
                });
            });
            ui.add_space(4.0);
            if theme::button(
                ui,
                if st.running {
                    ui_kit::t!("Comparing…", "对比中…")
                } else {
                    ui_kit::t!("Run Compare", "运行对比")
                },
                theme::BtnKind::Primary,
            )
                .clicked()
                && !st.running
            {
                run = true;
            }
            if let Some(err) = &st.error {
                ui.add_space(6.0);
                ui.label(RichText::new(icons::clip(err, 120)).size(12.0).color(p.red));
            }
            if let Some(diff) = &st.result {
                ui.add_space(6.0);
                if diff.is_empty() {
                    ui.label(
                        RichText::new(ui_kit::t!("Schemas are identical, no differences", "结构一致,无差异"))
                            .size(12.5)
                            .color(p.green),
                    );
                } else {
                    let diff_summary = if ui_kit::i18n::is_english() {
                        format!(
                            "{} columns added · {} extra columns in target · {} attribute differences",
                            diff.added.len(),
                            diff.removed.len(),
                            diff.changed.len()
                        )
                    } else {
                        format!(
                            "新增 {} 列 · 目标多余 {} 列 · 属性差异 {} 处",
                            diff.added.len(),
                            diff.removed.len(),
                            diff.changed.len()
                        )
                    };
                    ui.label(RichText::new(diff_summary).size(11.5).color(p.text2));
                    egui::ScrollArea::vertical()
                        .id_salt("diff-findings")
                        .max_height(ui.ctx().screen_rect().height() * 0.32)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("diff-grid")
                                .num_columns(4)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    for c in &diff.added {
                                        ui.label(RichText::new(ui_kit::t!("Added", "新增")).size(11.0).color(p.green));
                                        ui.label(RichText::new(&c.name).font(theme::mono(12.0)).color(p.text));
                                        ui.label(RichText::new(&c.data_type).font(theme::mono(12.0)).color(p.text2));
                                        ui.label(RichText::new("").size(11.0));
                                        ui.end_row();
                                    }
                                    for c in &diff.removed {
                                        ui.label(RichText::new(ui_kit::t!("Extra in Target", "目标多余")).size(11.0).color(p.red));
                                        ui.label(RichText::new(&c.name).font(theme::mono(12.0)).color(p.text));
                                        ui.label(RichText::new(&c.data_type).font(theme::mono(12.0)).color(p.text2));
                                        ui.label(RichText::new("").size(11.0));
                                        ui.end_row();
                                    }
                                    for ch in &diff.changed {
                                        ui.label(RichText::new(ui_kit::t!("Changed", "差异")).size(11.0).color(DIFF_ORANGE));
                                        ui.label(
                                            RichText::new(format!("{} · {}", ch.name, ch.field))
                                                .font(theme::mono(12.0))
                                                .color(p.text),
                                        );
                                        ui.label(
                                            RichText::new(format!("{} {}", ui_kit::t!("Source", "源"), ch.src))
                                                .font(theme::mono(12.0))
                                                .color(p.green),
                                        );
                                        ui.label(
                                            RichText::new(format!("{} {}", ui_kit::t!("Target", "目标"), ch.dst))
                                                .font(theme::mono(12.0))
                                                .color(p.red),
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                    if !diff.alter_sql.is_empty() {
                        ui.add_space(4.0);
                        let alter_label = if ui_kit::i18n::is_english() {
                            format!("Aligned ALTER (target dialect, {} statements)", diff.alter_sql.len())
                        } else {
                            format!("对齐 ALTER(目标方言,{} 条)", diff.alter_sql.len())
                        };
                        ui.label(RichText::new(alter_label).size(11.0).color(p.muted));
                        let sql_text = diff.alter_sql.join(";\n") + ";";
                        egui::Frame::new()
                            .fill(p.input_bg)
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("diff-sql")
                                    .max_height(ui.ctx().screen_rect().height() * 0.26)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        let mut job = crate::sql_highlight::highlight(&sql_text);
                                        job.wrap.max_width = ui.available_width();
                                        let galley = ui.fonts(|f| f.layout_job(job));
                                        let sz = galley.size();
                                        ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                                        ui.allocate_space(sz);
                                    });
                            });
                        ui.horizontal(|ui| {
                            if theme::button(ui, ui_kit::t!("Copy SQL", "复制 SQL"), theme::BtnKind::Secondary).clicked() {
                                ui.ctx().copy_text(sql_text.clone());
                            }
                            for w in &diff.warnings {
                                ui.label(RichText::new(icons::clip(w, 60)).size(11.0).color(DIFF_ORANGE));
                            }
                        });
                    }
                }
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if run {
        app.run_schema_diff();
    }
    if close || modal.should_close() {
        app.schema_diff = None;
    }
}
/// Parquet 文件预览(· · ·菜单入口):网格展示前 200 行
pub fn draw_parquet_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    if app.parquet_preview.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let (path, preview) = app.parquet_preview.as_ref().unwrap();
    let file_name = path.rsplit('/').next().unwrap_or(&path).to_string();
    let modal = egui::Modal::new(egui::Id::new("parquet-preview"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(780.0);
            theme::modal_title(ui, &format!("{} · {}", ui_kit::t!("Parquet Preview", "Parquet 预览"), file_name));
            let stats_line = if ui_kit::i18n::is_english() {
                format!(
                    "{} rows × {} columns · {} row groups · {} · previewing first {} rows",
                    icons::thousands(preview.total_rows.max(0) as u64),
                    preview.columns.len(),
                    preview.row_groups,
                    icons::thousands(preview.file_size as u64),
                    preview.rows.len()
                )
            } else {
                format!(
                    "{} 行 × {} 列 · {} 个行组 · {} · 预览前 {} 行",
                    icons::thousands(preview.total_rows.max(0) as u64),
                    preview.columns.len(),
                    preview.row_groups,
                    icons::thousands(preview.file_size as u64),
                    preview.rows.len()
                )
            };
            ui.label(RichText::new(stats_line).size(11.5).color(p.muted));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("parquet-grid")
                .max_height(ui.ctx().screen_rect().height() * 0.6)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut frozen = 0usize;
                    crate::grid::result_rows(
                        ui,
                        "parquet-preview-grid",
                        &preview.columns,
                        &preview.rows,
                        &mut frozen,
                    );
                });
            if preview.truncated {
                let total_line = if ui_kit::i18n::is_english() {
                    format!(
                        "{} rows in total, previewing first {} rows",
                        icons::thousands(preview.total_rows.max(0) as u64),
                        preview.rows.len()
                    )
                } else {
                    format!(
                        "共 {} 行,仅预览前 {} 行",
                        icons::thousands(preview.total_rows.max(0) as u64),
                        preview.rows.len()
                    )
                };
                ui.label(RichText::new(total_line).size(11.0).color(p.muted));
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary).clicked() {
                    close = true;
                }
            });
        });
    if close || modal.should_close() {
        app.parquet_preview = None;
    }
}
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
            theme::modal_title(ui, ui_kit::t!("Appearance", "外观设置"));
            // 表单体可滚动:小屏视口下超高内容不再被裁(footer 固定可见)
            egui::ScrollArea::vertical()
                .id_salt("settings-modal-scroll")
                .auto_shrink([false, false])
                .max_height(ui.ctx().screen_rect().height() - 150.0)
                .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(RichText::new(ui_kit::t!("Accent Color", "强调色")).size(11.0).color(p.muted));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (i, name) in theme::ACCENTS.iter().enumerate() {
                    let sw = egui::Rect::from_center_size(
                        egui::pos2(ui.cursor().left() + 13.0, ui.cursor().center().y),
                        egui::vec2(26.0, 26.0),
                    );
                    let _ = ui.allocate_space(egui::vec2(26.0, 26.0));
                    let resp = ui.interact(sw, egui::Id::new(("accent", i)), Sense::click());
                    let c2 = if theme::is_dark() {
                        theme::accent_color_dark(i)
                    } else {
                        theme::accent_color_light(i)
                    };
                    ui.painter()
                        .rect_filled(sw, egui::CornerRadius::same(13), c2);
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
            ui.label(RichText::new(ui_kit::t!("Theme Preset", "主题预设")).size(11.0).color(p.muted));
            ui.add_space(4.0);
            // 3×3 预设色卡:整卡=背景色,上条=面板色,圆点=当前强调色在该基调下的样子
            egui::Grid::new("theme-presets")
                .num_columns(3)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for (i, pr) in theme::PRESETS.iter().enumerate() {
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(104.0, 56.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), pr.palette.bg);
                        let bar = egui::Rect::from_min_size(
                            rect.left_top() + egui::vec2(6.0, 6.0),
                            egui::vec2(rect.width() - 12.0, 16.0),
                        );
                        ui.painter().rect_filled(bar, egui::CornerRadius::same(4), pr.palette.panel);
                        let dot_c = if pr.dark {
                            theme::accent_color_dark(theme::accent_idx())
                        } else {
                            theme::accent_color_light(theme::accent_idx())
                        };
                        ui.painter().circle_filled(
                            egui::pos2(rect.left() + 14.0, rect.bottom() - 13.0),
                            4.0,
                            dot_c,
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 8.0, rect.bottom() - 13.0),
                            egui::Align2::RIGHT_CENTER,
                            ui_kit::t!(pr.name_en, pr.name_zh),
                            egui::FontId::proportional(10.0),
                            if pr.dark { theme::DARK.text2 } else { theme::LIGHT.text2 },
                        );
                        if theme::preset_idx() == i {
                            ui.painter().rect_stroke(
                                rect.expand(1.5),
                                egui::CornerRadius::same(7),
                                egui::Stroke::new(1.5, p.text),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if resp.clicked() {
                            theme::set_preset(i);
                            app.dark_mode = pr.dark;
                            app.save_theme_settings();
                        }
                        if (i + 1) % 3 == 0 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(6.0);
            ui.label(RichText::new(ui_kit::t!("Theme", "主题")).size(11.0).color(p.muted));
            ui.horizontal(|ui| {
                // 跟随系统开关(手动点 ☀/☾ 会自动解除跟随)
                let on = app.follow_system_theme;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 22.0), Sense::click());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(11),
                    if on { p.accent } else { p.input_bg },
                );
                let knob_x = if on {
                    rect.right() - 11.0
                } else {
                    rect.left() + 11.0
                };
                ui.painter()
                    .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, p.text);
                resp.clone().on_hover_text(ui_kit::t!(
                    "Automatically follow the system light/dark switch",
                    "系统切深浅色时应用自动跟随"
                ));
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
                    RichText::new(if on {
                        ui_kit::t!("Follow system appearance", "跟随系统外观")
                    } else {
                        ui_kit::t!("Manual (toggle with sidebar ☀/☾)", "手动(侧栏 ☀/☾ 切换)")
                    })
                    .size(12.0)
                    .color(p.text2),
                );
            });
            // 定时自动备份
            ui.add_space(8.0);
            ui.label(theme::strong(ui_kit::t!("Scheduled Backup", "定时备份"), 12.5).color(p.text2));
            ui.label(
                RichText::new(ui_kit::t!(
                    "Periodically dump all online MySQL/PostgreSQL/SQLite connections to ~/Documents/数镜备份",
                    "按周期对在线的 MySQL/PostgreSQL/SQLite 连接执行备份,写入 ~/Documents/数镜备份"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.horizontal(|ui| {
                let on = app.auto_backup.enabled;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 22.0), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(11),
                    if on { p.accent } else { p.input_bg },
                );
                let knob_x = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
                ui.painter().circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, p.text);
                if resp.clicked() {
                    app.auto_backup.enabled = !on;
                    app.save_theme_settings();
                }
                ui.label(
                    RichText::new(if on {
                        ui_kit::t!("On", "已开启")
                    } else {
                        ui_kit::t!("Off", "已关闭")
                    })
                        .size(12.0)
                        .color(p.text2),
                );
                ui.separator();
                ui.label(RichText::new(ui_kit::t!("Every", "间隔")).size(11.5).color(p.text2));
                let hours = app.auto_backup.interval_hours.max(1);
                egui::ComboBox::from_id_salt("auto-backup-interval")
                    .width(90.0)
                    .selected_text(format!("{}h", hours))
                    .show_ui(ui, |ui| {
                        for h in [6u32, 12, 24, 72] {
                            ui.selectable_value(&mut app.auto_backup.interval_hours, h, format!("{}h", h));
                        }
                    });
                if app.auto_backup.last_run_ms > 0 {
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            ui_kit::t!("Last run", "上次"),
                            shujing_core::history::format_ts_local(app.auto_backup.last_run_ms)
                        ))
                            .size(10.5)
                            .color(p.muted),
                    );
                }
            });

            // 插件(WASM 沙箱)
            ui.add_space(8.0);
            ui.label(theme::strong(ui_kit::t!("Plugins (WASM)", "插件(WASM)"), 12.5).color(p.text2));
            ui.label(
                RichText::new(ui_kit::t!(
                    "Drop plugin folders (plugin.toml + plugin.wasm) into ~/.shujing-native/plugins",
                    "把插件目录(plugin.toml + plugin.wasm)放入 ~/.shujing-native/plugins"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}: {}/{}",
                        ui_kit::t!("Loaded", "已加载"),
                        app.plugins.ok_count(),
                        app.plugins.plugins.len()
                    ))
                        .size(11.5)
                        .color(p.text2),
                );
                if theme::button(ui, ui_kit::t!("Plugin Manager…", "插件管理…"), theme::BtnKind::Secondary).clicked() {
                    app.plugin_manager_open = true;
                }
                if theme::button(ui, ui_kit::t!("Rescan", "重新扫描"), theme::BtnKind::Secondary).clicked() {
                    app.reload_plugins();
                }
                if theme::button(ui, ui_kit::t!("Open Folder", "打开文件夹"), theme::BtnKind::Secondary).clicked() {
                    let dir = app.config_dir.join("plugins");
                    let _ = std::fs::create_dir_all(&dir);
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }
            });

            // AI 助手
            ui.add_space(8.0);
            ui.label(theme::strong(ui_kit::t!("AI Assistant", "AI 助手"), 12.5).color(p.text2));
            ui.label(
                RichText::new(ui_kit::t!(
                    "OpenAI-compatible endpoint; the key is stored only in local settings.json",
                    "OpenAI 兼容端点;Key 只存本机 settings.json"
                ))
                    .size(10.5)
                    .color(p.muted),
            );
            ui.add_space(4.0);
            theme::form_field(ui, "Base URL", "ai-base", &mut app.ai_config.base_url, 320.0, false);
            theme::form_field(ui, "API Key", "ai-key", &mut app.ai_config.api_key, 320.0, true);
            theme::form_field(ui, ui_kit::t!("Model", "模型"), "ai-model", &mut app.ai_config.model, 320.0, false);
            // 界面语言(框架先行:已迁移文案走英文,未迁移的仍显示中文)
            ui.add_space(6.0);
            ui.label(RichText::new(ui_kit::t!("Language", "界面语言 / Language")).size(11.0).color(p.muted));
            let mut en = ui_kit::i18n::is_english();
            let mut en_mut = en;
            if theme::checkbox(ui, &mut en_mut, "English (partial)").changed() {
                en = en_mut;
                ui_kit::i18n::set_english(en);
                let _ = en;
            }
            });

            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Done", "完成"), theme::BtnKind::Primary).clicked() {
                    close = true;
                }
            });
        });
    if close || modal.should_close() {
        app.settings_open = false;
        // 关闭即保存(含 AI 配置)
        app.save_theme_settings();
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
            theme::modal_title(ui, ui_kit::t!("Filter Shown Databases", "筛选显示的库"));
            ui.spacing_mut().item_spacing.y = 8.0;
            let all_count = app.db_filter.as_ref().map(|f| f.all.len()).unwrap_or(0);
            // 搜索 + 全选/全不选
            ui.horizontal(|ui| {
                let st = app.db_filter.as_mut().unwrap();
                theme::search_box(ui, "db-filter-search", &mut st.search, ui_kit::t!("Search databases", "搜索库"), 200.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(ui_kit::t!("Deselect All", "全不选")).clicked() {
                        let kw = st.search.trim().to_lowercase();
                        if kw.is_empty() {
                            st.selected.clear();
                        } else {
                            st.selected.retain(|s| !s.to_lowercase().contains(&kw));
                        }
                    }
                    if ui.small_button(ui_kit::t!("Select All", "全选")).clicked() {
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
            let filter_line = if ui_kit::i18n::is_english() {
                format!(
                    "Selected {}/{} · unchecked databases are hidden in the sidebar",
                    app.db_filter
                        .as_ref()
                        .map(|f| f.selected.len())
                        .unwrap_or(0),
                    all_count
                )
            } else {
                format!(
                    "已选 {}/{} · 未勾选的库不在侧栏显示",
                    app.db_filter
                        .as_ref()
                        .map(|f| f.selected.len())
                        .unwrap_or(0),
                    all_count
                )
            };
            ui.label(RichText::new(filter_line).size(11.0).color(p.muted));
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
                        .auto_shrink([false, true])
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
                                if theme::checkbox(ui, &mut checked, d).changed() {
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
                                    ui.label(RichText::new(ui_kit::t!("No matching databases", "无匹配库")).size(12.0).color(p.muted));
                                });
                            }
                        });
                });
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Done", "完成"), theme::BtnKind::Primary).clicked() {
                    let st = app.db_filter.as_ref().unwrap();
                    apply = Some(st.selected.clone());
                }
                ui.add_space(6.0);
                if theme::button(ui, ui_kit::t!("Cancel", "取消"), theme::BtnKind::Secondary).clicked() {
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