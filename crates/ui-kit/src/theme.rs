//! 数镜设计令牌(对齐 DBX 默认主题的中性灰体系):
//!
//! | 令牌 | 深色 | 浅色 |
//! | --- | --- | --- |
//! | 内容区 bg | `#131416` | `#FFFFFF` |
//! | 侧栏 panel | `#19191C` | `#FAFAFA` |
//! | 外框 chrome(标签栏/顶栏) | `#1B1B1E` | `#F5F5F5` |
//! | 网格列头 header | `#202022` | `#EFEFEF` |
//! | 悬停 hover | `#2E2F33` | `#F5F5F5` |
//! | 斑马纹 stripe | `#28282B` | `#F0F0F0` |
//! | 分隔 border | `#2C2D30` | `#E5E5E5` |
//! | 主文字 text | `#D7D7DB` | `#0A0A0A` |
//! | 次文字 text2 | `#97989D` | `#737373` |
//! | 强调(选中/焦点) | `#60A5FA` | `#2563EB` |
//! | 成功 | `#4ADE80` | `#16A34A` |
//! | 危险 | `#F3625F` | `#E7000B` |
//! | 主按钮(反色实心) | `#D0D0D6`/`#131416` | `#171717`/`#FAFAFA` |
//!
//! 排版:表名 15 / 连接 14 / 树 14 / 标签 12 / 列头 13 粗 + 类型 10 / 数据格 13 / 辅助 11。
//! 节奏:行高 30、工具条 44、标签栏 38、通用列头 36、数据列头 52、状态栏 34。圆角 6/8。

use egui::{style::Selection, Color32, FontFamily, FontId, RichText, Stroke, TextStyle};

/// 粗体家族(原型标题/连接名/按钮一律半粗以上)
pub const BOLD: &str = "bold";
/// 等宽粗体家族(表头等)
pub const MONO_BOLD: &str = "mono-bold";

// ---------------------------------------------------------------- 排版节奏

pub const FS_TABLE_TITLE: f32 = 15.0;
pub const FS_CONN: f32 = 14.0;
/// 侧栏树(DBX 14px)
pub const FS_TREE: f32 = 14.0;
/// 标签文字(DBX 12px/500)
pub const FS_TAB: f32 = 12.0;
/// 列头名称(DBX 13px/600)
pub const FS_HEADER: f32 = 13.0;
/// 列头类型小字(DBX 10px)
pub const FS_TYPE: f32 = 10.0;
/// 数据格(DBX 13px 比例字体)
pub const FS_DATA: f32 = 13.0;
pub const FS_AUX: f32 = 11.0;

pub const TABBAR_H: f32 = 38.0;
pub const TOOLBAR_H: f32 = 44.0;
/// 通用结果网格的单行列头
pub const HEADER_H: f32 = 36.0;
/// 表数据网格列头:DBX 两行三段式(名称 16 + 类型 12 + 注释 12,上下内边距各 6)
pub const DATA_HEADER_H: f32 = 52.0;
/// 表页内容头(面包屑 + 动作)与查询栏(WHERE/ORDER BY + 动作),对齐 DBX 的 34/36
pub const CONTENT_HEAD_H: f32 = 34.0;
pub const QUERYBAR_H: f32 = 36.0;
/// 数据行高:对齐 DBX 的 29-30(此前 36 偏松,同屏行数少约 17%)
pub const ROW_H: f32 = 30.0;
pub const TREE_ROW_H: f32 = 34.0;
pub const FOOTER_H: f32 = 34.0;

/// 中文字形相对拉丁基线的视觉偏移系数(冬青黑体回退时整体偏高)
pub const CJK_BASELINE_FACTOR: f32 = 0.25;

#[derive(Clone, Copy)]
pub struct Palette {
    // 表面
    pub bg: Color32,
    pub panel: Color32,
    /// 外框色:标签栏/顶部工具栏(比侧栏略浮起一档)
    pub chrome: Color32,
    /// 网格列头底色
    pub header: Color32,
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
    // 主按钮:DBX 的反色实心(深色主题浅灰底,浅色主题近黑底)
    pub primary_bg: Color32,
    pub primary_fg: Color32,
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

pub const DARK: Palette = Palette {
    // DBX 默认暗色:纯中性灰四级表面(内容区 → 侧栏 → 外框 → 列头 → hover),
    // 无色相偏移;选中/hover 才带蓝
    bg: Color32::from_rgb(0x13, 0x14, 0x16),
    panel: Color32::from_rgb(0x19, 0x19, 0x1c),
    chrome: Color32::from_rgb(0x1b, 0x1b, 0x1e),
    header: Color32::from_rgb(0x20, 0x20, 0x22),
    elevated: Color32::from_rgb(0x1e, 0x1e, 0x20),
    hover: Color32::from_rgb(0x2e, 0x2f, 0x33),
    border: Color32::from_rgb(0x2c, 0x2d, 0x30),
    row_line: Color32::from_rgb(0x24, 0x25, 0x28),
    text: Color32::from_rgb(0xd7, 0xd7, 0xdb),
    text2: Color32::from_rgb(0x97, 0x98, 0x9d),
    muted: Color32::from_rgb(0x67, 0x68, 0x6d),
    cell_text: Color32::from_rgb(0xd7, 0xd7, 0xdb),
    null_text: Color32::from_rgb(0x74, 0x75, 0x7b),
    accent: Color32::from_rgb(0x60, 0xa5, 0xfa),
    green: Color32::from_rgb(0x4a, 0xde, 0x80),
    red: Color32::from_rgb(0xf3, 0x62, 0x5f),
    primary_bg: Color32::from_rgb(0xd0, 0xd0, 0xd6),
    primary_fg: Color32::from_rgb(0x13, 0x14, 0x16),
    row_hover: Color32::from_rgba_premultiplied(13, 13, 14, 15),
    active_row: Color32::from_rgb(0x1e, 0x40, 0x60),
    selected_row: Color32::from_rgb(0x1a, 0x33, 0x50),
    change_bg: Color32::from_rgb(0x13, 0x25, 0x21),
    change_fg: Color32::from_rgb(0x4a, 0xde, 0x80),
    dot_off: Color32::from_rgb(0x4a, 0x4b, 0x50),
    stripe: Color32::from_rgb(0x28, 0x28, 0x2b),
    input_bg: Color32::from_rgb(0x13, 0x14, 0x16),
    overlay: Color32::from_black_alpha(150),
    shadow: Color32::from_black_alpha(180),
    sql_keyword: Color32::from_rgb(0x7b, 0xa7, 0xff),
    sql_string: Color32::from_rgb(0x5e, 0xea, 0xd4),
    sql_number: Color32::from_rgb(0xfb, 0xbf, 0x24),
    sql_comment: Color32::from_rgb(0x6a, 0x70, 0x7a),
};

pub const LIGHT: Palette = Palette {
    // DBX 默认浅色:纯白内容区 + 近灰侧栏/外框,边框 #e5e5e5 一档到底
    bg: Color32::from_rgb(0xff, 0xff, 0xff),
    panel: Color32::from_rgb(0xfa, 0xfa, 0xfa),
    chrome: Color32::from_rgb(0xf5, 0xf5, 0xf5),
    header: Color32::from_rgb(0xef, 0xef, 0xef),
    elevated: Color32::from_rgb(0xff, 0xff, 0xff),
    hover: Color32::from_rgb(0xf5, 0xf5, 0xf5),
    border: Color32::from_rgb(0xe5, 0xe5, 0xe5),
    row_line: Color32::from_rgb(0xec, 0xec, 0xec),
    text: Color32::from_rgb(0x0a, 0x0a, 0x0a),
    text2: Color32::from_rgb(0x73, 0x73, 0x73),
    muted: Color32::from_rgb(0xa3, 0xa3, 0xa3),
    cell_text: Color32::from_rgb(0x0a, 0x0a, 0x0a),
    null_text: Color32::from_rgb(0x7a, 0x7a, 0x7a),
    accent: Color32::from_rgb(0x25, 0x63, 0xeb),
    green: Color32::from_rgb(0x16, 0xa3, 0x4a),
    red: Color32::from_rgb(0xe7, 0x00, 0x0b),
    primary_bg: Color32::from_rgb(0x17, 0x17, 0x17),
    primary_fg: Color32::from_rgb(0xfa, 0xfa, 0xfa),
    row_hover: Color32::from_rgba_premultiplied(1, 1, 1, 14),
    active_row: Color32::from_rgb(0xbf, 0xdb, 0xfe),
    selected_row: Color32::from_rgb(0xd3, 0xe3, 0xf5),
    change_bg: Color32::from_rgb(0xe7, 0xf8, 0xf2),
    change_fg: Color32::from_rgb(0x15, 0x80, 0x3d),
    dot_off: Color32::from_rgb(0xc9, 0xc9, 0xc9),
    stripe: Color32::from_rgb(0xf0, 0xf0, 0xf0),
    input_bg: Color32::from_rgb(0xff, 0xff, 0xff),
    overlay: Color32::from_black_alpha(64),
    shadow: Color32::from_black_alpha(45),
    sql_keyword: Color32::from_rgb(0x25, 0x63, 0xeb),
    sql_string: Color32::from_rgb(0x0f, 0x76, 0x6e),
    sql_number: Color32::from_rgb(0xc2, 0x41, 0x0c),
    sql_comment: Color32::from_rgb(0x8a, 0x92, 0x9e),
};

/// 主题预设:每个 = 明暗基调 + 表面配色(文字/语义色随基底派生)。
/// 0/1 是历史默认(石墨蓝/雾白);☀/☾ 手动切换在两个基调的默认预设间往返
pub struct ThemePreset {
    pub name_en: &'static str,
    pub name_zh: &'static str,
    pub dark: bool,
    pub palette: Palette,
}

const AMOLED: Palette = Palette {
    bg: Color32::from_rgb(0x00, 0x00, 0x00),
    panel: Color32::from_rgb(0x0b, 0x0d, 0x10),
    chrome: Color32::from_rgb(0x11, 0x11, 0x13),
    header: Color32::from_rgb(0x1a, 0x1a, 0x1c),
    elevated: Color32::from_rgb(0x15, 0x17, 0x1b),
    hover: Color32::from_rgb(0x1b, 0x1e, 0x23),
    border: Color32::from_rgb(0x20, 0x24, 0x29),
    row_line: Color32::from_rgb(0x0e, 0x10, 0x13),
    stripe: Color32::from_rgb(0x05, 0x06, 0x08),
    input_bg: Color32::from_rgb(0x0b, 0x0d, 0x10),
    active_row: Color32::from_rgb(0x0e, 0x1a, 0x2a),
    selected_row: Color32::from_rgb(0x0c, 0x14, 0x20),
    change_bg: Color32::from_rgb(0x06, 0x1a, 0x10),
    ..DARK
};

const JADE: Palette = Palette {
    bg: Color32::from_rgb(0x0a, 0x0f, 0x0c),
    panel: Color32::from_rgb(0x12, 0x1a, 0x15),
    chrome: Color32::from_rgb(0x15, 0x20, 0x1a),
    header: Color32::from_rgb(0x1c, 0x28, 0x21),
    elevated: Color32::from_rgb(0x1a, 0x24, 0x1d),
    hover: Color32::from_rgb(0x20, 0x2c, 0x25),
    border: Color32::from_rgb(0x25, 0x30, 0x28),
    row_line: Color32::from_rgb(0x0f, 0x17, 0x12),
    stripe: Color32::from_rgb(0x0c, 0x12, 0x0e),
    active_row: Color32::from_rgb(0x12, 0x27, 0x1b),
    selected_row: Color32::from_rgb(0x0f, 0x1f, 0x16),
    change_bg: Color32::from_rgb(0x0f, 0x24, 0x18),
    ..DARK
};

const OCEAN: Palette = Palette {
    bg: Color32::from_rgb(0x0a, 0x0e, 0x14),
    panel: Color32::from_rgb(0x10, 0x17, 0x20),
    chrome: Color32::from_rgb(0x13, 0x1b, 0x26),
    header: Color32::from_rgb(0x1a, 0x24, 0x30),
    elevated: Color32::from_rgb(0x18, 0x22, 0x2e),
    hover: Color32::from_rgb(0x1e, 0x2a, 0x37),
    border: Color32::from_rgb(0x23, 0x2f, 0x3c),
    row_line: Color32::from_rgb(0x0f, 0x15, 0x1d),
    stripe: Color32::from_rgb(0x0c, 0x11, 0x18),
    active_row: Color32::from_rgb(0x0e, 0x21, 0x30),
    selected_row: Color32::from_rgb(0x0c, 0x1a, 0x27),
    ..DARK
};

const WINE: Palette = Palette {
    bg: Color32::from_rgb(0x0f, 0x0b, 0x10),
    panel: Color32::from_rgb(0x17, 0x12, 0x1b),
    chrome: Color32::from_rgb(0x1a, 0x15, 0x20),
    header: Color32::from_rgb(0x22, 0x1b, 0x2b),
    elevated: Color32::from_rgb(0x1f, 0x19, 0x26),
    hover: Color32::from_rgb(0x26, 0x1f, 0x2e),
    border: Color32::from_rgb(0x2b, 0x23, 0x34),
    row_line: Color32::from_rgb(0x14, 0x0f, 0x18),
    stripe: Color32::from_rgb(0x0e, 0x0a, 0x12),
    active_row: Color32::from_rgb(0x22, 0x15, 0x30),
    selected_row: Color32::from_rgb(0x1a, 0x11, 0x23),
    ..DARK
};

const WARM_SAND: Palette = Palette {
    bg: Color32::from_rgb(0xf6, 0xf3, 0xee),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xf2, 0xee, 0xe8),
    header: Color32::from_rgb(0xef, 0xe9, 0xe0),
    elevated: Color32::from_rgb(0xef, 0xe9, 0xe0),
    hover: Color32::from_rgb(0xe7, 0xe0, 0xd5),
    border: Color32::from_rgb(0xde, 0xd6, 0xc9),
    row_line: Color32::from_rgb(0xec, 0xe6, 0xdc),
    stripe: Color32::from_rgb(0xf1, 0xec, 0xe4),
    input_bg: Color32::from_rgb(0xff, 0xff, 0xff),
    active_row: Color32::from_rgb(0xf2, 0xe7, 0xd6),
    selected_row: Color32::from_rgb(0xf7, 0xef, 0xe3),
    ..LIGHT
};

const ROSE_GRAY: Palette = Palette {
    bg: Color32::from_rgb(0xf3, 0xf2, 0xf5),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xef, 0xee, 0xf1),
    header: Color32::from_rgb(0xea, 0xe9, 0xed),
    elevated: Color32::from_rgb(0xe9, 0xe8, 0xee),
    hover: Color32::from_rgb(0xe0, 0xdf, 0xe6),
    border: Color32::from_rgb(0xd8, 0xd6, 0xe0),
    row_line: Color32::from_rgb(0xe6, 0xe5, 0xec),
    stripe: Color32::from_rgb(0xef, 0xed, 0xf3),
    active_row: Color32::from_rgb(0xe6, 0xe0, 0xf5),
    selected_row: Color32::from_rgb(0xef, 0xea, 0xf7),
    ..LIGHT
};

const MINT: Palette = Palette {
    bg: Color32::from_rgb(0xf0, 0xf5, 0xf3),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xec, 0xf2, 0xef),
    header: Color32::from_rgb(0xe5, 0xee, 0xea),
    elevated: Color32::from_rgb(0xe2, 0xec, 0xe8),
    hover: Color32::from_rgb(0xd8, 0xe5, 0xe0),
    border: Color32::from_rgb(0xcd, 0xdb, 0xd6),
    row_line: Color32::from_rgb(0xe1, 0xeb, 0xe7),
    stripe: Color32::from_rgb(0xea, 0xf2, 0xef),
    active_row: Color32::from_rgb(0xd8, 0xec, 0xdf),
    selected_row: Color32::from_rgb(0xe6, 0xf3, 0xea),
    ..LIGHT
};

/// 主题预设表(index 即持久化 id;0=石墨蓝深 1=雾白浅 为历史默认)
pub const PRESETS: [ThemePreset; 9] = [
    ThemePreset { name_en: "Graphite", name_zh: "石墨蓝", dark: true, palette: DARK },
    ThemePreset { name_en: "Fog White", name_zh: "雾白", dark: false, palette: LIGHT },
    ThemePreset { name_en: "AMOLED Black", name_zh: "纯黑", dark: true, palette: AMOLED },
    ThemePreset { name_en: "Jade", name_zh: "墨玉", dark: true, palette: JADE },
    ThemePreset { name_en: "Deep Ocean", name_zh: "深海", dark: true, palette: OCEAN },
    ThemePreset { name_en: "Wine", name_zh: "绛紫", dark: true, palette: WINE },
    ThemePreset { name_en: "Warm Sand", name_zh: "暖沙", dark: false, palette: WARM_SAND },
    ThemePreset { name_en: "Rose Gray", name_zh: "玫瑰灰", dark: false, palette: ROSE_GRAY },
    ThemePreset { name_en: "Mint", name_zh: "薄荷", dark: false, palette: MINT },
];

static PRESET_IDX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
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

/// 切换预设(index 越界取模保护);持久化 id 即 PRESETS 下标
pub fn set_preset(i: usize) {
    PRESET_IDX.store(i.min(PRESETS.len() - 1), std::sync::atomic::Ordering::Relaxed);
}

pub fn preset_idx() -> usize {
    PRESET_IDX.load(std::sync::atomic::Ordering::Relaxed)
}

/// ☀/☾ 语义的明暗切换:目标基调与其当前预设不符时,切到该基调的默认预设
/// (0=石墨蓝深 / 1=雾白浅)。预设不换基调时是 no-op
pub fn set_dark(dark: bool) {
    let cur = PRESETS[preset_idx()].dark;
    if cur != dark {
        set_preset(if dark { 0 } else { 1 });
    }
}

pub fn is_dark() -> bool {
    PRESETS[preset_idx()].dark
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
    let mut pal = PRESETS[preset_idx()].palette;
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

// ---------------------------------------------------------------- 列类型分类着色

/// 列类型类别:列头的类型小字按类别着色(DBX 同款九类)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind {
    Integer,
    Numeric,
    String,
    Bool,
    Temporal,
    /// 半结构化:JSON/XML/数组/几何…
    Structured,
    /// 标识符:UUID
    Identifier,
    Binary,
    Other,
}

/// 把驱动上报的列类型(可带参数或修饰,如 `varchar(128)`、`int unsigned`、
/// `timestamp with time zone`)归入类别:取首个字母数字词(自然去掉长度/精度括号),
/// 再按关键字匹配
pub fn classify_type(ty: &str) -> TypeKind {
    let t = ty.trim().to_ascii_lowercase();
    let head: String = t
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .next()
        .unwrap_or_default()
        .to_string();
    match head.as_str() {
        "int" | "integer" | "tinyint" | "smallint" | "mediumint" | "bigint" | "int2" | "int4"
        | "int8" | "int16" | "int32" | "int64" | "serial" | "bigserial" | "smallserial"
        | "identity" => TypeKind::Integer,
        "bool" | "boolean" | "logical" => TypeKind::Bool,
        "decimal" | "numeric" | "dec" | "number" | "float" | "float4" | "float8" | "float32"
        | "float64" | "double" | "real" | "money" | "smallmoney" => TypeKind::Numeric,
        "date" | "datetime" | "datetime2" | "timestamp" | "timestamptz" | "datetimetz" | "time"
        | "timetz" | "year" | "interval" | "smalldatetime" | "datetimeoffset" => TypeKind::Temporal,
        "char" | "varchar" | "nchar" | "nvarchar" | "text" | "tinytext" | "mediumtext"
        | "longtext" | "clob" | "ntext" | "string" | "character" | "varchar2" | "nvarchar2"
        | "memo" => TypeKind::String,
        "json" | "jsonb" | "xml" | "array" | "hstore" | "struct" | "map" | "set" | "enum"
        | "geometry" | "geography" | "point" => TypeKind::Structured,
        "uuid" => TypeKind::Identifier,
        "binary" | "varbinary" | "blob" | "bytea" | "image" | "bit" | "raw" | "longblob"
        | "mediumblob" | "tinyblob" | "bytes" | "byte" | "varbyte" => TypeKind::Binary,
        _ => TypeKind::Other,
    }
}

/// 类别对应的类型小字颜色(DBX 明暗两套色板:浅色深而饱和,深色主题换亮阶)
pub fn type_color(kind: TypeKind) -> Color32 {
    let pairs: [(TypeKind, (u8, u8, u8), (u8, u8, u8)); 8] = [
        (TypeKind::Integer, (0x1d, 0x4e, 0xd8), (0x93, 0xc5, 0xfd)),
        (TypeKind::Numeric, (0x0e, 0x74, 0x90), (0x67, 0xe8, 0xf9)),
        (TypeKind::String, (0x16, 0x65, 0x34), (0x86, 0xef, 0xac)),
        (TypeKind::Bool, (0xc2, 0x41, 0x0c), (0xfd, 0xba, 0x74)),
        (TypeKind::Temporal, (0x7e, 0x22, 0xce), (0xd8, 0xb4, 0xfe)),
        (TypeKind::Structured, (0xbe, 0x18, 0x5d), (0xf9, 0xa8, 0xd4)),
        (TypeKind::Identifier, (0x92, 0x40, 0x0e), (0xfc, 0xd3, 0x4d)),
        (TypeKind::Binary, (0xb9, 0x1c, 0x1c), (0xfc, 0xa5, 0xa5)),
    ];
    for (k, light, dark) in pairs {
        if k == kind {
            let (r, g, b) = if is_dark() { dark } else { light };
            return Color32::from_rgb(r, g, b);
        }
    }
    c().text2
}

/// 查询栏 WHERE 芯片文字(DBX 蓝)
pub fn where_color() -> Color32 {
    if is_dark() {
        Color32::from_rgb(0x60, 0xa5, 0xfa)
    } else {
        Color32::from_rgb(0x25, 0x63, 0xeb)
    }
}

/// 查询栏 ORDER BY 芯片文字(DBX 橙)
pub fn order_color() -> Color32 {
    if is_dark() {
        Color32::from_rgb(0xfb, 0x92, 0x3c)
    } else {
        Color32::from_rgb(0xea, 0x58, 0x0c)
    }
}

/// 激活标签的薄荷绿淡底(DBX:青绿 16~24% 叠在外框色上,这里直接取叠加后的实色)
pub fn tab_active_bg() -> Color32 {
    if is_dark() {
        Color32::from_rgb(0x20, 0x48, 0x42)
    } else {
        Color32::from_rgb(0xd5, 0xed, 0xe8)
    }
}

/// 主键钥匙图标(DBX 橙)
pub fn pk_key_color() -> Color32 {
    if is_dark() {
        Color32::from_rgb(0xfb, 0x92, 0x3c)
    } else {
        Color32::from_rgb(0xf0, 0x84, 0x25)
    }
}

/// sRGB 线性混合:t=0 取 a,t=1 取 b(标签描边 = 文字色按比例掺进边框色)
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
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

/// 兜底字体安装:把库用到的 "bold"/"mono-bold" 族绑到 egui 内置字体。
/// 宿主应用通常安装自己的字体(中文/等宽)并覆盖之;不装也能先跑起来
/// (没装时 strong/bold 会 panic:FontFamily::Name("bold") is not bound)
pub fn install_fallback_fonts(ctx: &egui::Context) {
    let mut fd = egui::FontDefinitions::default();
    let prop = fd
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mono = fd
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    fd.families.insert(egui::FontFamily::Name(BOLD.into()), prop);
    fd.families
        .insert(egui::FontFamily::Name(MONO_BOLD.into()), mono);
    ctx.set_fonts(fd);
}

pub fn apply(ctx: &egui::Context) {
    // 点击位移容忍:egui 默认 6 逻辑px,点按带轻微滑动(触控板/魔术鼠标很常见)
    // 就被判 drag、release 不算 click → 全应用"点了没反应"。
    // 20px 覆盖手抖;不影响拖拽排序(阈值 48px)与拖选(用 primary_pressed/down)
    // 手抖容忍:位移>20px 才不算点击;按住 3s 内释放仍算点击(默认 0.8s 对
    // 慢点击/犹豫点击太苛刻,点 tab"没反应"的另一根源)
    ctx.options_mut(|o| {
        o.input_options.max_click_dist = 20.0;
        o.input_options.max_click_duration = 3.0;
    });
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
        bg_fill: tint(p.accent, 70),
        stroke: Stroke::NONE,
    };
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
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
    w.noninteractive.corner_radius = egui::CornerRadius::same(6);
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text2);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.corner_radius = egui::CornerRadius::same(6);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.corner_radius = egui::CornerRadius::same(6);
    w.active.bg_fill = tint(p.accent, 56);
    w.active.weak_bg_fill = tint(p.accent, 56);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.bg_stroke = Stroke::NONE;
    w.active.corner_radius = egui::CornerRadius::same(6);
    w.open.bg_fill = p.elevated;
    w.open.weak_bg_fill = p.elevated;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.bg_stroke = Stroke::new(1.0, p.border);
    w.open.corner_radius = egui::CornerRadius::same(6);

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
    /// 反色实心(DBX 主操作:深色主题浅灰底,浅色主题近黑底)
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
            BtnKind::Primary => (p.primary_bg, p.primary_fg, None),
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
                BtnKind::Primary => {
                    if is_dark() {
                        tint(p.primary_fg, 150)
                    } else {
                        tint(p.primary_fg, 160)
                    }
                }
                BtnKind::Danger => tint(Color32::WHITE, 170),
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
        } else if kind == BtnKind::Primary && !is_dark() {
            // 浅色主题的近黑主按钮:悬停应提亮(DBX 同款),乘法变暗会糊成一团
            fill.linear_multiply(1.6)
        } else {
            fill.linear_multiply(0.92)
        }
    } else if pressed {
        fill.linear_multiply(0.85)
    } else {
        fill
    };
    painter.rect_filled(rect, egui::CornerRadius::same(6), fill_now);
    if stroke_color != Color32::TRANSPARENT {
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(6),
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
    let h = 28.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let p = c();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), p.input_bg);
    // 描边与按钮同语义:输入框在工具栏/表单里有可辨边界
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
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

/// 多行表单输入框(Redis 新建 key 批量录入等):与 field() 同视觉,
/// 真多行——此前批量录入走 singleline,每行一条的数据全挤在一行里
pub fn form_field_multiline(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: &str,
    text: &mut String,
    width: f32,
    rows: usize,
) {
    let p = c();
    if !label.is_empty() {
        ui.label(RichText::new(label).size(11.0).color(p.muted));
        ui.add_space(4.0);
    }
    let line_h = 17.0;
    let h = (rows.max(2) as f32 * line_h + 12.0).min(260.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.input_bg);
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 10.0, rect.bottom() - 5.0),
    );
    // add_sized 钉死尺寸:TextEdit 按字体行高自撑会越过预留框压到下一个控件
    // (宿主字体恰好装得下时不显,默认字体 metrics 下必现),钉死后框内滚动
    let resp = ui
        .allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::TOP)),
            |ui| {
                ui.add_sized(
                    inner.size(),
                    egui::TextEdit::multiline(text)
                        .id(egui::Id::new(id_salt))
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .font(egui::FontId::monospace(12.0)),
                )
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
        28.0,
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
        if std::env::var("SHUJING_POPUP_DEBUG").is_ok() {
            eprintln!(
                "[popup] id={popup_id:?} widget={:?} parent_layer={:?} screen={:?}",
                resp.rect,
                ui.layer_id(),
                ui.input(|i| i.screen_rect)
            );
        }
        let _area_out = egui::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                if std::env::var("SHUJING_POPUP_DEBUG").is_ok() {
                    eprintln!("[popup-inner] max_rect={:?} layer={:?}", ui.max_rect(), ui.layer_id());
                }
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
        // 统一关闭钮:✕ 注入 Esc(egui::Modal 的 should_close 响应),
        // 各弹窗不必各自接实例——所有 modal 免费获得右上关闭
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let p = c();
            let r = ui.add(
                egui::Button::new(
                    egui::RichText::new("✕")
                        .size(12.5)
                        .color(if ui.rect_contains_pointer(ui.max_rect()) {
                            p.text2
                        } else {
                            p.muted
                        }),
                )
                .frame(false),
            );
            if r.clicked() {
                ui.ctx().input_mut(|i| {
                    i.events.push(egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: Some(egui::Key::Escape),
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    })
                });
            }
            r.on_hover_text("关闭 (Esc)");
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_type_strips_args_and_modifiers() {
        let cases = [
            ("INTEGER", TypeKind::Integer),
            ("varchar(128)", TypeKind::String),
            ("bigint unsigned", TypeKind::Integer),
            ("NUMERIC(10, 2)", TypeKind::Numeric),
            ("timestamp with time zone", TypeKind::Temporal),
            ("character varying", TypeKind::String),
            ("integer[]", TypeKind::Integer),
            ("DOUBLE PRECISION", TypeKind::Numeric),
            ("boolean", TypeKind::Bool),
            ("jsonb", TypeKind::Structured),
            ("uuid", TypeKind::Identifier),
            ("bytea", TypeKind::Binary),
            ("inet", TypeKind::Other),
            ("", TypeKind::Other),
        ];
        for (ty, want) in cases {
            assert_eq!(classify_type(ty), want, "type {ty:?}");
        }
    }

    #[test]
    fn type_color_falls_back_to_secondary_text() {
        assert_eq!(type_color(TypeKind::Other), c().text2);
        assert_ne!(type_color(TypeKind::Integer), type_color(TypeKind::String));
    }

    #[test]
    fn mix_blends_endpoints() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(100, 200, 255);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), Color32::from_rgb(50, 100, 128));
    }
}
