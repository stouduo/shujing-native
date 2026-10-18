//! 数镜设计令牌(严格对齐高保真原型):
//!
//! | 令牌 | 深色 | 浅色 |
//! | --- | --- | --- |
//! | 背景 systemBackground | `#000000` | `#F6F5F8` |
//! | 卡片 secondaryBackground | `#1C1C1E` | `#FFFFFF` |
//! | 凸起 tertiaryBackground | `#2C2C2E` | `#ECEBEF` |
//! | 分隔 separator | `#38383A` | `#D7D7D9` |
//! | 强调 systemBlue | `#0A84FF` | `#007AFF` |
//! | 成功 systemGreen | `#30D158` | `#248A3D` |
//! | 危险 systemRed | `#FF453A` | `#D70015` |
//!
//! 排版:表名 15 / 连接 14 / 列头 12.5 / 数据格 12 等宽 / 辅助 11。
//! 节奏:行高 32、工具条 44、标签栏 38、列头 36、状态栏 34。圆角 6/8/9/12。

use egui::{style::Selection, Color32, FontFamily, FontId, RichText, Stroke, TextStyle};

/// 粗体家族(原型标题/连接名/按钮一律半粗以上)
pub const BOLD: &str = "bold";
/// 等宽粗体家族(表头等)
pub const MONO_BOLD: &str = "mono-bold";

// ---------------------------------------------------------------- 排版节奏

pub const FS_TABLE_TITLE: f32 = 15.0;
pub const FS_CONN: f32 = 14.0;
pub const FS_TREE: f32 = 13.0;
pub const FS_TAB: f32 = 13.0;
pub const FS_HEADER: f32 = 12.5;
pub const FS_DATA: f32 = 12.0;
pub const FS_AUX: f32 = 11.0;

pub const TABBAR_H: f32 = 38.0;
pub const TOOLBAR_H: f32 = 44.0;
pub const HEADER_H: f32 = 36.0;
pub const ROW_H: f32 = 32.0;
pub const TREE_ROW_H: f32 = 34.0;
pub const FOOTER_H: f32 = 34.0;

/// 中文字形相对拉丁基线的视觉偏移系数(冬青黑体回退时整体偏高)
pub const CJK_BASELINE_FACTOR: f32 = 0.25;

#[derive(Clone, Copy)]
pub struct Palette {
    // 表面
    pub bg: Color32,
    pub panel: Color32,
    pub elevated: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub row_line: Color32,
    // 文字
    pub text: Color32,
    pub text2: Color32,
    pub muted: Color32,
    pub cell_text: Color32,
    pub null_text: Color32,
    // 语义
    pub accent: Color32,
    pub green: Color32,
    pub red: Color32,
    // 行状态(已按原型透明度预乘到背景)
    pub row_hover: Color32,
    pub active_row: Color32,
    pub selected_row: Color32,
    // 其它
    pub change_bg: Color32,
    pub change_fg: Color32,
    pub dot_off: Color32,
    pub stripe: Color32,
    pub input_bg: Color32,
    pub overlay: Color32,
    pub shadow: Color32,
    pub sql_keyword: Color32,
    pub sql_string: Color32,
    pub sql_number: Color32,
    pub sql_comment: Color32,
}

pub static DARK: Palette = Palette {
    // 石墨蓝:比纯黑柔和,卡片与背景有层次但不生硬
    bg: Color32::from_rgb(0x0c, 0x0e, 0x12),
    panel: Color32::from_rgb(0x15, 0x18, 0x1e),
    elevated: Color32::from_rgb(0x1f, 0x23, 0x2a),
    hover: Color32::from_rgb(0x26, 0x2b, 0x33),
    border: Color32::from_rgb(0x2a, 0x2f, 0x38),
    row_line: Color32::from_rgb(0x1b, 0x1f, 0x25),
    text: Color32::from_rgb(0xed, 0xef, 0xf3),
    text2: Color32::from_rgb(0x9a, 0xa0, 0xaa),
    muted: Color32::from_rgb(0x6a, 0x70, 0x7a),
    cell_text: Color32::from_rgb(0xe8, 0xea, 0xee),
    null_text: Color32::from_rgb(0x5a, 0x61, 0x6c),
    accent: Color32::from_rgb(0x5b, 0x8c, 0xff),
    green: Color32::from_rgb(0x3d, 0xd6, 0x8c),
    red: Color32::from_rgb(0xef, 0x44, 0x44),
    row_hover: Color32::from_rgba_premultiplied(7, 7, 7, 7),
    active_row: Color32::from_rgb(0x16, 0x25, 0x3c),
    selected_row: Color32::from_rgb(0x12, 0x1a, 0x29),
    change_bg: Color32::from_rgb(0x10, 0x2a, 0x1d),
    change_fg: Color32::from_rgb(0x3d, 0xd6, 0x8c),
    dot_off: Color32::from_rgb(0x4b, 0x52, 0x5c),
    stripe: Color32::from_rgb(0x10, 0x13, 0x18),
    input_bg: Color32::from_rgb(0x15, 0x18, 0x1e),
    overlay: Color32::from_black_alpha(150),
    shadow: Color32::from_black_alpha(180),
    sql_keyword: Color32::from_rgb(0x7b, 0xa7, 0xff),
    sql_string: Color32::from_rgb(0x5e, 0xea, 0xd4),
    sql_number: Color32::from_rgb(0xfb, 0xbf, 0x24),
    sql_comment: Color32::from_rgb(0x6a, 0x70, 0x7a),
};

pub static LIGHT: Palette = Palette {
    // 雾白:冷调浅灰底 + 纯白卡片,选中用低饱和蓝
    bg: Color32::from_rgb(0xf4, 0xf6, 0xf9),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    elevated: Color32::from_rgb(0xea, 0xed, 0xf2),
    hover: Color32::from_rgb(0xe2, 0xe6, 0xec),
    border: Color32::from_rgb(0xd9, 0xde, 0xe6),
    row_line: Color32::from_rgb(0xe7, 0xeb, 0xf0),
    text: Color32::from_rgb(0x1b, 0x1f, 0x26),
    text2: Color32::from_rgb(0x5f, 0x66, 0x73),
    muted: Color32::from_rgb(0x8a, 0x92, 0x9e),
    cell_text: Color32::from_rgb(0x1b, 0x1f, 0x26),
    null_text: Color32::from_rgb(0xa6, 0xad, 0xb8),
    accent: Color32::from_rgb(0x3b, 0x82, 0xf6),
    green: Color32::from_rgb(0x16, 0xa3, 0x4a),
    red: Color32::from_rgb(0xdc, 0x26, 0x26),
    row_hover: Color32::from_rgba_premultiplied(0, 0, 0, 5),
    active_row: Color32::from_rgb(0xdf, 0xea, 0xff),
    selected_row: Color32::from_rgb(0xea, 0xf1, 0xff),
    change_bg: Color32::from_rgb(0xe7, 0xf8, 0xee),
    change_fg: Color32::from_rgb(0x15, 0x80, 0x3d),
    dot_off: Color32::from_rgb(0xc3, 0xc9, 0xd2),
    stripe: Color32::from_rgb(0xee, 0xf1, 0xf6),
    input_bg: Color32::from_rgb(0xff, 0xff, 0xff),
    overlay: Color32::from_black_alpha(64),
    shadow: Color32::from_black_alpha(45),
    sql_keyword: Color32::from_rgb(0x25, 0x63, 0xeb),
    sql_string: Color32::from_rgb(0x0f, 0x76, 0x6e),
    sql_number: Color32::from_rgb(0xc2, 0x41, 0x0c),
    sql_comment: Color32::from_rgb(0x8a, 0x92, 0x9e),
};

static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static ACCENT_IDX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 强调色板(与 dbclient 外观设置一致;index 0 = 默认)
pub const ACCENTS: &[&str] = &["Indigo", "Violet", "Cyan", "Emerald", "Amber", "Rose"];
const ACCENT_DARK: [Color32; 6] = [
    Color32::from_rgb(0x8a, 0x8c, 0xfa),
    Color32::from_rgb(0xA7, 0x8B, 0xFA),
    Color32::from_rgb(0x4D, 0xD0, 0xE1),
    Color32::from_rgb(0x4E, 0xC9, 0x8A),
    Color32::from_rgb(0xEE, 0xC1, 0x7E),
    Color32::from_rgb(0xF2, 0x77, 0x8C),
];
const ACCENT_LIGHT: [Color32; 6] = [
    Color32::from_rgb(0x5E, 0x5F, 0xE0),
    Color32::from_rgb(0x7C, 0x5C, 0xE0),
    Color32::from_rgb(0x08, 0x91, 0xB2),
    Color32::from_rgb(0x16, 0x80, 0x3F),
    Color32::from_rgb(0xB4, 0x5A, 0x09),
    Color32::from_rgb(0xD8, 0x3B, 0x54),
];

pub fn set_dark(dark: bool) {
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn accent_color_dark(i: usize) -> Color32 {
    ACCENT_DARK[i.min(ACCENT_DARK.len() - 1)]
}

pub fn accent_color_light(i: usize) -> Color32 {
    ACCENT_LIGHT[i.min(ACCENT_LIGHT.len() - 1)]
}

pub fn set_accent(idx: usize) {
    ACCENT_IDX.store(
        idx.min(ACCENTS.len() - 1),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn accent_idx() -> usize {
    ACCENT_IDX.load(std::sync::atomic::Ordering::Relaxed)
}

/// 当前调色板(即时模式每帧读取,切换即时生效;按值返回以支持强调色替换)
pub fn c() -> Palette {
    let mut pal = if is_dark() { DARK } else { LIGHT };
    let idx = ACCENT_IDX.load(std::sync::atomic::Ordering::Relaxed);
    if idx > 0 {
        pal.accent = if is_dark() {
            ACCENT_DARK[idx.min(ACCENT_DARK.len() - 1)]
        } else {
            ACCENT_LIGHT[idx.min(ACCENT_LIGHT.len() - 1)]
        };
    }
    pal
}

/// 语义色淡底(徽章/选中);保持与原型一致的 16~20% 透明度
pub fn tint(color: Color32, alpha: u8) -> Color32 {
    // 按 sRGB 预乘(与调色板常量一致):egui 的线性混合下不会把 16% 淡底放大成高饱和色
    let a = alpha as u16;
    let scale = |v: u8| ((v as u16 * a) / 255) as u8;
    Color32::from_rgba_premultiplied(scale(color.r()), scale(color.g()), scale(color.b()), alpha)
}

// ---------------------------------------------------------------- 字体

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// 粗体标签(原型里的标题/连接名/按钮)
pub fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(bold(size))
}

pub fn apply(ctx: &egui::Context) {
    let p = c();
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = is_dark();
    v.panel_fill = p.bg;
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.input_bg;
    v.window_fill = p.panel;
    v.override_text_color = Some(p.text);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = Color32::from_rgb(0xff, 0xd6, 0x0a);
    v.error_fg_color = p.red;
    v.selection = Selection {
        bg_fill: tint(p.accent, 70),
        stroke: Stroke::NONE,
    };
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: p.shadow,
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: p.shadow,
    };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.corner_radius = egui::CornerRadius::same(8);
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text2);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.corner_radius = egui::CornerRadius::same(8);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.corner_radius = egui::CornerRadius::same(8);
    w.active.bg_fill = tint(p.accent, 56);
    w.active.weak_bg_fill = tint(p.accent, 56);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.bg_stroke = Stroke::NONE;
    w.active.corner_radius = egui::CornerRadius::same(8);
    w.open.bg_fill = p.elevated;
    w.open.weak_bg_fill = p.elevated;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.bg_stroke = Stroke::new(1.0, p.border);
    w.open.corner_radius = egui::CornerRadius::same(8);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = egui::Margin::symmetric(6, 5);
    style.spacing.interact_size.y = 26.0;
    style.spacing.scroll = thin_scroll(&p);
    style.spacing.text_edit_width = 220.0;

    style.text_styles = [
        (TextStyle::Heading, bold(15.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Monospace, mono(12.0)),
    ]
    .into();

    ctx.set_style(style);
}

fn thin_scroll(p: &Palette) -> egui::style::ScrollStyle {
    let mut s = egui::style::ScrollStyle::thin();
    s.floating = true;
    s.bar_width = 8.0;
    s.floating_width = 5.0;
    s.handle_min_length = 24.0;
    s.bar_inner_margin = 3.0;
    s.bar_outer_margin = 0.0;
    s.floating_allocated_width = 0.0;
    s.foreground_color = false;
    s.dormant_handle_opacity = 0.0;
    s.dormant_background_opacity = 0.0;
    s.active_handle_opacity = 0.55;
    s.active_background_opacity = 0.12;
    s.interact_handle_opacity = 0.55;
    s.interact_background_opacity = 0.12;
    let _ = p;
    s
}

// ---------------------------------------------------------------- 发丝线

/// 发丝分隔线:画在当前 ui 底边,横向铺满
pub fn hairline_bottom(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [
            egui::pos2(r.left(), r.bottom() - 0.5),
            egui::pos2(r.right(), r.bottom() - 0.5),
        ],
        Stroke::new(1.0, c().border),
    );
}

/// 发丝分隔线:画在当前光标处(弹窗标题/底栏用;避免 max_rect 被拉伸时跑位)
pub fn hairline_at_cursor(ui: &egui::Ui) {
    let r = ui.max_rect();
    let y = ui.cursor().top() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        Stroke::new(1.0, c().border),
    );
}

/// 发丝分隔线:画在当前 ui 顶边
pub fn hairline_top(ui: &egui::Ui) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [
            egui::pos2(r.left(), r.top() + 0.5),
            egui::pos2(r.right(), r.top() + 0.5),
        ],
        Stroke::new(1.0, c().border),
    );
}

// ---------------------------------------------------------------- 按钮

#[derive(Clone, Copy, PartialEq)]
pub enum BtnKind {
    /// 系统蓝填充(主操作)
    Primary,
    /// 系统绿填充(保存)
    Success,
    /// 系统红填充(危险操作)
    Danger,
    /// 描边(次要操作,如"刷新")
    Secondary,
}

impl BtnKind {
    fn colors(self) -> (Color32, Color32, Option<(Color32, Color32)>) {
        let p = c();
        match self {
            BtnKind::Primary => (p.accent, Color32::WHITE, None),
            BtnKind::Success => {
                if is_dark() {
                    (p.green, Color32::from_rgb(0x04, 0x1f, 0x14), None)
                } else {
                    (p.green, Color32::WHITE, None)
                }
            }
            BtnKind::Danger => (p.red, Color32::WHITE, None),
            BtnKind::Secondary => (p.bg, p.text2, Some((p.border, p.muted))),
        }
    }
}

/// 按钮:主文字 + 可选快捷键副符号(如 "保存"+"⌘S")。
/// 自绘保证文字/符号同一基线,返回响应(点击即触发)。
pub fn button(ui: &mut egui::Ui, label: &str, kind: BtnKind) -> egui::Response {
    button_ex(ui, label, None, kind, 28.0, None)
}

/// 带快捷键的按钮
pub fn button_shortcut(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    kind: BtnKind,
) -> egui::Response {
    button_ex(ui, label, Some(shortcut), kind, 28.0, None)
}

/// 指定高度/最小宽度的按钮(工具栏用)
pub fn button_ex(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<&str>,
    kind: BtnKind,
    height: f32,
    min_width: Option<f32>,
) -> egui::Response {
    let (fill, fg, stroke) = kind.colors();
    let p = c();
    let main_font = bold(12.5);
    let sub_font = bold(11.0);
    let (sub_color, stroke_color) = match stroke {
        Some((border, sub)) => (sub, border),
        None => {
            let sub = match kind {
                BtnKind::Primary | BtnKind::Danger => tint(Color32::WHITE, 170),
                BtnKind::Success => {
                    if is_dark() {
                        tint(Color32::BLACK, 130)
                    } else {
                        tint(Color32::WHITE, 170)
                    }
                }
                _ => p.muted,
            };
            (sub, Color32::TRANSPARENT)
        }
    };
    let main_g =
        ui.painter()
            .layout_no_wrap(label.to_owned(), main_font.clone(), Color32::PLACEHOLDER);
    let sub_g = shortcut.map(|s| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), sub_font.clone(), Color32::PLACEHOLDER)
    });
    let pad_x = 13.0;
    let mut w = pad_x * 2.0 + main_g.size().x;
    if let Some(g) = &sub_g {
        w += 8.0 + g.size().x;
    }
    if let Some(mw) = min_width {
        w = w.max(mw);
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::click());
    let painter = ui.painter();
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    let fill_now = if hovered {
        if fill == Color32::TRANSPARENT {
            p.hover
        } else {
            fill.linear_multiply(0.92)
        }
    } else if pressed {
        fill.linear_multiply(0.85)
    } else {
        fill
    };
    painter.rect_filled(rect, egui::CornerRadius::same(9), fill_now);
    if stroke_color != Color32::TRANSPARENT {
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(9),
            Stroke::new(1.0, if hovered { p.hover } else { stroke_color }),
            egui::StrokeKind::Inside,
        );
    }
    let cy = rect.center().y;
    let mut x = rect.left() + pad_x;
    painter.galley(
        egui::pos2(x, cy - main_g.size().y / 2.0),
        main_g.clone(),
        fg,
    );
    x += main_g.size().x;
    if let (Some(g), Some(s)) = (&sub_g, shortcut) {
        x += 8.0;
        painter.galley(egui::pos2(x, cy - g.size().y / 2.0), g.clone(), sub_color);
        let _ = s;
    }
    resp
}

/// 自绘复选框:当前主题把通用按钮的描边关掉了,egui 默认 Checkbox 的方框会不可见
pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    let p = c();
    let enabled = ui.is_enabled();
    let font = egui::FontId::proportional(11.5);
    let g = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::PLACEHOLDER);
    let label_w = if label.is_empty() {
        0.0
    } else {
        8.0 + g.size().x
    };
    let (rect, mut resp) =
        ui.allocate_exact_size(egui::vec2(15.0 + label_w, 18.0), egui::Sense::click());
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 7.5, rect.center().y),
        egui::Vec2::splat(15.0),
    );
    let (fill, stroke, fg) = if !enabled {
        (p.bg, p.border, p.muted)
    } else if *checked {
        (p.accent, p.accent, Color32::WHITE)
    } else {
        (
            p.bg,
            if resp.hovered() { p.accent } else { p.border },
            p.text2,
        )
    };
    ui.painter()
        .rect_filled(box_rect, egui::CornerRadius::same(4), fill);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if *checked {
        let c = box_rect.center();
        let st = egui::Stroke::new(1.6, fg);
        ui.painter().line_segment(
            [egui::pos2(c.x - 3.4, c.y), egui::pos2(c.x - 0.8, c.y + 2.6)],
            st,
        );
        ui.painter().line_segment(
            [
                egui::pos2(c.x - 0.8, c.y + 2.6),
                egui::pos2(c.x + 3.6, c.y - 2.9),
            ],
            st,
        );
    }
    if !label.is_empty() {
        ui.painter().text(
            egui::pos2(box_rect.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            if !enabled {
                p.muted
            } else if resp.hovered() {
                p.text
            } else {
                p.text2
            },
        );
    }
    if enabled && resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    resp
}

/// 次级小文字按钮(悬停行操作),带语义色;返回 (是否点击, 响应)
pub fn mini_text_button(ui: &mut egui::Ui, label: &str, color: Color32) -> (bool, egui::Response) {
    let r = ui.add(
        egui::Button::new(RichText::new(label).size(11.0).color(color))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(egui::vec2(0.0, 20.0)),
    );
    (r.clicked(), r)
}

// ---------------------------------------------------------------- 输入框

/// 表单输入框:圆角卡片底 + 描边,聚焦时系统蓝描边
pub fn field(
    ui: &mut egui::Ui,
    id_salt: &str,
    text: &mut String,
    width: f32,
    pwd: bool,
) -> egui::Response {
    let h = 30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let p = c();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.input_bg);
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.top() + 2.0),
        egui::pos2(rect.right() - 10.0, rect.bottom() - 2.0),
    );
    let resp = ui
        .allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                let mut te = egui::TextEdit::singleline(text)
                    .id(egui::Id::new(id_salt))
                    .frame(false)
                    .margin(egui::Margin::ZERO)
                    .font(egui::FontId::proportional(12.5))
                    .desired_width(inner.width());
                if pwd {
                    te = te.password(true);
                }
                ui.add(te)
            },
        )
        .inner;
    let border = if resp.has_focus() { p.accent } else { p.border };
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    resp
}

/// 表单输入框(带上置标签)
pub fn form_field(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: &str,
    text: &mut String,
    width: f32,
    pwd: bool,
) {
    ui.label(RichText::new(label).size(11.0).color(c().muted));
    ui.add_space(4.0);
    field(ui, id_salt, text, width, pwd);
}

/// 侧栏/表格搜索框:卡片底 + 放大镜 + 无框输入
pub fn search_box(
    ui: &mut egui::Ui,
    id_salt: &str,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    input_with_icon(
        ui,
        id_salt,
        text,
        hint,
        width,
        30.0,
        InputIcon::Search,
        true,
    )
}

/// 工具栏筛选框:背景底 + 漏斗 + 无框输入(原型"筛选行")
pub fn filter_box(
    ui: &mut egui::Ui,
    id_salt: &str,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    input_with_icon(
        ui,
        id_salt,
        text,
        hint,
        width,
        28.0,
        InputIcon::Funnel,
        false,
    )
}

#[derive(Clone, Copy)]
enum InputIcon {
    Search,
    Funnel,
}

fn input_with_icon(
    ui: &mut egui::Ui,
    id_salt: &str,
    text: &mut String,
    hint: &str,
    width: f32,
    height: f32,
    icon: InputIcon,
    card: bool,
) -> egui::Response {
    let p = c();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let fill = if card { p.input_bg } else { p.bg };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    match icon {
        InputIcon::Search => crate::icons::magnifier(
            ui.painter(),
            egui::pos2(rect.left() + 14.0, rect.center().y),
            p.muted,
        ),
        InputIcon::Funnel => crate::icons::funnel_glyph(
            ui.painter(),
            egui::pos2(rect.left() + 14.0, rect.center().y),
            p.muted,
        ),
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 28.0, rect.top() + 1.0),
        egui::pos2(rect.right() - 10.0, rect.bottom() - 1.0),
    );
    let resp = ui
        .allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .id(egui::Id::new(id_salt))
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .hint_text(RichText::new(hint).size(12.5).color(p.muted))
                        .font(egui::FontId::proportional(12.5))
                        .desired_width(inner.width()),
                )
            },
        )
        .inner;
    let border = if resp.has_focus() {
        p.accent
    } else if !card {
        p.border
    } else {
        p.border
    };
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    resp
}

// ---------------------------------------------------------------- 下拉

/// iOS 式下拉选择:圆角输入框样式 + 弹层列表(选中蓝底/悬停浅蓝)
/// 返回 Some(新值) 当用户选中;options = [(值, 显示)]
pub fn select(
    ui: &mut egui::Ui,
    id: &str,
    selected: &str,
    options: &[(String, String)],
    width: f32,
) -> Option<String> {
    let p = c();
    let popup_id = egui::Id::new(id).with("popup");
    let opened = ui.memory(|m| m.is_popup_open(popup_id));
    let h = 30.0;

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(8), p.bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(
            1.0,
            if resp.hovered() || opened {
                p.accent
            } else {
                p.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        selected,
        egui::FontId::proportional(12.5),
        p.text,
    );
    let tri = egui::pos2(rect.right() - 14.0, rect.center().y - 1.0);
    let s3 = 3.2;
    let pts: Vec<egui::Pos2> = if opened {
        vec![
            egui::pos2(tri.x - s3, tri.y + s3 / 2.0),
            egui::pos2(tri.x + s3, tri.y + s3 / 2.0),
            egui::pos2(tri.x, tri.y - s3),
        ]
    } else {
        vec![
            egui::pos2(tri.x - s3, tri.y - s3 / 2.0),
            egui::pos2(tri.x + s3, tri.y - s3 / 2.0),
            egui::pos2(tri.x, tri.y + s3),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, p.muted, Stroke::NONE));

    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut picked = None;
    if opened {
        egui::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_min_width(width);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (val, label) in options {
                    let is_sel = val == selected;
                    let item =
                        egui::Button::new(RichText::new(label).size(12.5).color(if is_sel {
                            p.text
                        } else {
                            p.text2
                        }))
                        .fill(if is_sel {
                            tint(p.accent, 48)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(width - 20.0, 26.0));
                    if ui.add(item).clicked() {
                        picked = Some(val.clone());
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
            },
        );
    }
    picked
}

// ---------------------------------------------------------------- 弹窗

/// 统一弹窗骨架:iOS sheet 式(标题行 + 发丝线 + 内容 + 发丝线 + 底栏)
pub fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(c().panel)
        .stroke(Stroke::new(1.0, c().border))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 16,
            bottom: 14,
        })
}

/// 弹窗标题行(左标题 + 底部发丝线)
pub fn modal_title(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(strong(title, 15.0).color(c().text));
    });
    ui.add_space(10.0);
    hairline_at_cursor(ui);
    ui.add_space(16.0);
}

/// 弹窗底栏容器:先画顶部发丝线再进入右对齐布局
pub fn modal_footer(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(16.0);
    hairline_at_cursor(ui);
    ui.add_space(12.0);
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 30.0),
        egui::Layout::right_to_left(egui::Align::Center),
        add_buttons,
    );
    ui.add_space(2.0);
}
