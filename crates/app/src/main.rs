#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod net;
mod sidebar;
mod table_view;

use app::{BgEvent, ShujingApp};

fn main() {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 880.0])
            .with_min_inner_size([980.0, 620.0])
            .with_title("数镜 Native"),
        ..Default::default()
    };
    eframe::run_native(
        "数镜 Native",
        native,
        Box::new(|cc| Ok(Box::new(ShujingApp::new(cc)))),
    )
    .expect("eframe 启动失败");
}

impl eframe::App for ShujingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.autoopen();
        // 后台任务事件先落地(可能改变标签数据)
        self.drain_events(ctx);

        self.draw_sidebar_panel(ctx);
        table_view::draw_tabs(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tabs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("点击左侧连接,展开库/表后点击表名开始").weak(),
                    );
                });
                return;
            }
            if self.tabs[self.active_tab].loading || !self.tabs[self.active_tab].columns.is_empty()
            {
                let actions = {
                    let tab = &mut self.tabs[self.active_tab];
                    table_view::draw_table_view(ctx, tab)
                };
                for a in actions {
                    table_view::execute_tab_action(self, a);
                }
            } else {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let t = &self.tabs[self.active_tab];
                    ui.label(egui::RichText::new(
                        t.error.clone().unwrap_or_else(|| "加载中…".into()),
                    ));
                    if ui.button("重试").clicked() {
                        let key = t.key();
                        self.load_rows(&key);
                    }
                });
            }
        });

        // 连接弹窗
        if self.conn_modal.is_some() {
            let mut modal = self.conn_modal.take().unwrap();
            let action = table_view::draw_conn_modal(ctx, &mut modal);
            match action {
                Some(table_view::ConnAction::Save(info)) => {
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
                Some(table_view::ConnAction::Test(info)) => {
                    modal.testing = true;
                    modal.test_result = None;
                    self.conn_modal = Some(modal);
                    let tx = self.event_tx.clone();
                    let service = self.service.clone();
                    net::spawn(async move {
                        let r = shujing_core::Service::test_connection(info).await;
                        let payload = r
                            .map(|ok| format!("连接成功: {}", ok.version))
                            .map_err(|e| e);
                        let _ = tx.send(BgEvent::TestDone { result: payload });
                    });
                }
                Some(table_view::ConnAction::Cancel) => self.conn_modal = None,
                None => self.conn_modal = Some(modal),
            }
        }

        // 错误 toast 6 秒后消失
        if let Some((_, at)) = &self.error_toast {
            if at.elapsed().as_secs() >= 6 {
                self.error_toast = None;
            }
        }
    }
}
