//! 功能弹窗:⌘P 快速打开 / CSV 导入向导 / 表统计 / 新建表。
//! 全部使用 theme::modal_* 骨架,动作返回给调用方执行。

use crate::app::{ImportState, QuickOpen, ShujingApp};
use crate::theme;
use egui::RichText;

/// ⌘P 快速打开:过滤当前在线连接的表,回车打开数据
pub fn draw_quick_open(app: &mut ShujingApp, ctx: &egui::Context) {
    let open = std::cell::Cell::new(true);
    let mut picked: Option<(String, String, Option<String>)> = None;

    if app.quick_open.is_none() {
        app.quick_open = Some(QuickOpen { query: String::new(), sel: 0 });
    }
    let mut q = app.quick_open.take().unwrap();

    egui::Window::new("")
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            theme::modal_title(ui, "快速打开表");
            // 输入框
            let resp = ui.add(
                egui::TextEdit::singleline(&mut q.query)
                    .hint_text(RichText::new("输入表名过滤…").color(theme::c().muted))
                    .desired_width(470.0)
                    .font(egui::TextStyle::Body),
            );
            resp.request_focus();
            ui.add_space(8.0);

            // 候选:所有在线树的表
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
                        if query.is_empty() || t.name.to_lowercase().contains(&query) {
                            candidates.push((cid.clone(), t.name.clone(), Some(db.clone()), conn_name.clone()));
                        }
                    }
                }
            }
            candidates.truncate(12);

            // 键盘:上下选择,回车打开
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
            let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            if !candidates.is_empty() {
                if down {
                    q.sel = (q.sel + 1).min(candidates.len() - 1);
                }
                if up {
                    q.sel = q.sel.saturating_sub(1);
                }
                q.sel = q.sel.min(candidates.len() - 1);
            }

            egui::ScrollArea::vertical()
                .max_height(280.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if candidates.is_empty() {
                        ui.label(RichText::new("无匹配表").size(12.0).color(theme::c().muted));
                    }
                    for (i, (cid, name, db, conn_name)) in candidates.iter().enumerate() {
                        let is_sel = i == q.sel;
                        let btn = egui::Button::new(
                            RichText::new(format!("{name}   {conn_name}/{}", db.clone().unwrap_or_default()))
                                .size(12.5)
                                .color(if is_sel { theme::c().text } else { theme::c().text2 }),
                        )
                        .fill(if is_sel {
                            theme::c().accent.gamma_multiply(0.3)
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(470.0, 28.0));
                        if ui.add(btn).clicked() || (is_sel && enter) {
                            picked = Some((cid.clone(), name.clone(), db.clone()));
                        }
                    }
                });
        });

    if enter_pressed(ctx) && picked.is_none() {
        if let Some(c) = candidates_snapshot(app, &q.query, q.sel) {
            picked = Some(c);
        }
    }

    if !open.get() {
        app.quick_open = None;
    } else {
        app.quick_open = Some(q);
    }
    if let Some((conn, table, db)) = picked {
        app.quick_open = None;
        app.open_table(&conn, &table, db);
    }
}

fn enter_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

fn candidates_snapshot(
    app: &ShujingApp,
    query: &str,
    sel: usize,
) -> Option<(String, String, Option<String>)> {
    let query = query.to_lowercase();
    let mut candidates: Vec<(String, String, Option<String>)> = Vec::new();
    for (cid, tree) in &app.trees {
        if !tree.online {
            continue;
        }
        for (db, tbls) in &tree.tables_by_db {
            for t in tbls {
                if query.is_empty() || t.name.to_lowercase().contains(&query) {
                    candidates.push((cid.clone(), t.name.clone(), Some(db.clone())));
                }
            }
        }
    }
    candidates.get(sel).cloned()
}

/// CSV 导入向导:选文件列映射到目标表列 → 导入
pub fn draw_import_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some(im) = app.import.as_mut() else { return };
    let open = std::cell::Cell::new(true);
    let mut do_import = false;
    let mut parse_now = false;

    egui::Window::new("")
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            theme::modal_title(ui, &format!("导入 CSV → {}", im.table));
            ui.label(
                RichText::new(format!("文件: {}", crate::icons::clip(&im.path, 60)))
                    .size(11.0)
                    .color(theme::c().muted),
            );
            ui.add_space(10.0);

            if im.headers.is_empty() {
                if ui.button("读取文件预览").clicked() {
                    parse_now = true;
                }
            } else {
                ui.label(
                    RichText::new("列映射(源 CSV 列 → 目标表列;留空跳过该列)")
                        .size(11.0)
                        .color(theme::c().muted),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        egui::Grid::new("import-map")
                            .num_columns(4)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("CSV 列").size(11.5).color(theme::c().muted));
                                ui.label(RichText::new("→").size(11.5).color(theme::c().muted));
                                ui.label(RichText::new("目标表列").size(11.5).color(theme::c().muted));
                                ui.end_row();
                                for (i, h) in im.headers.clone().into_iter().enumerate() {
                                    ui.label(RichText::new(h).size(12.0).monospace());
                                    ui.label(RichText::new("→").size(11.5).color(theme::c().muted));
                                    let cur = im.col_map.get(i).cloned().flatten();
                                    let opts: Vec<String> = std::iter::once(String::new())
                                        .chain(im.headers_target().into_iter())
                                        .collect();
                                    let cur_label = cur
                                        .and_then(|ci| im.headers_target().get(ci).cloned())
                                        .unwrap_or_else(|| "(跳过)".into());
                                    egui::ComboBox::from_id_salt(format!("imap-{i}"))
                                        .selected_text(
                                            RichText::new(cur_label).size(12.0).color(theme::c().text),
                                        )
                                        .width(180.0)
                                        .show_ui(ui, |ui| {
                                            for (oi, o) in opts.iter().enumerate() {
                                                let target_idx = oi.checked_sub(1);
                                                if ui
                                                    .selectable_label(cur == target_idx, o.clone())
                                                    .clicked()
                                                {
                                                    while im.col_map.len() <= i {
                                                        im.col_map.push(None);
                                                    }
                                                    im.col_map[i] = target_idx;
                                                }
                                            }
                                        });
                                    ui.end_row();
                                }
                            });
                    });
            }

            if let Some(err) = &im.batch_errors {
                ui.add_space(6.0);
                ui.label(RichText::new(icons_clip(err)).size(11.5).color(theme::c().red));
            }

            theme::modal_footer(ui, |ui| {
                if !im.headers.is_empty()
                    && theme::modal_primary(ui, "开始导入", theme::c().accent)
                {
                    do_import = true;
                }
                ui.add_space(8.0);
                if theme::modal_secondary(ui, "取消") {
                    open.set(false);
                }
            });
        });

    if parse_now {
        let path = im.path.clone();
        match shujing_core::composite::parse_csv(&path) {
            Ok((headers, rows)) => {
                let _ = rows;
                im.headers = headers;
                im.col_map = (0..im.headers.len())
                    .map(|i| {
                        im.headers
                            .get(i)
                            .and_then(|h| Some(i))
                            .or(Some(i))
                    })
                    .collect();
            }
            Err(e) => im.batch_errors = Some(e),
        }
    }
    if do_import {
        im.batch_errors = None;
        app.start_import();
    }
    if !open.get() {
        app.import = None;
    }
}

fn icons_clip(s: &str) -> String {
    crate::icons::clip(s, 60)
}

/// 表统计弹窗
pub fn draw_stats_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some((key, data, err)) = app.stats.clone() else { return };
    let open = std::cell::Cell::new(true);

    egui::Window::new("")
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            theme::modal_title(ui, "表统计");
            if let Some((rows, data_len, idx_len)) = data {
                egui::Grid::new("stats-grid")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        stat_row(ui, "行数", &format!("{rows}"));
                        if let Some(d) = data_len {
                            stat_row(ui, "数据大小", &fmt_bytes(d));
                        }
                        if let Some(i) = idx_len {
                            stat_row(ui, "索引大小", &fmt_bytes(i));
                        }
                    });
            } else if let Some(e) = err {
                ui.label(RichText::new(e).size(12.0).color(theme::c().red));
            }
            let _ = key;
            theme::modal_footer(ui, |ui| {
                if theme::modal_secondary(ui, "关闭") {
                    open.set(false);
                }
            });
        });
    if !open.get() {
        app.stats = None;
    }
}

fn stat_row(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(RichText::new(k).size(12.0).color(theme::c().muted));
    ui.label(RichText::new(v).size(13.0).strong());
    ui.end_row();
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / (1 << 10) as f64)
    } else {
        format!("{n} B")
    }
}

/// 新建表弹窗:表名 + 列定义 SQL 片段
pub fn draw_new_table_modal(app: &mut ShujingApp, ctx: &egui::Context) {
    let Some((conn, db, _)) = app.new_table.clone() else { return };
    let open = std::cell::Cell::new(true);
    let mut create = false;
    let mut name = String::new();

    // 编辑缓冲(挂在 app 上会被清理逻辑吞,先用独立窗口局部状态:
    // 简化实现——SQL 直接生成,列定义用多行文本)
    let mut sql = String::new();
    if let Some((_, _, s)) = app.new_table.as_ref() {
        sql = s.clone();
    }
    let name_ref = name.clone();

    egui::Window::new("")
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            theme::modal_title(ui, &format!("新建表  {conn}{}", db.as_deref().map(|d| format!(" · {d}")).unwrap_or_default()));
            ui.label(RichText::new("表名").size(11.0).color(theme::c().muted));
            let name_resp = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text(RichText::new("my_table").color(theme::c().muted))
                    .desired_width(460.0)
                    .font(egui::TextStyle::Body),
            );
            ui.add_space(8.0);
            ui.label(RichText::new("列定义(CREATE TABLE 内容,不含表名)").size(11.0).color(theme::c().muted));
            ui.add(
                egui::TextEdit::multiline(&mut sql)
                    .code_editor()
                    .desired_width(470.0)
                    .desired_rows(7),
            );
            theme::modal_footer(ui, |ui| {
                let name_ok = !name_ref.trim().is_empty();
                if ui
                    .add_enabled(
                        name_ok && !sql.trim().is_empty(),
                        egui::Button::new(
                            RichText::new("创建")
                                .size(12.5)
                                .color(theme::c().text)
                                .strong(),
                        )
                        .fill(theme::c().accent)
                        .corner_radius(egui::CornerRadius::same(9))
                        .min_size(egui::vec2(84.0, 28.0)),
                    )
                    .clicked()
                {
                    create = true;
                }
                ui.add_space(8.0);
                if theme::modal_secondary(ui, "取消") {
                    open.set(false);
                }
            });
            let _ = name_resp;
        });

    if create {
        let full_sql = format!(
            "CREATE TABLE {} (\n{}\n)",
            shujing_core::edits::quote_ident_pub(&db, &name.trim()),
            sql.trim().trim_end_matches(';')
        );
        if let Some((_, _, store)) = app.new_table.as_mut() {
            *store = full_sql.clone();
        }
        let info = app
            .conns
            .iter()
            .find(|c| &c.id == &conn)
            .cloned();
        if let Some(info) = info {
            let service = app.service.clone();
            let tx = app.event_tx.clone();
            crate::net::spawn(async move {
                let r = shujing_core::Service::run_sql_standalone(&service, &info, &full_sql).await;
                let _ = tx.send(crate::app::BgEvent::DdlDone {
                    result: r.map(|_| ()).map_err(|e| e),
                });
            });
        }
    }
    if !open.get() {
        app.new_table = None;
    }
}
