//! 左侧栏:连接列表 + 库/表树 + 表搜索 + toast。

use crate::app::{ConnModal, ShujingApp, BgEvent};
use crate::net;
use crate::{icons, theme};
use egui::RichText;

impl ShujingApp {
    pub fn draw_sidebar_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(238.0)
            .min_width(190.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg)
                    .stroke(egui::Stroke::new(1.0, theme::c().border))
                    .inner_margin(egui::Margin { left: 12, right: 10, top: 10, bottom: 8 }),
            )
            .show(ctx, |ui| {
                self.draw_sidebar(ctx, ui);
            });
    }

    /// 底部提示条(错误/导出结果),在标签栏之后、内容面板之前调用
    pub fn draw_toast(&mut self, ctx: &egui::Context) {
        if self.error_toast.is_none() {
            return;
        }
        let (msg, at) = self.error_toast.as_ref().unwrap();
        if at.elapsed().as_secs() >= 6 {
            self.error_toast = None;
            return;
        }
        egui::TopBottomPanel::bottom("toast")
            .frame(
                egui::Frame::new()
                    .fill(theme::c().elevated)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(icons::clip(msg, 160)).size(12.0).color(theme::c().text2));
            });
    }

    pub fn draw_sidebar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("数镜").size(14.0).strong());
            ui.label(RichText::new("Native").size(11.0).color(theme::c().muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::text_button(ui, "＋ 连接") {
                    self.conn_modal = Some(ConnModal::new());
                }
                let dark = theme::is_dark();
                if icons::theme_toggle(ui, dark)
                    .on_hover_text(if dark { "切换浅色" } else { "切换深色" })
                    .clicked()
                {
                    let new_dark = !theme::is_dark();
                    self.set_theme_dark(ctx, new_dark);
                }
            });
        });
        theme::hairline_bottom(ui);
        ui.add_space(8.0);

        let has_tables = self
            .trees
            .values()
            .any(|t| t.online && !t.tables_by_db.values().all(|v| v.is_empty()));
        if has_tables {
            icons::search_box(ui, "side-table-search", &mut self.table_filter, "搜索表", ui.available_width());
            ui.add_space(4.0);
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let conns = self.conns.clone();
                for c in conns {
                    self.draw_conn_block(ui, &c);
                }
            });
    }

    fn draw_conn_block(&mut self, ui: &mut egui::Ui, c: &shujing_core::ConnInfo) {
        let online = self.trees.get(&c.id).map(|t| t.online).unwrap_or(false);
        let row_top = ui.cursor().top();
        ui.horizontal(|ui| {
            icons::dot(ui, online);
            let resp = ui.selectable_label(
                false,
                RichText::new(&c.name)
                    .size(13.0)
                    .strong()
                    .color(if online { theme::c().text } else { theme::c().text2}),
            );
            let ver = self
                .trees
                .get(&c.id)
                .filter(|t| t.online)
                .map(|t| t.version.clone())
                .unwrap_or_default();
            if !ver.is_empty() {
                ui.label(RichText::new(&ver).size(10.0).monospace().color(theme::c().muted));
            }
            let tip = if online { "点击断开" } else { "点击连接" };
            if resp.clicked() {
                self.toggle_connect(c);
            }
            resp.on_hover_text(tip);
            // 行级悬停:整行区域都算,移向按钮不会消失
            let row_rect = egui::Rect::from_min_max(
                egui::pos2(ui.min_rect().left(), row_top),
                egui::pos2(ui.max_rect().right(), ui.min_rect().bottom() + 2.0),
            );
            if ui.rect_contains_pointer(row_rect) {
                ui.add_space(6.0);
                let (edit_click, edit_resp) =
                    theme::mini_text_button(ui, "编辑", theme::c().text2);
                edit_resp.on_hover_text("编辑连接");
                if edit_click {
                    self.conn_modal = Some(ConnModal::from_conn(c));
                }
                let (del_click, del_resp) = theme::mini_text_button(ui, "删除", theme::c().red);
                del_resp.on_hover_text("删除连接");
                if del_click {
                    self.delete_connection(&c.id);
                }
            }
        });
        let cid = c.id.clone();
        self.draw_tree(ui, &cid);
        // 连接块之间发丝分组线(最后一块不画)
        ui.add_space(4.0);
        theme::hairline_bottom(ui);
        ui.add_space(4.0);
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, conn_id: &str) {
        // 先快照再释放借用:循环内要调 &mut self
        let Some((online, databases, expanded, tables_by_db, loading)) = self
            .trees
            .get(conn_id)
            .map(|t| {
                (
                    t.online,
                    t.databases.clone(),
                    t.expanded_db.clone(),
                    t.tables_by_db.clone(),
                    t.loading,
                )
            })
        else {
            return;
        };
        if !online {
            return;
        }
        let filter = self.table_filter.to_lowercase();

        for db in databases {
            let is_open = expanded.as_deref() == Some(db.as_str());
            let tables: Vec<shujing_core::TableMeta> =
                tables_by_db.get(&db).cloned().unwrap_or_default();
            ui.horizontal(|ui| {
                let tri = icons::triangle(ui, is_open);
                if tri.clicked() {
                    self.toggle_database(conn_id, &db);
                }
                let lbl = ui.selectable_label(
                    is_open,
                    RichText::new(&db)
                        .size(12.5)
                        .strong()
                        .color(if is_open { theme::c().text } else { theme::c().text2 }),
                );
                if lbl.clicked() {
                    self.toggle_database(conn_id, &db);
                }
                if is_open && filter.is_empty() {
                    if loading {
                        ui.spinner();
                    } else {
                        ui.label(RichText::new(format!("{}", tables.len())).size(12.0).color(theme::c().muted));
                    }
                }
            });
            if is_open {
                let mut shown = 0usize;
                ui.indent(db.clone(), |ui| {
                    for t in &tables {
                        if !filter.is_empty() && !t.name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        shown += 1;
                        self.draw_table_row(ui, conn_id, &db, &t.name);
                    }
                    if !filter.is_empty() && shown == 0 {
                        ui.label(RichText::new("无匹配表").size(12.0).color(theme::c().muted));
                    }
                });
            }
        }
    }

    fn draw_table_row(&mut self, ui: &mut egui::Ui, conn_id: &str, db: &str, name: &str) {
        let active = self
            .active_tab_info()
            .map(|(c, t, d)| c == conn_id && t == name && d == Some(db))
            .unwrap_or(false);
        // iOS 列表:选中行 = 系统蓝浸染 + 圆角;普通行透明
        let frame = if active {
            egui::Frame::new()
                .fill(theme::c().active_row)
                .corner_radius(egui::CornerRadius::same(7))
                .inner_margin(egui::Margin::symmetric(6, 3))
        } else {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(6, 3))
        };
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::table(ui, active);
                let lbl = ui.selectable_label(
                    active,
                    RichText::new(name)
                        .size(13.0)
                        .color(if active { theme::c().text } else { theme::c().text2 }),
                );
                let hovered = lbl.hovered();
                if lbl.clicked() {
                    self.open_table(conn_id, name, Some(db.to_string()));
                }
                let table = name.to_string();
                if let Some(resp) = lbl.context_menu(move |ui| {
                    if ui.button("复制表名").clicked() {
                        ui.ctx().copy_text(table.clone());
                        ui.close_menu();
                    }
                    if ui.button("复制 SELECT *").clicked() {
                        let ident = crate::app::quote_ident_by(&table, true);
                        ui.ctx().copy_text(format!("SELECT * FROM {ident} LIMIT 100;"));
                        ui.close_menu();
                    }
                }) {
                    let _ = &resp;
                }
                if hovered && icons::structure(ui).clicked() {
                    self.open_structure(conn_id, name, Some(db.to_string()));
                }
            });
        });
    }

    fn toggle_connect(&mut self, c: &shujing_core::ConnInfo) {
        if self.trees.get(&c.id).map(|t| t.online).unwrap_or(false) {
            self.trees.remove(&c.id);
            // 断开的连接:相关标签一并关闭,避免残留标签对失效连接报错
            let cid = &c.id;
            let mut removed_before_active = 0usize;
            let mut i = 0usize;
            while i < self.tabs.len() {
                let belongs = match &self.tabs[i].kind {
                    crate::app::TabKind::Table(t) => &t.conn_id == cid,
                    crate::app::TabKind::Structure(t) => &t.conn_id == cid,
                    crate::app::TabKind::Query(q) => &q.conn_id == cid,
                };
                if belongs {
                    if i < self.active_tab {
                        removed_before_active += 1;
                    }
                    self.tabs.remove(i);
                } else {
                    i += 1;
                }
            }
            self.active_tab = self
                .active_tab
                .saturating_sub(removed_before_active)
                .min(self.tabs.len().saturating_sub(1));
            return;
        }
        self.trees.insert(c.id.clone(), crate::app::ConnTree::default());
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let info = c.clone();
        net::spawn(async move {
            let result = service.connect(info.clone()).await;
            let version = result.as_ref().ok().map(|r| r.version.clone());
            let _ = tx.send(BgEvent::ConnectDone {
                conn_id: info.id.clone(),
                result: result.map(|_| version.unwrap_or_default()),
            });
        });
    }

    fn toggle_database(&mut self, conn_id: &str, db: &str) {
        let state = self.trees.get_mut(conn_id);
        let Some(tree) = state else { return };
        if tree.expanded_db.as_deref() == Some(db) {
            tree.expanded_db = None;
            return;
        }
        tree.expanded_db = Some(db.to_string());
        tree.loading = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let (cid, db) = (conn_id.to_string(), db.to_string());
        net::spawn(async move {
            let tables = service.list_tables(&cid, Some(db.clone())).await;
            let _ = tx.send(BgEvent::TablesLoaded {
                conn_id: cid,
                database: Some(db),
                tables: tables.unwrap_or_default(),
            });
        });
    }

    fn delete_connection(&mut self, id: &str) {
        let service = self.service.clone();
        match service.delete_connection(id) {
            Ok(conns) => self.conns = conns,
            Err(e) => self.toast_error(e),
        }
        self.trees.remove(id);
    }
}
