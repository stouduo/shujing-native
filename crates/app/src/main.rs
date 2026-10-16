#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod grid;
mod icons;
mod net;
mod query;
mod sidebar;
mod sql_highlight;
mod structure;
mod table_view;
mod theme;

#[cfg(test)]
mod render_test;

use app::ShujingApp;

fn main() {
    #[cfg(target_os = "macos")]
    force_dark_appearance();
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

/// macOS:强制 NSApp 深色外观(标题栏/红绿灯区随内容变黑,eframe 未暴露该能力)
#[cfg(target_os = "macos")]
fn force_dark_appearance() {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Class};
    unsafe {
        let app_name = c"NSApplication";
        let Some(cls) = Class::get(app_name) else { return };
        let app: *mut AnyObject = msg_send![cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let ns_name = c"NSString";
        let Some(nsstring) = Class::get(ns_name) else { return };
        let name: *mut AnyObject = {
            let alloc: *mut AnyObject = msg_send![nsstring, alloc];
            msg_send![alloc, initWithUTF8String: c"NSAppearanceNameVibrantDark".as_ptr()]
        };
        let ap_name = c"NSAppearance";
        let Some(appearance_cls) = Class::get(ap_name) else { return };
        let appearance: *mut AnyObject = msg_send![appearance_cls, appearanceNamed: name];
        if !appearance.is_null() {
            let _: () = msg_send![app, setAppearance: appearance];
        }
    }
}
