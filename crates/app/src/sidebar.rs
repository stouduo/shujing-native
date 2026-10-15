//! 左侧栏:连接列表 + 库/表树 + 表搜索 + toast。

use crate::app::{ConnModal, ShujingApp, BgEvent};
use crate::net;
use crate::{icons, theme};
use egui::RichText;

impl ShujingApp {
    pub fn draw_sidebar_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(236.0)
            .min_width(190.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin { left: 12, right: 8, top: 10, bottom: 8 }),
            )
            .show(ctx, |ui| {
                self.draw_sidebar(ui);
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
                    .fill(theme::ELEVATED)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(icons::clip(msg, 160)).size(12.0).color(theme::TEXT2));
            });
    }

    pub fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("数镜").size(16.0).strong());
            ui.label(RichText::new("Native").size(11.0).color(theme::MUTED));
        });
        ui.add_space(8.0);
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::new(RichText::new("＋ 新建连接").size(12.5)),
            )
            .clicked()
        {
            self.conn_modal = Some(ConnModal::new());
        }
        ui.add_space(6.0);

        let has_tables = self.trees.values().any(|t| t.online && !t.tables.is_empty());
        if has_tables {
            ui.add(
                egui::TextEdit::singleline(&mut self.table_filter)
                    .hint_text(RichText::new("搜索表…").color(theme::MUTED))
                    .desired_width(f32::INFINITY),
            );
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
        ui.horizontal(|ui| {
            icons::dot(ui, online);
            let resp = ui.selectable_label(
                false,
                RichText::new(&c.name)
                    .size(13.0)
                    .color(if online { theme::TEXT } else { theme::TEXT2 }),
            );
            let tip = if online { "点击断开" } else { "点击连接" };
            let hovered = resp.hovered();
            if resp.clicked() {
                self.toggle_connect(c);
            }
            resp.on_hover_text(tip);
            if hovered {
                if ui
                    .small_button(RichText::new("编辑").size(10.0))
                    .on_hover_text("编辑连接")
                    .clicked()
                {
                    self.conn_modal = Some(ConnModal::from_conn(c));
                }
                if ui
                    .small_button(RichText::new("删").size(10.0))
                    .on_hover_text("删除连接")
                    .clicked()
                {
                    self.delete_connection(&c.id);
                }
            }
        });
        let cid = c.id.clone();
        self.draw_tree(ui, &cid);
        ui.add_space(5.0);
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, conn_id: &str) {
        let Some(tree) = self.trees.get(conn_id) else { return };
        if !tree.online {
            return;
        }
        let databases = tree.databases.clone();
        let expanded = tree.expanded_db.clone();
        let tables = tree.tables.clone();
        let filter = self.table_filter.to_lowercase();

        for db in databases {
            let is_open = expanded.as_deref() == Some(db.as_str());
            ui.horizontal(|ui| {
                let tri = icons::triangle(ui, is_open);
                if tri.clicked() {
                    self.toggle_database(conn_id, &db);
                }
                let lbl = ui.selectable_label(
                    is_open,
                    RichText::new(&db)
                        .size(13.0)
                        .color(if is_open { theme::TEXT } else { theme::TEXT2 }),
                );
                if lbl.clicked() {
                    self.toggle_database(conn_id, &db);
                }
                if is_open && filter.is_empty() {
                    ui.label(RichText::new(format!("{}", tables.len())).size(10.0).color(theme::MUTED));
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
                        ui.label(RichText::new("无匹配表").size(11.0).color(theme::MUTED));
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
        ui.horizontal(|ui| {
            icons::table(ui, active);
            let lbl = ui.selectable_label(
                active,
                RichText::new(name)
                    .size(12.5)
                    .color(if active { theme::ACCENT } else { theme::TEXT2 }),
            );
            let tip = if active { "当前标签" } else { "打开表数据" };
            let hovered = lbl.hovered();
            if lbl.clicked() {
                self.open_table(conn_id, name, Some(db.to_string()));
            }
            lbl.on_hover_text(tip);
            if hovered && icons::structure(ui).clicked() {
                self.open_structure(conn_id, name, Some(db.to_string()));
            }
        });
    }

    fn toggle_connect(&mut self, c: &shujing_core::ConnInfo) {
        if self.trees.get(&c.id).map(|t| t.online).unwrap_or(false) {
            self.trees.remove(&c.id);
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
