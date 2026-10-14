//! 左侧栏:连接列表 + 库/表树。

use crate::app::{ConnModal, ShujingApp, BgEvent};
use crate::net;
use egui::RichText;

impl ShujingApp {
    pub fn draw_sidebar_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(240.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                self.draw_sidebar(ui);
            });
    }

    pub fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("数镜 Native").size(16.0));
        ui.add_space(6.0);
        if ui.button("＋ 新建连接").clicked() {
            self.conn_modal = Some(ConnModal::new());
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let conns = self.conns.clone();
            for c in conns {
                let online = self.trees.get(&c.id).map(|t| t.online).unwrap_or(false);
                let dot = if online { "●" } else { "○" };
                let dot_color = if online { egui::Color32::from_rgb(80, 200, 120) } else { egui::Color32::GRAY };
                ui.horizontal(|ui| {
                    ui.colored_label(dot_color, dot);
                    if ui
                        .selectable_label(self.active_tab_is_conn(&c.id), RichText::new(&c.name).size(13.0))
                        .clicked()
                    {
                        self.toggle_connect(&c);
                    }
                    if ui.small_button("✕").clicked() {
                        self.delete_connection(&c.id);
                    }
                });
                // 展开的库/表树
                self.draw_tree(ui, &c.id);
            }
        });
    }

    fn active_tab_is_conn(&self, _conn_id: &str) -> bool {
        false
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, conn_id: &str) {
        let Some(tree) = self.trees.get(conn_id) else { return };
        if !tree.online {
            return;
        }
        let databases = tree.databases.clone();
        let expanded = tree.expanded_db.clone();
        let tables = tree.tables.clone();
        for db in databases {
            let is_open = expanded.as_deref() == Some(db.as_str());
            let db_label = db.clone();
            if ui
                .selectable_label(is_open, RichText::new(format!("{} {db_label}", if is_open { "▾" } else { "▸" })).size(13.0))
                .clicked()
            {
                self.toggle_database(conn_id, &db_label);
            }
            if is_open {
                ui.indent(db_label.clone(), |ui| {
                    for t in &tables {
                        let label = RichText::new(format!("▤ {}", t.name)).size(12.0);
                        if ui.selectable_label(false, label).clicked() {
                            self.open_table(conn_id, &t.name.clone(), Some(db.clone()));
                        }
                    }
                });
            }
        }
    }

    fn toggle_connect(&mut self, c: &shujing_core::ConnInfo) {
        if self.trees.get(&c.id).map(|t| t.online).unwrap_or(false) {
            self.trees.remove(&c.id);
            return;
        }
        self.trees.insert(
            c.id.clone(),
            crate::app::ConnTree::default(),
        );
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
            let _ = tx.send(BgEvent::TablesLoaded { conn_id: cid, database: Some(db), tables: tables.unwrap_or_default() });
        });
    }

    fn delete_connection(&mut self, id: &str) {
        let service = self.service.clone();
        if let Err(e) = service.delete_connection(id) {
            self.toast_error(e);
            return;
        }
        self.conns.retain(|c| c.id != id);
        self.trees.remove(id);
    }
}
