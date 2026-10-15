//! 表数据网格视图 + 标签栏 + 连接弹窗 + eframe::App 实现。
//! egui 即时模式惯例:绘制函数返回"动作",由 update 循环执行(避免 self 双重借用)。

use crate::app::{BgEvent, ConnModal, ShujingApp, TabKind, TableTab};
use crate::net;
use crate::query;
use crate::structure;
use crate::{icons, theme};
use egui::RichText;
use egui_extras::Column;
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

// ---------------------------------------------------------------- 标签栏

pub(crate) fn draw_tabs(app: &mut ShujingApp, ctx: &egui::Context) {
    let mut activate: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut new_query = false;

    egui::TopBottomPanel::top("tabbar")
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin { left: 8, right: 8, top: 3, bottom: 0 }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for i in 0..app.tabs.len() {
                    let title = app.tabs[i].title.clone();
                    let (a, c) = draw_one_tab(ui, i == app.active_tab, &title);
                    if a {
                        activate = Some(i);
                    }
                    if c {
                        close = Some(i);
                    }
                }
                if app.tabs.len() < 12 {
                    ui.add_space(6.0);
                    if ui
                        .button(RichText::new("＋").size(13.0))
                        .on_hover_text("新建查询标签 (⌘T)")
                        .clicked()
                    {
                        new_query = true;
                    }
                }
            });
            let r = ui.max_rect();
            ui.painter().line_segment(
                [egui::pos2(r.left(), r.bottom()), egui::pos2(r.right(), r.bottom())],
                egui::Stroke::new(1.0, theme::BORDER),
            );
        });

    if let Some(i) = close {
        app.close_tab(i);
    }
    if let Some(i) = activate {
        app.active_tab = i;
    }
    if new_query {
        app.open_query_tab(None);
    }
}

/// 自绘单个标签:激活态浅底+底部强调线,悬停显关闭钮;返回 (激活, 关闭)
fn draw_one_tab(ui: &mut egui::Ui, active: bool, title: &str) -> (bool, bool) {
    let h = 30.0;
    let font = egui::FontId::proportional(12.5);
    let measure = ui.painter().layout_no_wrap(title.to_string(), font.clone(), theme::TEXT2);
    let w = (measure.size().x + 48.0).clamp(96.0, 230.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let mut activated = resp.clicked();
    let mut closed = resp.middle_clicked();

    let fill = if active {
        theme::PANEL
    } else if resp.hovered() {
        theme::HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    let p = ui.painter().clone();
    p.rect_filled(
        rect,
        egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 },
        fill,
    );
    if active {
        p.rect_filled(
            egui::Rect::from_min_size(
                rect.left_bottom() + egui::vec2(8.0, 0.0),
                egui::vec2(w - 16.0, 2.0),
            ),
            0,
            theme::ACCENT,
        );
    }
    let tcol = if active { theme::TEXT } else { theme::TEXT2 };
    let galley = p.layout_no_wrap(title.to_string(), font, tcol);
    let tpos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    p.galley(tpos, galley, tcol);

    let crect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    let cresp = ui.allocate_rect(crect, egui::Sense::click());
    if cresp.clicked() {
        closed = true;
        activated = false;
    }
    let xcol = if cresp.hovered() {
        theme::TEXT
    } else if resp.hovered() || active {
        theme::TEXT2
    } else {
        egui::Color32::TRANSPARENT
    };
    if xcol != egui::Color32::TRANSPARENT {
        let c = crect.center();
        let d = 3.0;
        let st = egui::Stroke::new(1.3, xcol);
        p.line_segment([egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)], st);
        p.line_segment([egui::pos2(c.x - d, c.y + d), egui::pos2(c.x + d, c.y - d)], st);
    }
    (activated, closed)
}

// ---------------------------------------------------------------- 表数据视图

pub(crate) fn draw_table_view(ctx: &egui::Context, tab_id: &str, tab: &mut TableTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id = tab_id.to_string();

    // 工具栏
    egui::TopBottomPanel::top("table_toolbar")
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&tab.table).strong().size(14.0));
                if let Some(db) = &tab.database {
                    ui.label(RichText::new(db).size(11.0).color(theme::MUTED));
                }
                if tab.loading {
                    ui.spinner();
                }
                if !tab.pk_cols.is_empty() || tab.pk_loaded {
                    let hint = if tab.pk_cols.is_empty() { "无主键 · 只读" } else { "双击单元格编辑" };
                    ui.label(RichText::new(hint).size(10.5).color(theme::MUTED));
                }
                if let Some(err) = &tab.error {
                    ui.label(RichText::new(icons::clip(err, 120)).size(11.0).color(theme::RED));
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
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("保存修改 ({n})"))
                                        .color(theme::BG)
                                        .strong(),
                                )
                                .fill(theme::GREEN),
                            )
                            .clicked()
                        {
                            actions.push(TabAction::SaveChanges { tab_id: tab_id.clone() });
                        }
                    }
                    if ui.button("刷新").clicked() && !tab.loading {
                        actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                    }
                });
            });
        });

    // 底部分页条
    egui::TopBottomPanel::bottom("table_pager")
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(10, 6)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let total_text = match tab.total {
                    Some(t) => format!("共 {t} 行"),
                    None => "行数未知".to_string(),
                };
                ui.label(RichText::new(total_text).size(11.5).color(theme::MUTED));
                if !tab.changes.is_empty() {
                    ui.label(
                        RichText::new(format!("· {} 处未保存", tab.changes.len()))
                            .size(11.5)
                            .color(theme::GREEN),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut ps = tab.page_size;
                    egui::ComboBox::from_id_salt("page-size")
                        .selected_text(format!("{ps} 行/页"))
                        .width(88.0)
                        .show_ui(ui, |ui| {
                            for v in [50usize, 100, 200, 500] {
                                ui.selectable_value(&mut ps, v, format!("{v} 行/页"));
                            }
                        });
                    if ps != tab.page_size {
                        tab.page_size = ps;
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: 1 });
                    }
                    let total_pages =
                        tab.total.map(|t| t.div_ceil(tab.page_size as u64) as usize);
                    let at_first = tab.page <= 1;
                    let at_last = match total_pages {
                        Some(n) => tab.page >= n,
                        None => tab.rows.len() < tab.page_size,
                    };
                    if ui
                        .add_enabled(!at_last && !tab.loading, egui::Button::new("下一页 ›"))
                        .clicked()
                    {
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: tab.page + 1 });
                    }
                    ui.label(
                        RichText::new(match total_pages {
                            Some(n) => format!("第 {} / {n} 页", tab.page),
                            None => format!("第 {} 页", tab.page),
                        })
                        .size(11.5),
                    );
                    if ui
                        .add_enabled(!at_first && !tab.loading, egui::Button::new("‹ 上一页"))
                        .clicked()
                    {
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: tab.page - 1 });
                    }
                });
            });
        });

    // 主内容
    egui::CentralPanel::default().show(ctx, |ui| {
        if tab.columns.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if tab.loading {
                    "加载中…".to_string()
                } else {
                    tab.error.clone().unwrap_or_else(|| "无数据".into())
                };
                ui.label(RichText::new(msg).color(theme::MUTED));
            });
            return;
        }

        let TableTab {
            rows,
            columns,
            editing,
            draft,
            changes,
            pk_cols,
            sort,
            page,
            page_size,
            ..
        } = tab;

        let mut sort_click: Option<String> = None;
        let mut commit: Option<(usize, usize, String)> = None;
        {
            let sc = &mut sort_click;
            let cr = &mut commit;
            let srt = &*sort;

            let mut tb = egui_extras::TableBuilder::new(ui)
                .id_salt("data-table")
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .column(Column::exact(52.0));
            for (i, _c) in columns.iter().enumerate() {
                tb = tb.column(if i + 1 == columns.len() {
                    Column::remainder().clip(true)
                } else {
                    Column::initial(170.0)
                        .resizable(true)
                        .clip(true)
                        .at_least(56.0)
                });
            }
            tb.header(28.0, |mut header| {
                header.col(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("#").size(11.5).color(theme::MUTED));
                    });
                });
                for c in columns.iter() {
                    let active_sort =
                        srt.as_ref().and_then(|(scn, asc)| (*scn == *c).then_some(*asc));
                    let mut label_text = c.clone();
                    if active_sort.is_some() {
                        label_text.push_str("   ");
                    }
                    header.col(|ui| {
                        let resp = ui.selectable_label(
                            active_sort.is_some(),
                            RichText::new(&label_text).size(12.5).strong(),
                        );
                        if resp.clicked() {
                            *sc = Some(c.clone());
                        }
                        if let Some(asc) = active_sort {
                            let p = ui.painter();
                            let c0 = resp.rect.right_center() - egui::vec2(9.0, 0.0);
                            let s = 3.2;
                            let pts = if asc {
                                vec![
                                    egui::pos2(c0.x - s, c0.y + s / 2.0),
                                    egui::pos2(c0.x + s, c0.y + s / 2.0),
                                    egui::pos2(c0.x, c0.y - s),
                                ]
                            } else {
                                vec![
                                    egui::pos2(c0.x - s, c0.y - s / 2.0),
                                    egui::pos2(c0.x + s, c0.y - s / 2.0),
                                    egui::pos2(c0.x, c0.y + s),
                                ]
                            };
                            p.add(egui::Shape::convex_polygon(pts, theme::ACCENT, egui::Stroke::NONE));
                        }
                    });
                }
            })
            .body(|mut body| {
                for (ri, rowdata) in rows.iter().enumerate() {
                    let is_editing_row = matches!(*editing, Some((er, _)) if er == ri);
                    let h = if is_editing_row { 32.0 } else { 26.0 };
                    let row_no = (*page - 1) * *page_size + ri + 1;
                    body.row(h, |mut row| {
                        row.col(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{row_no}")).size(11.0).color(theme::MUTED),
                                );
                            });
                        });
                        for (ci, col_name) in columns.iter().enumerate() {
                            let cell_editing =
                                matches!(*editing, Some((er, ec)) if er == ri && ec == ci);
                            let changed = changes
                                .iter()
                                .any(|(r, cn, _)| *r == ri && cn == col_name);
                            if cell_editing {
                                row.col(|ui| {
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(draft)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(ui.available_width().max(120.0)),
                                    );
                                    resp.request_focus();
                                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    if esc {
                                        *editing = None;
                                    } else if resp.lost_focus() || enter {
                                        *cr = Some((ri, ci, draft.clone()));
                                        *editing = None;
                                    }
                                });
                            } else {
                                let v = &rowdata[ci];
                                row.col(|ui| {
                                    draw_data_cell(
                                        ui,
                                        v,
                                        changed,
                                        !pk_cols.is_empty(),
                                        ri,
                                        ci,
                                        editing,
                                        draft,
                                    );
                                });
                            }
                        }
                    });
                }
            });
        }

        if let Some(c) = sort_click {
            *sort = match &*sort {
                Some((sc, true)) if sc == &c => Some((c.clone(), false)),
                Some((sc, false)) if sc == &c => None,
                _ => Some((c, true)),
            };
            *page = 1;
            actions.push(TabAction::Reload { tab_id });
        }
        if let Some((ri, ci, val)) = commit {
            let col_name = columns[ci].clone();
            let new_val = (!val.is_empty()).then_some(val);
            rows[ri][ci] = new_val.clone();
            changes.retain(|(r, cn, _)| !(*r == ri && cn == &col_name));
            changes.push((ri, col_name, new_val));
        }
    });

    actions
}

/// 普通数据格:满格绘制,双击进入编辑(有主键时)
fn draw_data_cell(
    ui: &mut egui::Ui,
    v: &Option<String>,
    changed: bool,
    has_pk: bool,
    ri: usize,
    ci: usize,
    editing: &mut Option<(usize, usize)>,
    draft: &mut String,
) {
    let text = icons::cell_text(v, 2000);
    let color = icons::cell_text_color(v, changed);
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if changed {
        painter.rect_filled(rect.expand2(egui::vec2(1.0, 2.0)), 2, theme::CHANGE_BG);
    }
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(12.0), color);
    let pos = egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(pos, galley, color);
    if has_pk && resp.double_clicked() {
        *editing = Some((ri, ci));
        *draft = v.as_deref().unwrap_or("").to_string();
    }
}

fn execute_tab_action(app: &mut ShujingApp, action: TabAction) {
    match action {
        TabAction::Reload { tab_id } => {
            let is_struct = app
                .tabs
                .iter()
                .any(|t| t.id == tab_id && matches!(t.kind, TabKind::Structure(_)));
            if is_struct {
                app.load_structure(&tab_id);
            } else {
                app.load_rows(&tab_id);
            }
        }
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
    egui::Window::new(RichText::new("连接配置").size(14.0).strong())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(430.0)
        .show(ctx, |ui| {
            egui::Grid::new("conn-form")
                .num_columns(2)
                .spacing([10.0, 7.0])
                .show(ui, |ui| {
                    ui.label("名称");
                    ui.text_edit_singleline(&mut m.name);
                    ui.end_row();
                    ui.label("类型");
                    egui::ComboBox::from_id_salt("dbtype")
                        .selected_text(format!("{:?}", m.db_type))
                        .width(160.0)
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
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("测试连接").clicked() {
                    action = Some(ConnAction::Test(app_conn_from_modal(m)));
                }
                if let Some(r) = &m.test_result {
                    match r {
                        Ok(msg) => ui.label(RichText::new(msg).size(11.5).color(theme::GREEN)),
                        Err(e) => ui.label(RichText::new(icons::clip(e, 60)).size(11.5).color(theme::RED)),
                    };
                }
            });
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("保存").color(theme::BG).strong())
                            .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    action = Some(ConnAction::Save(app_conn_from_modal(m)));
                }
                if ui.button("取消").clicked() {
                    action = Some(ConnAction::Cancel);
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

        if ui_input_new_query(ctx) {
            self.open_query_tab(None);
        }

        // 面板按占用顺序:侧栏 → 标签栏 → toast → 工具/分页/内容
        self.draw_sidebar_panel(ctx);
        draw_tabs(self, ctx);
        self.draw_toast(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tabs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("点击左侧连接,展开库/表后点击表名开始").color(theme::MUTED));
                });
                return;
            }
            let tab_id = self.tabs[self.active_tab].id.clone();
            let actions = match &mut self.tabs[self.active_tab].kind {
                TabKind::Table(t) => draw_table_view(ctx, &tab_id, t),
                TabKind::Structure(s) => structure::draw_structure_view(ctx, &tab_id, s),
                TabKind::Query(q) => {
                    let conns = self.conns.clone();
                    query::draw_query_view(&conns, ctx, &tab_id, q)
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

/// ⌘T 新建查询标签
fn ui_input_new_query(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::T) && (i.modifiers.command || i.modifiers.ctrl))
}
