//! 滚动纪律:egui 默认滚轮行为的两处修正,任何"滚完立刻点"的界面都该启用。
//!
//! 1. 惯性尾巴:egui 把大步进滚轮(Point≥8)归入 unprocessed 分帧释放,
//!    停滚后内容还要滑 ~0.1-0.3s;期间任何点击的按下/松开会命中不同 widget
//!    而被 egui 静默取消(实测 11 帧漂移)。feed_raw_wheel 把 smooth 覆写为
//!    raw(当帧真实滚轮量),鼠标滚轮一步到位;触控板小步进 raw≈smooth 手感不变。
//! 2. 按压冻结:主键按住期间一律清零——按压中内容绝不能动。
//!
//! 用法:在 eframe App::update 开头(任何绘制之前)调用 feed_raw_wheel(ctx)。

/// 滚轮一步到位 + 按压冻结(全应用,任何绘制之前调用一次)
pub fn feed_raw_wheel(ctx: &egui::Context) {
    let _ = ctx.input_mut(|i| {
        let zooming = i.modifiers.ctrl || i.modifiers.mac_cmd || i.modifiers.command;
        if i.pointer.primary_down() || zooming {
            i.smooth_scroll_delta = egui::Vec2::ZERO;
        } else {
            i.smooth_scroll_delta = i.raw_scroll_delta;
        }
    });
}

/// 悬浮细滚动条设置(不占布局高度,静止半透明):适合标签栏/工具栏等矮条区域。
/// 注意:drag_to_scroll 需另行关闭(由调用方决定,内容自带拖拽手势时必须关)
pub fn floating_thin_scroll(ui: &mut egui::Ui) {
    ui.spacing_mut().scroll.floating = true;
    ui.spacing_mut().scroll.floating_width = 5.0;
    ui.spacing_mut().scroll.bar_width = 6.0;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.5;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.0;
    ui.spacing_mut().scroll.active_handle_opacity = 0.85;
}
