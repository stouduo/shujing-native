//! 表数据网格视图 + 连接弹窗。
//! egui 即时模式惯例:绘制函数返回"动作",由 update 循环执行(避免 self 双重借用)。

use crate::app::{BgEvent, ConnModal, ShujingApp, TableTab};
use egui::RichText;

/// 表格视图产生的用户动作
pub enum TabAction {
    Reload { key: String },
    SetPage { key: String, page: usize },
    ExportCsv { key: String },
}

/// 连接弹窗产生的动作
pub enum ConnAction {
    Save(shujing_core::ConnInfo),
    Test(shujing_core::ConnInfo),
    Cancel,
}

pub fn draw_tabs(app: &mut ShujingApp, ctx: &egui::Context) {
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
                    let label: RichText =
                        RichText::new(format!("{}▤ {}", if i == app.active_tab { "● " } else { "" }, t.table))
                            .size(12.0);
                    if ui.selectable_label(i == app.active_tab, label).clicked() {
                        app.active_tab = i;
                    }
                }
            });
        });
}

pub fn draw_table_view(ctx: &egui::Context, tab: &mut TableTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let key = tab.key();

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
                    if ui.button("导出 CSV").clicked() {
                        actions.push(TabAction::ExportCsv { key: key.clone() });
                    }
                    if ui.button("刷新").clicked() && !tab.loading {
                        actions.push(TabAction::Reload { key: key.clone() });
                    }
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        if tab.columns.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(if tab.loading { "加载中…" } else { "无数据" });
            });
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("data-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("data-grid")
                    .striped(true)
                    .num_columns(tab.columns.len() + 1)
                    .show(ui, |ui| {
                        ui.label(RichText::new("#").size(12.0).weak());
                        for c in &tab.columns {
                            let label = match &tab.sort {
                                Some((sc, asc)) if sc == c => {
                                    format!("{c} {}", if *asc { "▲" } else { "▼" })
                                }
                                _ => c.clone(),
                            };
                            if ui
                                .add(egui::Button::new(RichText::new(label).strong().size(12.0)))
                                .clicked()
                            {
                                tab.sort = match &tab.sort {
                                    Some((sc, true)) if sc == c => Some((c.clone(), false)),
                                    Some((sc, false)) if sc == c => None,
                                    _ => Some((c.clone(), true)),
                                };
                                tab.page = 1;
                                actions.push(TabAction::Reload { key: key.clone() });
                            }
                            ui.end_row();
                        }
                        for (ri, row) in tab.rows.iter().enumerate() {
                            ui.label(
                                RichText::new(format!(
                                    "{}",
                                    (tab.page - 1) * tab.page_size + ri + 1
                                ))
                                .size(11.0)
                                .weak(),
                            );
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
    });

    egui::TopBottomPanel::bottom("table_pager")
        .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let total = tab.total.unwrap_or(0);
                let total_pages =
                    (total + tab.page_size as u64 - 1) / tab.page_size.max(1) as u64;
                if ui
                    .add_enabled(tab.page > 1 && !tab.loading, egui::Button::new("‹ 上一页"))
                    .clicked()
                {
                    actions.push(TabAction::SetPage { key: key.clone(), page: tab.page - 1 });
                }
                ui.label(RichText::new(format!(
                    "第 {} 页 / {} · 共 {total} 行",
                    tab.page, total_pages
                ))
                .size(11.0));
                if ui
                    .add_enabled(
                        total_pages > 0 && (tab.page as u64) < total_pages && !tab.loading,
                        egui::Button::new("下一页 ›"),
                    )
                    .clicked()
                {
                    actions.push(TabAction::SetPage { key: key.clone(), page: tab.page + 1 });
                }
            });
        });

    actions
}

pub fn execute_tab_action(app: &mut ShujingApp, action: TabAction) {
    match action {
        TabAction::Reload { key } => app.load_rows(&key),
        TabAction::SetPage { key, page } => {
            if let Some(t) = app.tabs.iter_mut().find(|t| t.key() == key) {
                t.page = page;
            }
            app.load_rows(&key);
        }
        TabAction::ExportCsv { key } => {
            if let Some(t) = app.tabs.iter().find(|t| t.key() == key) {
                match export_csv_to_file(t) {
                    Ok(path) => app.toast_error(format!("已导出到 {path}")),
                    Err(e) => app.toast_error(e),
                }
            }
        }
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
