#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod grid;
mod icons;
mod net;
mod query;
mod sidebar;
mod structure;
mod table_view;
mod theme;

#[cfg(test)]
mod render_test;

use app::ShujingApp;

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
