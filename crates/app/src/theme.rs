//! iOS(Cupertino)设计令牌:深/浅双调色板,运行时切换。
//! 色值取自 iOS 官方语义色(深色 secondarySystemBackground #1C1C1E / systemBlue #0A84FF;
//! 浅色 #F5F5F7 / #007AFF;语义绿红深浅各自成档)。

use egui::{style::Selection, Color32, FontId, Stroke, TextStyle};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub elevated: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub red: Color32,
    pub change_bg: Color32,
    pub change_fg: Color32,
    pub row_hover: Color32,
    pub cell_hover: Color32,
    pub cell_select: Color32,
    pub active_row: Color32,
    pub row_line: Color32,
    pub dot_off: Color32,
    pub null_text: Color32,
    pub stripe: Color32,
    pub input_bg: Color32,
    pub sql_keyword: Color32,
    pub sql_string: Color32,
    pub sql_number: Color32,
    pub sql_comment: Color32,
}

pub static DARK: Palette = Palette {
    bg: Color32::from_rgb(0x00, 0x00, 0x00),
    panel: Color32::from_rgb(0x1c, 0x1c, 0x1e),
    elevated: Color32::from_rgb(0x2c, 0x2c, 0x2e),
    hover: Color32::from_rgb(0x3a, 0x3a, 0x3c),
    border: Color32::from_rgb(0x38, 0x38, 0x3a),
    text: Color32::from_rgb(0xff, 0xff, 0xff),
    text2: Color32::from_rgb(0xc9, 0xc9, 0xcf),
    muted: Color32::from_rgb(0x77, 0x78, 0x7f),
    accent: Color32::from_rgb(0x0a, 0x84, 0xff),
    green: Color32::from_rgb(0x30, 0xd1, 0x58),
    red: Color32::from_rgb(0xff, 0x45, 0x3a),
    change_bg: Color32::from_rgb(0x0f, 0x2a, 0x1b),
    change_fg: Color32::from_rgb(0x7c, 0xe0, 0xa8),
    row_hover: Color32::from_rgba_premultiplied(16, 16, 18, 22),
    cell_hover: Color32::from_rgba_premultiplied(22, 22, 22, 24),
    cell_select: Color32::from_rgba_premultiplied(2, 29, 56, 64),
    active_row: Color32::from_rgba_premultiplied(2, 29, 56, 56),
    row_line: Color32::from_rgb(0x22, 0x22, 0x24),
    dot_off: Color32::from_rgb(0x55, 0x56, 0x5c),
    null_text: Color32::from_rgb(0x55, 0x55, 0x5a),
    stripe: Color32::from_rgb(0x1a, 0x1a, 0x1e),
    input_bg: Color32::from_rgb(0x14, 0x14, 0x16),
    sql_keyword: Color32::from_rgb(0x42, 0x9b, 0xff),
    sql_string: Color32::from_rgb(0x64, 0xd2, 0xff),
    sql_number: Color32::from_rgb(0xff, 0x9f, 0x0a),
    sql_comment: Color32::from_rgb(0x63, 0x63, 0x66),
};

pub static LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf5, 0xf5, 0xf7),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    elevated: Color32::from_rgb(0xe9, 0xe9, 0xeb),
    hover: Color32::from_rgb(0xe3, 0xe3, 0xe8),
    border: Color32::from_rgb(0xd8, 0xd8, 0xdc),
    text: Color32::from_rgb(0x1d, 0x1d, 0x1f),
    text2: Color32::from_rgb(0x55, 0x55, 0x5a),
    muted: Color32::from_rgb(0x8e, 0x8e, 0x93),
    accent: Color32::from_rgb(0x00, 0x7a, 0xff),
    green: Color32::from_rgb(0x24, 0x8a, 0x3d),
    red: Color32::from_rgb(0xd7, 0x00, 0x15),
    change_bg: Color32::from_rgb(0xe2, 0xf5, 0xea),
    change_fg: Color32::from_rgb(0x1e, 0x7a, 0x44),
    row_hover: Color32::from_rgba_premultiplied(0, 0, 0, 6),
    cell_hover: Color32::from_rgba_premultiplied(0, 0, 0, 12),
    cell_select: Color32::from_rgba_premultiplied(0, 122, 255, 46),
    active_row: Color32::from_rgba_premultiplied(0, 122, 255, 40),
    row_line: Color32::from_rgb(0xe8, 0xe8, 0xec),
    dot_off: Color32::from_rgb(0xc7, 0xc7, 0xcc),
    null_text: Color32::from_rgb(0xae, 0xae, 0xb2),
    stripe: Color32::from_rgb(0xec, 0xec, 0xf0),
    input_bg: Color32::from_rgb(0xe9, 0xe9, 0xeb),
    sql_keyword: Color32::from_rgb(0x00, 0x66, 0xcc),
    sql_string: Color32::from_rgb(0x1f, 0x7a, 0x44),
    sql_number: Color32::from_rgb(0xb2, 0x50, 0x00),
    sql_comment: Color32::from_rgb(0x8e, 0x8e, 0x93),
};

static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_dark(dark: bool) {
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 当前调色板(即时模式每帧读取,切换即时生效)
pub fn c() -> &'static Palette {
    if is_dark() { &DARK } else { &LIGHT }
}

pub fn apply(ctx: &egui::Context) {
    let p = c();
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = is_dark();
    v.panel_fill = p.bg;
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.input_bg;
    v.window_fill = p.elevated;
    v.override_text_color = Some(p.text);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = Color32::from_rgb(0xff, 0xd6, 0x0a);
    v.error_fg_color = p.red;
    v.selection = Selection {
        bg_fill: p.accent.gamma_multiply(0.25),
        stroke: Stroke::NONE,
    };

    let w = &mut style.visuals.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text2);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.active.bg_fill = p.accent.gamma_multiply(0.28);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.28);
    w.active.fg_stroke = Stroke::new(1.0, p.text);

    // iOS 圆角语言:控件 8,弹窗 12,菜单 10
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(10);

    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.menu_margin = egui::Margin::symmetric(6, 4);
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    ctx.set_style(style);
}

/// 统一弹窗骨架:iOS sheet 式(标题行 + 发丝线 + 内容 + 发丝线 + 底栏容器由调用方填)
pub fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(c().panel)
        .stroke(egui::Stroke::new(1.0, c().border))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 0 })
}

/// 弹窗标题行(左标题 + 右关闭返回值) + 底部发丝线
pub fn modal_title(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(14.0).strong());
    });
    ui.add_space(2.0);
    hairline_top(ui);
    ui.add_space(14.0);
}

/// 弹窗底栏容器:先画顶部发丝线再进入右对齐布局
pub fn modal_footer(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(14.0);
    hairline_top(ui);
    ui.add_space(10.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add_buttons);
    ui.add_space(12.0);
}

/// 弹窗主按钮(填充)
pub fn modal_primary(ui: &mut egui::Ui, label: &str, fill: egui::Color32) -> bool {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label).size(12.5).color(if fill == c().accent { c().text } else { Color32::WHITE }).strong(),
        )
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(9))
        .min_size(egui::vec2(84.0, 28.0)),
    )
    .clicked()
}

/// 弹窗次按钮(透明)
pub fn modal_secondary(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.5).color(c().text2))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .min_size(egui::vec2(64.0, 28.0)),
    )
    .clicked()
}

/// 字段:标签上置 + 统一宽输入框
pub fn form_field(ui: &mut egui::Ui, label: &str, text: &mut String, width: f32, pwd: bool) {
    ui.label(egui::RichText::new(label).size(11.0).color(c().muted));
    let mut te = egui::TextEdit::singleline(text)
        .desired_width(width)
        .font(egui::TextStyle::Body);
    if pwd {
        te = te.password(true);
    }
    ui.add(te);
}

/// 次级小文字按钮(悬停行操作),带语义色;返回 (是否点击, 响应)
pub fn mini_text_button(ui: &mut egui::Ui, label: &str, color: Color32) -> (bool, egui::Response) {
    let r = ui.add(
        egui::Button::new(egui::RichText::new(label).size(11.0).color(color))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(egui::vec2(0.0, 24.0)),
    );
    (r.clicked(), r)
}

/// iOS 发丝分隔线(separator):画在当前 ui 底边,横向铺满
pub fn hairline_bottom(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [egui::pos2(r.left() - 12.0, r.bottom()), egui::pos2(r.right() + 12.0, r.bottom())],
        Stroke::new(1.0, c().border),
    );
}

/// iOS 发丝分隔线:画在当前 ui 顶边
pub fn hairline_top(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [egui::pos2(r.left() - 12.0, r.top()), egui::pos2(r.right() + 12.0, r.top())],
        Stroke::new(1.0, c().border),
    );
}

/// iOS 导航栏式文字按钮(系统蓝、无底),返回是否点击
pub fn text_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(c().accent))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE),
    )
    .clicked()
}
