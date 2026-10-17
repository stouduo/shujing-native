#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod grid;
mod icons;
mod modals;
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
    {
        ensure_installed_from_dmg();
        force_appearance(true);
    }
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([1024.0, 640.0])
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

/// macOS:强制 NSApp 外观随应用主题(标题栏/红绿灯区同色,eframe 未暴露该能力)
#[cfg(target_os = "macos")]
pub fn force_appearance(dark: bool) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass as Class, AnyObject};
    unsafe {
        let app_name = c"NSApplication";
        let Some(cls) = Class::get(app_name) else {
            return;
        };
        let app: *mut AnyObject = msg_send![cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let ns_name = c"NSString";
        let Some(nsstring) = Class::get(ns_name) else {
            return;
        };
        let name: *mut AnyObject = {
            let alloc: *mut AnyObject = msg_send![nsstring, alloc];
            msg_send![alloc, initWithUTF8String: if dark { c"NSAppearanceNameVibrantDark".as_ptr() } else { c"NSAppearanceNameAqua".as_ptr() }]
        };
        let ap_name = c"NSAppearance";
        let Some(appearance_cls) = Class::get(ap_name) else {
            return;
        };
        let appearance: *mut AnyObject = msg_send![appearance_cls, appearanceNamed: name];
        if !appearance.is_null() {
            let _: () = msg_send![app, setAppearance: appearance];
        }
    }
}

/// 从 DMG 挂载卷直接运行时:自动安装到 /Applications 并以正式身份重启(VS Code 安装器同款)
#[cfg(target_os = "macos")]
fn ensure_installed_from_dmg() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if !exe.to_string_lossy().starts_with("/Volumes/") {
        return;
    }
    let Some(src_app) = exe.ancestors().nth(3) else {
        return;
    };
    let dst = std::path::Path::new("/Applications").join("数镜Native.app");
    let _ = std::process::Command::new("rm")
        .args(["-rf", &dst.to_string_lossy()])
        .status();
    let ok = std::process::Command::new("cp")
        .args(["-R", &src_app.to_string_lossy(), &dst.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        let _ = std::process::Command::new("open").arg(&dst).spawn();
        std::process::exit(0);
    }
}
