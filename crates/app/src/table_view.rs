//! 表数据网格视图 + 标签栏 + 连接弹窗 + eframe::App 实现。
//! egui 即时模式惯例:绘制函数返回"动作",由 update 循环执行(避免 self 双重借用)。

use crate::app::{BgEvent, ConnModal, QueryTab, ShujingApp, Tab, TabKind, TableTab};
use crate::net;
use crate::query;
use crate::sidebar;
use crate::structure;
use egui::RichText;
use shujing_core::Service;

/// 表格视图产生的用户动作
pub enum TabAction {
    Reload { tab_id: String },
    SetPage { tab_id: String, page: usize },
    ExportCsv { tab_id: String },
    ExportSql { tab_id: String },
    SaveChanges { tab_id: String },
    RunQuery { tab_id: String },
}

/// 连接弹窗产生的动作
pub enum ConnAction {
    Save(shujing_core::ConnInfo),
    Test(shujing_core::ConnInfo),
    Cancel,
}

fn draw_tabs(app: &mut ShujingApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("tabbar")
        .frame(egui::Frame::default().inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 4,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let n = app.tabs.len();
                for i in 0..n {
                    let t = &app.tabs[i];
                    let icon = match &t.kind {
                        TabKind::Table(_) => "\u{25a4}",
                        TabKind::Structure(_) => "\u{2637}",
                        TabKind::Query(_) => "\u{25b6}",
                    };
                    let active = i == app.active_tab;
                    let label = RichText::new(format!(
                        "{}{} {}",
                        if active { "\u{25cf} " } else { "" },
                        icon,
                        t.title
                    ))
                    .size(12.0);
                    if ui.selectable_label(active, label).clicked() {
                        app.active_tab = i;
                    }
                    if ui.small_button("×").clicked() {
                        app.close_tab(i);
                        break;
                    }
                }
            });
        });
}

fn draw_table_view(ctx: &egui::Context, tab_id: &str, tab: &mut TableTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id = tab_id.to_string();

    egui::TopBottomPanel::top("table_toolbar")
        .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&tab.table).strong().size(14.0));
                if let Some(db) = &tab.database {
                    ui.label(RichText::new(db).weak().size(11.0));
                }
                if tab.loading {
                    ui.spinner();
                }
                if let Some(err) = &tab.error {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 120, 120),
                        RichText::new(err).size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("导出 SQL").clicked() {
                        actions.push(TabAction::ExportSql { tab_id: tab_id.clone() });
                    }
                    if ui.button("导出 CSV").clicked() {
                        actions.push(TabAction::ExportCsv { tab_id: tab_id.clone() });
                    }
                    if !tab.changes.is_empty() {
                        let n = tab.changes.len();
                        if ui.button(format!("保存修改 ({n})")).clicked() {
                            actions.push(TabAction::SaveChanges { tab_id: tab_id.clone() });
                        }
                    }
                    if ui.button("刷新").clicked() && !tab.loading {
                        actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                    }
                });
            });
        });

    let inner = egui::CentralPanel::default().show(ctx, |ui| {
        if tab.columns.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(if tab.loading { "加载中…" } else { "无数据" });
            });
            return Vec::new();
        }
        // 字段解构:rows 与编辑状态的分离借用
        let TableTab {
            rows,
            columns,
            editing,
            draft,
            changes,
            pk_cols,
            sort,
            ..
        } = tab;

        let mut start_edit: Option<(usize, usize)> = None;
        let mut commit: Option<(usize, usize, String)> = None;
        let mut cancel = false;

        egui::ScrollArea::vertical()
            .id_salt("data-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("data-grid")
                    .striped(true)
                    .num_columns(columns.len() + 1)
                    .show(ui, |ui| {
                        ui.label(RichText::new("#").size(12.0).weak());
                        for c in columns.iter() {
                            let label = match &*sort {
                                Some((sc, asc)) if sc == c => {
                                    format!("{c} {}", if *asc { "▲" } else { "▼" })
                                }
                                _ => c.clone(),
                            };
                            if ui
                                .button(RichText::new(label).strong().size(12.0))
                                .clicked()
                            {
                                // 排序循环:无 → 升 → 降 → 无
                                *sort = match &*sort {
                                    Some((sc, true)) if sc == c => Some((c.clone(), false)),
                                    Some((sc, false)) if sc == c => None,
                                    _ => Some((c.clone(), true)),
                                };
                                tab.page = 1;
                                actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                            }
                            ui.end_row();
                        }
                        for (ri, row) in rows.iter().enumerate() {
                            ui.label(
                                RichText::new(format!(
                                    "{}",
                                    (tab.page - 1) * tab.page_size + ri + 1
                                ))
                                .size(11.0)
                                .weak(),
                            );
                            for (ci, v) in row.iter().enumerate() {
                                let col_name = &columns[ci];
                                let editing_here = editing.as_ref() == Some(&(ri, ci));
                                let changed =
                                    changes.iter().any(|(r, cn, _)| *r == ri && cn == col_name);
                                if editing_here {
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(draft)
                                            .desired_width(160.0)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                    resp.request_focus();
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        cancel = true;
                                    } else if resp.lost_focus() {
                                        commit = Some((ri, ci, draft.clone()));
                                    }
                                } else {
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
                                    let mut label = RichText::new(text).size(12.0);
                                    if v.is_none() {
                                        label = label.weak();
                                    }
                                    if changed {
                                        label = label
                                            .color(egui::Color32::from_rgb(190, 220, 140))
                                            .strong();
                                    }
                                    let resp =
                                        ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                                    if resp.double_clicked() && !pk_cols.is_empty() {
                                        start_edit = Some((ri, ci));
                                        *draft = v.as_deref().unwrap_or("").to_string();
                                    }
                                }
                                ui.end_row();
                            }
                            ui.end_row();
                        }
                    });
            });

        if cancel {
            *editing = None;
        }
        if let Some((ri, ci, val)) = commit {
            let col_name = columns[ci].clone();
            let new_val = (!val.is_empty()).then_some(val);
            rows[ri][ci] = new_val.clone();
            *editing = None;
            changes.retain(|(r, cn, _)| !(*r == ri && cn == &col_name));
            changes.push((ri, col_name, new_val));
        }
        if let Some((ri, ci)) = start_edit {
            *editing = Some((ri, ci));
            *draft = rows[ri][ci].as_deref().unwrap_or("").to_string();
        }

        actions
    });
    inner.inner
}

fn execute_tab_action(app: &mut ShujingApp, action: TabAction) {
    match action {
        TabAction::Reload { tab_id } => app.load_rows(&tab_id),
        TabAction::SetPage { tab_id, page } => {
            if let Some(t) = app.table_tab_mut(&tab_id) {
                t.page = page;
            }
            app.load_rows(&tab_id);
        }
        TabAction::ExportCsv { tab_id } => {
            if let Some(t) = app.table_tab(&tab_id) {
                match export_csv_to_file(t) {
                    Ok(path) => app.toast_error(format!("已导出到 {path}")),
                    Err(e) => app.toast_error(e),
                }
            }
        }
        TabAction::ExportSql { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else { return };
            let default_name = format!("{}_schema.sql", t.table);
            let Some(path) = rfd::FileDialog::new()
                .set_file_name(&default_name)
                .add_filter("SQL", &["sql"])
                .save_file()
            else {
                return;
            };
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .export_table_sql(&conn_id, &table, true)
                    .await
                    .and_then(|(sql, _rows)| {
                        std::fs::write(&path, sql).map_err(|e| format!("写入失败: {e}"))
                    })
                    .map(|_| path.display().to_string());
                let _ = tx.send(BgEvent::ExportDone { result });
            });
        }
        TabAction::SaveChanges { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else { return };
            if t.pk_cols.is_empty() {
                app.toast_error("该表没有主键,无法回写".to_string());
                return;
            }
            let pk_idxs: Vec<usize> = t
                .pk_cols
                .iter()
                .filter_map(|p| t.columns.iter().position(|c| c == p))
                .collect();
            if pk_idxs.len() != t.pk_cols.len() {
                app.toast_error("主键列不在结果集中,无法回写".to_string());
                return;
            }
            let mut updates = Vec::new();
            for (ri, col, val) in &t.changes {
                let Some(row) = t.rows.get(*ri) else { continue };
                let pk: Vec<(String, String)> = pk_idxs
                    .iter()
                    .map(|&i| (t.columns[i].clone(), row[i].clone().unwrap_or_default()))
                    .collect();
                if pk.iter().any(|(_, v)| v.is_empty()) {
                    continue;
                }
                updates.push(shujing_core::edits::CellUpdate {
                    pk,
                    sets: vec![(col.clone(), val.clone())],
                });
            }
            if updates.is_empty() {
                app.toast_error("没有可保存的修改".to_string());
                return;
            }
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .apply_changes(&conn_id, &table, &updates, &[], &[])
                    .await;
                let _ = tx.send(BgEvent::ChangesSaved {
                    tab_id: tab_id.clone(),
                    result: result.map(|r| r.affected),
                });
            });
        }
        TabAction::RunQuery { tab_id } => app.run_query(&tab_id),
    }
}

/// 导出当前页为 CSV,返回目标路径或错误
pub fn export_csv_to_file(tab: &TableTab) -> Result<String, String> {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&tab.columns.join(","));
    out.push('\n');
    for row in &tab.rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| {
                let s = v.as_deref().unwrap_or("");
                if s.contains([',', '"', '\n']) {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s.to_string()
                }
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    let path = std::env::temp_dir().join(format!(
        "shujing_export_{}.csv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::write(&path, out).map_err(|e| format!("导出失败: {e}"))?;
    Ok(path.display().to_string())
}

pub fn draw_conn_modal(ctx: &egui::Context, m: &mut ConnModal) -> Option<ConnAction> {
    let mut action = None;
    let mut open = true;
    egui::Window::new("连接配置")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(430.0)
        .show(ctx, |ui| {
            egui::Grid::new("conn-form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("名称");
                    ui.text_edit_singleline(&mut m.name);
                    ui.end_row();
                    ui.label("类型");
                    egui::ComboBox::from_id_salt("dbtype")
                        .selected_text(format!("{:?}", m.db_type))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m.db_type, shujing_core::DbType::MySql, "MySQL");
                            ui.selectable_value(&mut m.db_type, shujing_core::DbType::Sqlite, "SQLite");
                            ui.selectable_value(&mut m.db_type, shujing_core::DbType::Postgres, "PostgreSQL");
                        });
                    ui.end_row();
                    if m.db_type == shujing_core::DbType::Sqlite {
                        ui.label("文件路径");
                        ui.text_edit_singleline(&mut m.file_path);
                        ui.end_row();
                    } else {
                        ui.label("主机");
                        ui.text_edit_singleline(&mut m.host);
                        ui.end_row();
                        ui.label("端口");
                        ui.text_edit_singleline(&mut m.port);
                        ui.end_row();
                        ui.label("用户");
                        ui.text_edit_singleline(&mut m.user);
                        ui.end_row();
                        ui.label("密码");
                        ui.add(egui::TextEdit::singleline(&mut m.password).password(true));
                        ui.end_row();
                        ui.label("默认库(可选)");
                        ui.text_edit_singleline(&mut m.database);
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("测试连接").clicked() {
                    action = Some(ConnAction::Test(app_conn_from_modal(m)));
                }
                if let Some(r) = &m.test_result {
                    match r {
                        Ok(msg) => ui.colored_label(egui::Color32::from_rgb(80, 200, 120), msg),
                        Err(e) => ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e),
                    };
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("取消").clicked() {
                    action = Some(ConnAction::Cancel);
                }
                if ui.button("保存").clicked() {
                    action = Some(ConnAction::Save(app_conn_from_modal(m)));
                }
            });
        });
    if !open {
        action = action.or(Some(ConnAction::Cancel));
    }
    action
}

fn app_conn_from_modal(m: &ConnModal) -> shujing_core::ConnInfo {
    shujing_core::ConnInfo {
        id: m
            .editing_id
            .clone()
            .unwrap_or_else(|| {
                format!(
                    "c{:x}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                )
            }),
        name: m.name.clone(),
        db_type: m.db_type,
        host: (!m.host.is_empty()).then(|| m.host.clone()),
        port: m.port.parse().ok(),
        user: (!m.user.is_empty()).then(|| m.user.clone()),
        password: (!m.password.is_empty()).then(|| m.password.clone()),
        database: (!m.database.is_empty()).then(|| m.database.clone()),
        file_path: (!m.file_path.is_empty()).then(|| m.file_path.clone()),
        color: None,
        read_only: false,
    }
}

impl eframe::App for ShujingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        self.autoopen();

        self.draw_sidebar_panel(ctx);
        draw_tabs(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tabs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("点击左侧连接,展开库/表后点击表名开始").weak());
                });
                return;
            }
            let tab_id = self.tabs[self.active_tab].id.clone();
            let actions = match &mut self.tabs[self.active_tab].kind {
                TabKind::Table(t) => draw_table_view(ctx, &tab_id, t),
                TabKind::Structure(s) => structure::draw_structure_view(ctx, s),
                TabKind::Query(q) => {
                    let conns = self.conns.clone();
                    query::draw_query_view(&conns, ctx, q)
                }
            };
            for a in actions {
                execute_tab_action(self, a);
            }
        });

        if self.conn_modal.is_some() {
            let mut modal = self.conn_modal.take().unwrap();
            let action = draw_conn_modal(ctx, &mut modal);
            match action {
                Some(ConnAction::Save(info)) => {
                    match self.service.save_connection(info.clone()) {
                        Ok(conns) => {
                            self.conns = conns;
                            self.conn_modal = None;
                        }
                        Err(e) => {
                            self.conn_modal = Some(modal);
                            self.toast_error(format!("保存失败: {e}"));
                        }
                    }
                }
                Some(ConnAction::Test(info)) => {
                    modal.testing = true;
                    modal.test_result = None;
                    self.conn_modal = Some(modal);
                    let tx = self.event_tx.clone();
                    let service = self.service.clone();
                    net::spawn(async move {
                        let r = Service::test_connection(info).await;
                        let payload = r
                            .map(|ok| format!("连接成功: {}", ok.version))
                            .map_err(|e| e);
                        let _ = tx.send(BgEvent::TestDone { result: payload });
                    });
                }
                Some(ConnAction::Cancel) => self.conn_modal = None,
                None => self.conn_modal = Some(modal),
            }
        }

        if let Some((_, at)) = &self.error_toast {
            if at.elapsed().as_secs() >= 6 {
                self.error_toast = None;
            }
        }
    }
}
