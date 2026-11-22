//! 组件库转发层:实体已抽到 ui-kit crate(通用组件库),此处保持原路径
//! 以免全应用逐文件改 import;新代码可直接用 ui_kit::。

pub use ui_kit::theme::*;

/// 弹窗内长表单的滚动条:常驻可见,避免"内容被裁但看不出能滚"
pub fn modal_scroll_style(ui: &mut egui::Ui) {
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.65;
    ui.spacing_mut().scroll.interact_handle_opacity = 0.85;
    ui.spacing_mut().scroll.active_handle_opacity = 0.9;
    ui.spacing_mut().scroll.bar_width = 8.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
}
