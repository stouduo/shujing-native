//! 开发工具箱标签:左侧工具导航 + 右侧工具页。工具都是"纯函数"型
//! (输入即输出,无网络/无后台任务),用 ui-kit 表单组件直接堆页面。

use crate::app::ToolboxTab;
use crate::table_view::TabAction;
use crate::theme;
use egui::RichText;
use shujing_core::hashing::Algo;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 工具条目:(分类, 名称)。分类只用于导航分组
const TOOLS: [(&str, &str, Tool); 6] = [
    ("编解码", "Base64 / URL / Hex", Tool::Codec),
    ("时间", "时间戳 ⇄ 日期", Tool::Timestamp),
    ("生成器", "UUID", Tool::Uuid),
    ("格式化", "JSON", Tool::Json),
    ("摘要", "哈希 / HMAC", Tool::Hash),
    ("令牌", "JWT", Tool::Jwt),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Codec,
    Timestamp,
    Uuid,
    Json,
    Hash,
    Jwt,
}

pub fn draw_toolbox_view(ctx: &egui::Context, tab_id: &str, tb: &mut ToolboxTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    theme::console_body_panel(ctx, |ui| {
        // 左侧工具导航
        egui::SidePanel::left(format!("toolbox-nav-{tab_id}"))
            .resizable(false)
            .default_width(190.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::ZERO))
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                let mut last_cat = "";
                for (cat, name, tool) in TOOLS {
                    if cat != last_cat {
                        if !last_cat.is_empty() {
                            ui.add_space(8.0);
                            theme::hairline_top(ui);
                            ui.add_space(4.0);
                        }
                        ui.label(
                            RichText::new(cat)
                                .size(theme::fs_px(10.5))
                                .color(theme::c().muted),
                        );
                        ui.add_space(2.0);
                        last_cat = cat;
                    }
                    let selected = tb.tool == tool;
                    if theme::menu_item_ex(ui, name, selected, false, true).clicked() {
                        tb.tool = tool;
                    }
                }
            });

        // 右侧工具页
        ui.add_space(6.0);
        let wide = (ui.available_width() - 16.0).clamp(420.0, 760.0);
        match tb.tool {
            Tool::Codec => draw_codec(ui, &mut tb.codec, wide),
            Tool::Timestamp => draw_timestamp(ui, &mut tb.ts),
            Tool::Uuid => draw_uuid(ui, &mut tb.uuid, wide),
            Tool::Json => draw_json(ui, &mut tb.json, wide),
            Tool::Hash => draw_hash(ui, tab_id, &mut tb.hash, wide, &mut actions),
            Tool::Jwt => draw_jwt(ui, &mut tb.jwt, wide, &mut actions),
        }
    });
    actions
}

// ---------------------------------------------------------------- 编解码

#[derive(Default, Clone)]
pub struct CodecTool {
    pub mode: usize,
    pub input: String,
    /// 上一次成功转换的输出(转换失败时保留,错误单独展示)
    pub output: String,
    pub error: Option<String>,
}

const CODEC_MODES: [&str; 6] = [
    "Base64 编码",
    "Base64 解码",
    "URL 编码",
    "URL 解码",
    "Hex 编码",
    "Unicode 转义",
];

fn codec_convert(mode: usize, input: &str) -> Result<String, String> {
    use base64::Engine as _;
    match mode {
        0 => Ok(base64::engine::general_purpose::STANDARD.encode(input.as_bytes())),
        1 => {
            let cleaned = input.replace(['\n', '\r', ' '], "");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(cleaned.as_bytes())
                .map_err(|e| format!("Base64 解码失败: {e}"))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        2 => Ok(url_encode(input)),
        3 => Ok(url_decode(input)),
        4 => Ok(input
            .bytes()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join("")),
        5 => Ok(input
            .chars()
            .map(|c| {
                if (c as u32) < 0x80 {
                    c.to_string()
                } else {
                    format!("\\u{{{:x}}}", c as u32)
                }
            })
            .collect()),
        _ => Err("未知模式".into()),
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 输入即输出:每帧重算,失败保留上一次输出并把错误亮出来
fn draw_codec(ui: &mut egui::Ui, t: &mut CodecTool, wide: f32) {
    let p = theme::c();
    ui.heading(RichText::new("编解码").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    let mode = theme::form_tabs(ui, &CODEC_MODES, &mut t.mode);
    ui.add_space(6.0);
    theme::form_field_multiline(ui, "输入", "codec-input", &mut t.input, wide, 8);
    match codec_convert(mode, &t.input) {
        Ok(out) => {
            t.output = out;
            t.error = None;
        }
        Err(e) => t.error = Some(e),
    }
    let mut shown = t.output.clone();
    theme::form_field_multiline(ui, "输出", "codec-output", &mut shown, wide, 8);
    button_row(ui, |ui| {
        if theme::button(ui, "复制输出", theme::BtnKind::Secondary).clicked() && !t.output.is_empty()
        {
            ui.ctx().copy_text(t.output.clone());
        }
        if theme::button(ui, "输出变输入", theme::BtnKind::Secondary).clicked()
            && !t.output.is_empty()
        {
            t.input = t.output.clone();
        }
        ui.label(
            RichText::new(format!("{} 字节", t.output.len()))
                .size(theme::fs_px(11.0))
                .color(p.muted),
        );
        if let Some(e) = &t.error {
            ui.label(RichText::new(e).size(theme::fs_px(11.5)).color(p.red));
        }
    });
}

/// 页内按钮行:按钮是带边框盒子,相邻 12px(与表单基线一致)
fn button_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        add(ui)
    })
    .inner
}

// ---------------------------------------------------------------- 时间戳

#[derive(Default, Clone)]
pub struct TsTool {
    /// 时间戳 → 日期
    pub stamp_in: String,
    pub date_out: String,
    /// 日期 → 时间戳
    pub date_in: String,
    pub stamp_out: String,
    /// 上一帧输入(检测变化后才解析,避免每帧覆盖只读展示)
    last_stamp_in: String,
    last_date_in: String,
}

fn draw_timestamp(ui: &mut egui::Ui, t: &mut TsTool) {
    let p = theme::c();
    ui.heading(RichText::new("时间戳 ⇄ 日期").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    // 当前时间(实时)
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("当前").size(theme::fs_px(13.0)));
        ui.label(
            RichText::new(format_ts(now_ms))
                .font(theme::mono(13.0))
                .color(p.text),
        );
        ui.label(
            RichText::new(format!("{now_ms} ms"))
                .font(theme::mono(12.0))
                .color(p.muted),
        );
        // 文字不是带边框盒子:到按钮 4 + 行距 8 = 12
        ui.add_space(4.0);
        if theme::button(ui, "复制时间戳", theme::BtnKind::Secondary).clicked() {
            ui.ctx().copy_text(now_ms.to_string());
        }
    });
    ui.add_space(10.0);
    theme::hairline_top(ui);
    ui.add_space(10.0);

    // 时间戳 → 日期(秒/毫秒自适应:>1e12 视为毫秒)
    ui.label(
        RichText::new("时间戳 → 日期")
            .size(theme::fs_px(12.5))
            .color(p.text2),
    );
    ui.add_space(4.0);
    theme::form_row(ui, "时间戳", |ui| {
        theme::field_hint(ui, "ts-in", &mut t.stamp_in, 280.0, false, "秒或毫秒,自动识别");
    });
    if t.stamp_in.trim() != t.last_stamp_in {
        t.last_stamp_in = t.stamp_in.trim().to_string();
        t.date_out = t
            .stamp_in
            .trim()
            .parse::<i64>()
            .map(|v| format_ts(if v > 1_000_000_000_000 { v } else { v * 1000 }))
            .unwrap_or_else(|_| "无法解析(需为整数)".into());
    }
    theme::form_row(ui, "日期", |ui| {
        ui.label(
            RichText::new(t.date_out.clone())
                .font(theme::mono(13.0))
                .color(p.text),
        );
    });
    ui.add_space(10.0);

    // 日期 → 时间戳(本地时区)
    ui.label(
        RichText::new("日期 → 时间戳")
            .size(theme::fs_px(12.5))
            .color(p.text2),
    );
    ui.add_space(4.0);
    theme::form_row(ui, "日期", |ui| {
        theme::field_hint(
            ui,
            "date-in",
            &mut t.date_in,
            280.0,
            false,
            "2026-11-28 10:30:00",
        );
    });
    if t.date_in.trim() != t.last_date_in {
        t.last_date_in = t.date_in.trim().to_string();
        t.stamp_out = parse_date(&t.date_in)
            .map(|ms| {
                let (s, ms_part) = (ms / 1000, ms % 1000);
                if ms_part == 0 {
                    s.to_string()
                } else {
                    format!("{s}.{ms_part:03}")
                }
            })
            .unwrap_or_else(|_| "无法解析(需为日期)".into());
    }
    theme::form_row(ui, "时间戳", |ui| {
        ui.label(
            RichText::new(t.stamp_out.clone())
                .font(theme::mono(13.0))
                .color(p.text),
        );
    });
}

fn parse_date(s: &str) -> Result<i64, String> {
    let s = s.trim();
    for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt.and_local_timezone(chrono::Local).single().map_or_else(
                || dt.and_utc().timestamp_millis(),
                |d| d.timestamp_millis(),
            ));
        }
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .single()
            .map_or_else(
                || d.and_time(chrono::NaiveTime::MIN).and_utc().timestamp_millis(),
                |d| d.timestamp_millis(),
            ));
    }
    Err("无法解析".into())
}

fn format_ts(ms: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_else(|| format!("{ms} ms"))
}

// ---------------------------------------------------------------- UUID

#[derive(Default, Clone)]
pub struct UuidTool {
    pub count: String,
    pub output: String,
}

fn draw_uuid(ui: &mut egui::Ui, t: &mut UuidTool, wide: f32) {
    let p = theme::c();
    ui.heading(RichText::new("UUID 生成").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    theme::form_row(ui, "数量", |ui| {
        // form_row 的控件区是纵向布局:输入框和按钮并排要自己包 horizontal
        ui.horizontal(|ui| {
            theme::field_hint(ui, "uuid-count", &mut t.count, 120.0, false, "1-500");
            if theme::button(ui, "生成 v4", theme::BtnKind::Primary).clicked() {
                let n = t
                    .count
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(1)
                    .clamp(1, 500);
                t.output = (0..n)
                    .map(|_| uuid::Uuid::new_v4().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            if theme::button(ui, "复制全部", theme::BtnKind::Secondary).clicked()
                && !t.output.is_empty()
            {
                ui.ctx().copy_text(t.output.clone());
            }
            if !t.output.is_empty() {
                ui.label(
                    RichText::new(format!("{} 个", t.output.lines().count()))
                        .size(theme::fs_px(11.0))
                        .color(p.muted),
                );
            }
        })
    });
    if !t.output.is_empty() {
        let mut shown = t.output.clone();
        // 行数随结果走(2~14 行):固定 14 行时只生成几个也留一大片空白
        let rows = t.output.lines().count().clamp(2, 14);
        theme::form_field_multiline(ui, "", "uuid-out", &mut shown, wide, rows);
    }
}

// ---------------------------------------------------------------- JSON

#[derive(Default, Clone)]
pub struct JsonTool {
    pub input: String,
    pub output: String,
    pub error: Option<String>,
}

fn draw_json(ui: &mut egui::Ui, t: &mut JsonTool, wide: f32) {
    let p = theme::c();
    ui.heading(RichText::new("JSON").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    button_row(ui, |ui| {
        if theme::button(ui, "格式化", theme::BtnKind::Primary).clicked() {
            match serde_json::from_str::<serde_json::Value>(&t.input) {
                Ok(v) => {
                    t.output = serde_json::to_string_pretty(&v).unwrap_or_default();
                    t.error = None;
                }
                Err(e) => t.error = Some(e.to_string()),
            }
        }
        if theme::button(ui, "压缩", theme::BtnKind::Secondary).clicked() {
            match serde_json::from_str::<serde_json::Value>(&t.input) {
                Ok(v) => {
                    t.output = serde_json::to_string(&v).unwrap_or_default();
                    t.error = None;
                }
                Err(e) => t.error = Some(e.to_string()),
            }
        }
        if theme::button(ui, "校验", theme::BtnKind::Secondary).clicked() {
            t.error = Some(match serde_json::from_str::<serde_json::Value>(&t.input) {
                Ok(_) => "JSON 有效 ✓".to_string(),
                Err(e) => e.to_string(),
            });
        }
        if theme::button(ui, "复制输出", theme::BtnKind::Secondary).clicked()
            && !t.output.is_empty()
        {
            ui.ctx().copy_text(t.output.clone());
        }
        if let Some(e) = &t.error {
            ui.label(
                RichText::new(crate::icons::clip(e, 70))
                    .size(theme::fs_px(11.5))
                    .color(p.red),
            );
        }
    });
    ui.add_space(8.0);
    // 上下两个框要在一屏里都看得到:输入 7 行 + 输出 9 行
    theme::form_field_multiline(ui, "输入", "json-in", &mut t.input, wide, 7);
    theme::form_field_multiline(ui, "输出", "json-out", &mut t.output, wide, 9);
}

// ---------------------------------------------------------------- 哈希 / HMAC

const HASH_MODES: [&str; 3] = ["文本摘要", "HMAC", "文件"];
const HMAC_ALGOS: [&str; 4] = ["MD5", "SHA-1", "SHA-256", "SHA-512"];
/// HMAC 输出格式:(下拉值, 显示)
const HMAC_FMTS: [(&str, &str); 3] = [("hex", "Hex 小写"), ("HEX", "Hex 大写"), ("b64", "Base64")];
const COPIED: &str = "已复制到剪贴板";

/// 文件摘要的状态。后台任务只写它,结果经 `BgEvent` 回填
#[derive(Clone)]
pub struct FileHash {
    pub path: String,
    pub name: String,
    pub total: u64,
    pub done: u64,
    pub digests: shujing_core::hashing::Digests,
    pub error: Option<String>,
    /// 用户取消(与读失败区分展示)
    pub cancelled: bool,
    /// 计算中的任务代次;None = 空闲
    pub running: Option<u64>,
}

#[derive(Clone)]
pub struct HashTool {
    /// 0 文本摘要 / 1 HMAC / 2 文件
    pub mode: usize,
    pub upper: bool,
    /// 文本摘要与 HMAC 共用
    pub input: String,
    pub text_out: shujing_core::hashing::Digests,
    last_input: String,
    pub hmac_algo: usize,
    pub hmac_key: String,
    pub hmac_fmt: usize,
    pub hmac_out: String,
    last_hmac: (usize, String, String, usize),
    pub file: Option<FileHash>,
    /// 在途任务的停止标志(与后台线程共享)
    cancel: Option<Arc<AtomicBool>>,
    /// 任务代次:每次启动 +1,迟到的进度/结果按它丢弃
    gen: u64,
}

impl Default for HashTool {
    fn default() -> Self {
        HashTool {
            mode: 0,
            upper: false,
            input: String::new(),
            text_out: Default::default(),
            last_input: String::new(),
            // 默认 SHA-256:HMAC-MD5/SHA-1 只在校验旧数据时才用
            hmac_algo: 2,
            hmac_key: String::new(),
            hmac_fmt: 0,
            hmac_out: String::new(),
            last_hmac: Default::default(),
            file: None,
            cancel: None,
            gen: 0,
        }
    }
}

impl HashTool {
    /// 登记一次新的文件任务:叫停旧任务、切到文件页,返回 (代次, 停止标志)。
    /// 调用方拿标志去后台算,结果带代次回来
    pub fn start_file(&mut self, path: &std::path::Path) -> (u64, Arc<AtomicBool>) {
        self.stop_running();
        self.gen += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.file = Some(FileHash {
            path: path.display().to_string(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            total: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            done: 0,
            digests: Default::default(),
            error: None,
            cancelled: false,
            running: Some(self.gen),
        });
        self.mode = 2;
        (self.gen, cancel)
    }

    /// 取代次匹配的在途状态。任务已结束(或已被更新的任务取代)就拿不到——
    /// 迟到的进度/结果因此落不下来
    pub fn running_mut(&mut self, gen: u64) -> Option<&mut FileHash> {
        let f = self.file.as_mut()?;
        (f.running == Some(gen)).then_some(f)
    }

    /// 叫停在途任务(不动结果展示)。用户取消、重选文件、标签关闭共用
    pub fn stop_running(&mut self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// 用户取消:任务立即视为结束并标记已取消
    pub fn cancel(&mut self) {
        self.stop_running();
        if let Some(f) = self.file.as_mut() {
            f.running = None;
            f.cancelled = true;
        }
    }
}

fn draw_hash(
    ui: &mut egui::Ui,
    tab_id: &str,
    t: &mut HashTool,
    wide: f32,
    actions: &mut Vec<TabAction>,
) {
    ui.heading(RichText::new("哈希 / HMAC").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    let mode = theme::form_tabs(ui, &HASH_MODES, &mut t.mode);
    ui.add_space(6.0);
    match mode {
        1 => draw_hash_hmac(ui, t, wide, actions),
        2 => draw_hash_file(ui, tab_id, t, wide, actions),
        _ => draw_hash_text(ui, t, wide, actions),
    }
}

fn cased(s: &str, upper: bool) -> String {
    if upper {
        s.to_uppercase()
    } else {
        s.to_string()
    }
}

/// 四种摘要拼成等宽文本块(算法名左对齐 8 字符,mono 字体下列对齐)。
/// `wrap` 给显示用:超 64 字符的摘要折成两行,续行缩进到摘要列——
/// 交给文本框自己折行的话续行会顶格,看着像多出来一行
fn digest_block(
    digests: &shujing_core::hashing::Digests,
    upper: bool,
    wrap: bool,
) -> String {
    Algo::ALL
        .iter()
        .zip(digests)
        .flat_map(|(a, d)| {
            let d = cased(d, upper);
            let head = format!("{:<8}{}", a.name(), &d[..d.len().min(64)]);
            if wrap && d.len() > 64 {
                vec![head, format!("        {}", &d[64..])]
            } else {
                vec![head]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// HMAC 输出:0 Hex 小写 / 1 Hex 大写 / 2 Base64
fn hmac_out(algo: usize, key: &str, data: &str, fmt: usize) -> String {
    use base64::Engine as _;
    let a = Algo::ALL[algo.min(3)];
    let raw = shujing_core::hashing::hmac_bytes(a, key.as_bytes(), data.as_bytes());
    match fmt {
        1 => raw.iter().map(|b| format!("{b:02X}")).collect(),
        2 => base64::engine::general_purpose::STANDARD.encode(raw),
        _ => raw.iter().map(|b| format!("{b:02x}")).collect(),
    }
}

/// 摘要输出框:每帧用临时副本显示(框里可选中复制,改动不回写)
fn digest_box(ui: &mut egui::Ui, id: &str, text: &mut String, wide: f32, rows: usize) {
    theme::form_row(ui, "摘要", |ui| {
        theme::code_box(
            ui,
            id,
            text,
            theme::fit_width(wide, ui.available_width()),
            rows,
            "",
        )
    });
}

/// 摘要下方的操作行:大写开关 + 每个算法一键复制 + 复制全部(窄窗口自动折行)。
/// `plain` 是"复制全部"给出的文本(不带显示用的续行缩进)
fn digest_actions(
    ui: &mut egui::Ui,
    digests: &shujing_core::hashing::Digests,
    upper: &mut bool,
    plain: &str,
    actions: &mut Vec<TabAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
        theme::checkbox(ui, upper, "大写");
        for (i, a) in Algo::ALL.into_iter().enumerate() {
            let clicked = theme::button(
                ui,
                &format!("复制 {}", a.name()),
                theme::BtnKind::Secondary,
            )
            .clicked();
            if clicked {
                actions.push(TabAction::CopyToClipboard {
                    text: cased(&digests[i], *upper),
                    toast: COPIED.into(),
                });
            }
        }
        if theme::button(ui, "复制全部", theme::BtnKind::Secondary).clicked() {
            actions.push(TabAction::CopyToClipboard {
                text: plain.to_string(),
                toast: COPIED.into(),
            });
        }
    });
}

/// 文本摘要:输入即算。输入变了才重算(大文本哈希有成本,不逐帧算)
fn draw_hash_text(ui: &mut egui::Ui, t: &mut HashTool, wide: f32, actions: &mut Vec<TabAction>) {
    theme::form_field_multiline(ui, "输入", "hash-in", &mut t.input, wide, 6);
    if t.last_input != t.input {
        t.text_out = shujing_core::hashing::digest_bytes(t.input.as_bytes());
        t.last_input = t.input.clone();
    }
    let mut shown = digest_block(&t.text_out, t.upper, true);
    digest_box(ui, "hash-out", &mut shown, wide, 5);
    let plain = digest_block(&t.text_out, t.upper, false);
    digest_actions(ui, &t.text_out, &mut t.upper, &plain, actions);
}

/// HMAC:算法 / 输出格式 / 密钥 / 输入,四者任一变化才重算
fn draw_hash_hmac(ui: &mut egui::Ui, t: &mut HashTool, wide: f32, actions: &mut Vec<TabAction>) {
    let p = theme::c();
    let algos: Vec<(String, String)> = HMAC_ALGOS.iter().map(|n| (n.to_string(), n.to_string())).collect();
    let fmts: Vec<(String, String)> = HMAC_FMTS
        .iter()
        .map(|(v, d)| (v.to_string(), d.to_string()))
        .collect();
    theme::form_row(ui, "算法", |ui| {
        // form_row 的控件区是纵向布局:两个下拉并排要自己包 horizontal
        ui.horizontal(|ui| {
            let sel = HMAC_ALGOS[t.hmac_algo.min(3)].to_string();
            if let Some(v) = theme::select(ui, "hash-hmac-algo", &sel, &algos, 170.0) {
                if let Some(i) = HMAC_ALGOS.iter().position(|n| *n == v) {
                    t.hmac_algo = i;
                }
            }
            ui.label(RichText::new("输出").size(theme::fs_px(11.5)).color(p.muted));
            let sel_f = HMAC_FMTS[t.hmac_fmt.min(2)].0.to_string();
            if let Some(v) = theme::select(ui, "hash-hmac-fmt", &sel_f, &fmts, 150.0) {
                if let Some(i) = HMAC_FMTS.iter().position(|(val, _)| *val == v) {
                    t.hmac_fmt = i;
                }
            }
        })
    });
    theme::form_row(ui, "密钥", |ui| {
        theme::field_hint(ui, "hash-hmac-key", &mut t.hmac_key, 320.0, false, "任意长度");
    });
    theme::form_field_multiline(ui, "输入", "hash-hmac-in", &mut t.input, wide, 6);
    let key = (t.hmac_algo, t.hmac_key.clone(), t.input.clone(), t.hmac_fmt);
    if key != t.last_hmac {
        t.hmac_out = hmac_out(t.hmac_algo, &t.hmac_key, &t.input, t.hmac_fmt);
        t.last_hmac = key;
    }
    let mut shown = t.hmac_out.clone();
    digest_box(ui, "hash-hmac-out", &mut shown, wide, 2);
    button_row(ui, |ui| {
        let clicked = theme::button(ui, "复制摘要", theme::BtnKind::Secondary).clicked();
        if clicked {
            actions.push(TabAction::CopyToClipboard {
                text: t.hmac_out.clone(),
                toast: COPIED.into(),
            });
        }
    });
}

/// 文件摘要:拖放区 + 后台进度 + 结果。拖入的新文件随时取代旧任务
fn draw_hash_file(
    ui: &mut egui::Ui,
    tab_id: &str,
    t: &mut HashTool,
    wide: f32,
    actions: &mut Vec<TabAction>,
) {
    let p = theme::c();
    let tab_id = tab_id.to_string();

    // 拖放区:还没有文件或在算时是大块;已有结果就收成一条细带,把结果提上来
    let compact = t
        .file
        .as_ref()
        .is_some_and(|f| f.running.is_none());
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(wide, if compact { 48.0 } else { 108.0 }),
        egui::Sense::hover(),
    );
    // 拖放判定不用 resp.hovered():区内子控件(图标/按钮)会在 egui hit-test 里
    // 抢走 hover;拖拽会话本身也会干扰 hit-test。直接看指针坐标是否落在区内
    let pointer_in_zone = ui.input(|i| {
        i.pointer
            .interact_pos()
            .is_some_and(|pt| rect.contains(pt))
    });
    let over = pointer_in_zone && ui.input(|i| !i.raw.hovered_files.is_empty());
    let radius = egui::CornerRadius::same(theme::radius_ctrl() as u8);
    ui.painter().rect_filled(
        rect,
        radius,
        if over {
            theme::tint(p.accent, 20)
        } else {
            p.input_bg
        },
    );
    theme::dashed_frame(
        ui.painter(),
        rect.shrink(0.5),
        if over { p.accent } else { p.border },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            "把文件拖到这里计算摘要",
        )
    });
    let mut pick = false;
    let inner_layout = if compact {
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center)
    } else {
        egui::Layout::top_down(egui::Align::Center)
    };
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(10.0))
            .layout(inner_layout),
        |ui| {
            if compact {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("把其他文件拖到这里,或")
                        .size(theme::fs_px(12.5))
                        .color(p.text2),
                );
                pick = theme::button(ui, "选择文件", theme::BtnKind::Secondary).clicked();
            } else {
                let (ir, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                crate::icons::paint_icon(
                    ui.painter(),
                    &crate::icons::UPLOAD,
                    ir.center(),
                    18.0,
                    if over { p.accent } else { p.muted },
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("把文件拖到这里计算摘要")
                        .size(theme::fs_px(12.5))
                        .color(p.text2),
                );
                ui.add_space(8.0);
                pick = theme::button(ui, "选择文件", theme::BtnKind::Primary).clicked();
            }
        },
    );
    // 指针落在本区且有文件放下:第一个带路径的文件
    let dropped: Option<std::path::PathBuf> =
        ui.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
    if let Some(path) = dropped.filter(|_| pointer_in_zone) {
        actions.push(TabAction::HashFile {
            tab_id: tab_id.clone(),
            path,
        });
    }
    if pick {
        actions.push(TabAction::HashPickFile { tab_id: tab_id.clone() });
    }

    let Some(f) = t.file.as_mut() else {
        return;
    };
    let path: std::path::PathBuf = f.path.clone().into();
    ui.add_space(6.0);
    let (cancel, redo, clear) = draw_file_state(ui, f, &mut t.upper, wide, actions);
    if cancel {
        t.cancel();
    }
    if clear {
        t.file = None;
    }
    if redo {
        actions.push(TabAction::HashFile { tab_id, path });
    }
}

/// 文件状态区:头部(文件名/大小/操作)+ 进度或结果。返回 (取消, 重新计算, 清除)
fn draw_file_state(
    ui: &mut egui::Ui,
    f: &FileHash,
    upper: &mut bool,
    wide: f32,
    actions: &mut Vec<TabAction>,
) -> (bool, bool, bool) {
    let p = theme::c();
    let running = f.running.is_some();
    let mut clicked = [false; 3];
    // 头部:文件名 + 大小在左,操作在右(计算中=取消;其余=重新计算 + 清除)
    ui.horizontal(|ui| {
        ui.label(RichText::new(&f.name).size(theme::fs_px(13.0)).color(p.text));
        ui.label(
            RichText::new(crate::app::fmt_bytes(f.total.max(f.done)))
                .size(theme::fs_px(11.5))
                .color(p.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if running {
                clicked[0] = theme::button(ui, "取消", theme::BtnKind::Secondary).clicked();
            } else {
                clicked[2] = theme::button(ui, "清除", theme::BtnKind::Secondary).clicked();
                clicked[1] = theme::button(ui, "重新计算", theme::BtnKind::Secondary).clicked();
            }
        });
    });
    ui.add_space(4.0);
    ui.label(RichText::new(&f.path).size(theme::fs_px(11.0)).color(p.muted));

    if running {
        ui.horizontal(|ui| {
            theme::progress_bar(ui, f.done, f.total, 260.0);
            ui.label(
                RichText::new(progress_text(f.done, f.total))
                    .size(theme::fs_px(11.5))
                    .color(p.muted),
            );
        });
    } else if let Some(e) = &f.error {
        ui.label(
            RichText::new(format!("读取失败:{e}"))
                .size(theme::fs_px(12.0))
                .color(p.red),
        );
    } else if f.cancelled {
        ui.label(RichText::new("已取消").size(theme::fs_px(12.0)).color(p.muted));
    } else {
        let mut shown = digest_block(&f.digests, *upper, true);
        digest_box(ui, "hash-file-out", &mut shown, wide, 5);
        let plain = digest_block(&f.digests, *upper, false);
        digest_actions(ui, &f.digests, upper, &plain, actions);
    }
    (clicked[0], clicked[1], clicked[2])
}

fn progress_text(done: u64, total: u64) -> String {
    if total > 0 {
        format!(
            "{:.0}%  ·  {} / {}",
            done as f32 / total as f32 * 100.0,
            crate::app::fmt_bytes(done),
            crate::app::fmt_bytes(total)
        )
    } else {
        format!("{}…", crate::app::fmt_bytes(done))
    }
}

// ---------------------------------------------------------------- JWT

/// JWT 解码与验签的页面状态。解码/验签结果按输入缓存,输入变了才重算
#[derive(Default, Clone)]
pub struct JwtTool {
    pub token: String,
    /// HS 系列填共享密钥,RS/PS/ES/EdDSA 填 PEM 公钥;为空 = 不验签
    pub key: String,
    last_token: String,
    last_key: String,
    decoded: Option<shujing_core::jwt::DecodedJwt>,
    decode_error: Option<String>,
    /// None = 没验(密钥为空,或令牌解不开)
    verify: Option<Result<shujing_core::jwt::VerifyResult, String>>,
}

impl JwtTool {
    /// 输入变了才重新解码、重新验签
    fn refresh(&mut self) {
        let (tok, key) = (self.token.trim().to_string(), self.key.trim().to_string());
        if tok == self.last_token && key == self.last_key {
            return;
        }
        (self.last_token, self.last_key) = (tok.clone(), key.clone());
        self.decoded = None;
        self.decode_error = None;
        if !tok.is_empty() {
            match shujing_core::jwt::decode(&tok) {
                Ok(j) => self.decoded = Some(j),
                Err(e) => self.decode_error = Some(e),
            }
        }
        // 密钥为空就是没验:不给任何"签名有效/无效"的结论
        self.verify = match (&self.decoded, key.is_empty()) {
            (Some(j), false) => Some(shujing_core::jwt::verify(j, &key)),
            _ => None,
        };
    }
}

/// 毫秒 → 人话时长(天 / 小时分 / 分秒 / 秒)
fn human_duration(ms: i64) -> String {
    let s = ms / 1000;
    if s >= 86_400 {
        format!("{} 天", s / 86_400)
    } else if s >= 3600 {
        format!("{} 小时 {} 分", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} 分 {} 秒", s / 60, s % 60)
    } else {
        format!("{} 秒", s)
    }
}

/// 图标 + 文字的状态标记。不用 ● ✓ 之类字符:字体回退会把行高撑乱(批次90 踩过)
fn status(
    ui: &mut egui::Ui,
    icon: &crate::icons::Icon,
    color: egui::Color32,
    text: impl Into<String>,
) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    crate::icons::paint_icon(ui.painter(), icon, r.center(), 14.0, color);
    ui.label(RichText::new(text).size(theme::fs_px(12.0)).color(color));
}

/// 一段内容的展示块:标题行(名称 + 备注 + 右侧"复制")+ 等宽框
#[allow(clippy::too_many_arguments)]
fn jwt_part(
    ui: &mut egui::Ui,
    title: &str,
    note: &str,
    id: &str,
    text: &mut String,
    width: f32,
    rows: usize,
    actions: &mut Vec<TabAction>,
) {
    let p = theme::c();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(title)
                .size(theme::fs_px(11.0))
                .strong()
                .color(p.muted),
        );
        if !note.is_empty() {
            ui.label(RichText::new(note).size(theme::fs_px(11.0)).color(p.muted));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::mini_text_button(ui, "复制", p.text2).0 {
                actions.push(TabAction::CopyToClipboard {
                    text: text.clone(),
                    toast: COPIED.into(),
                });
            }
        });
    });
    ui.add_space(2.0);
    theme::code_box(
        ui,
        id,
        text,
        theme::fit_width(width, ui.available_width()),
        rows,
        "",
    );
}

/// payload 里的时间声明,一声明一行(本地时间;悬停看原始秒数)
fn jwt_time_claims(ui: &mut egui::Ui, claims: &[shujing_core::jwt::TimeClaim]) {
    let p = theme::c();
    if claims.is_empty() {
        return;
    }
    ui.add_space(12.0);
    ui.label(
        RichText::new("时间声明")
            .size(theme::fs_px(11.0))
            .strong()
            .color(p.muted),
    );
    ui.add_space(4.0);
    for c in claims {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(c.name).font(theme::mono(12.0)).color(p.text2));
            ui.add(
                egui::Label::new(
                    RichText::new(&c.local)
                        .font(theme::mono(11.5))
                        .color(p.text),
                )
                .sense(egui::Sense::hover()),
            )
            .on_hover_text(format!("{} (秒)", c.ts));
        });
    }
}

/// JWT 解码与验签。解码 ≠ 可信:没验签时状态行明说"内容可被伪造"
fn draw_jwt(ui: &mut egui::Ui, t: &mut JwtTool, wide: f32, actions: &mut Vec<TabAction>) {
    let p = theme::c();
    ui.heading(RichText::new("JWT").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    theme::form_field_multiline(ui, "令牌", "jwt-token", &mut t.token, wide, 3);
    t.refresh();

    let Some(jwt) = &t.decoded else {
        ui.add_space(6.0);
        let text = t.decode_error.as_deref().unwrap_or(
            "粘贴 JWT(eyJ 开头的三段式)后自动拆开 header、payload 与签名",
        );
        let color = if t.decode_error.is_some() { p.red } else { p.muted };
        ui.label(RichText::new(text).size(theme::fs_px(12.0)).color(color));
        return;
    };

    // 摘要行:算法 + 验签状态 + 有效期
    let unsigned = jwt.alg == "none";
    let now_ms = chrono::Local::now().timestamp_millis();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(14.0, 6.0);
        ui.label(
            RichText::new(&jwt.alg)
                .font(theme::mono(12.5))
                .strong()
                .color(if unsigned { p.orange } else { p.text }),
        );
        match &t.verify {
            // 没验签就不给"可信"的暗示
            None if unsigned => status(
                ui,
                &crate::icons::TRIANGLE_ALERT,
                p.orange,
                "未签名:任何人都能伪造其内容",
            ),
            None => status(
                ui,
                &crate::icons::INFO,
                p.muted,
                "未验证签名:以下内容可被伪造",
            ),
            Some(Ok(shujing_core::jwt::VerifyResult::Valid)) => {
                status(ui, &crate::icons::CIRCLE_CHECK, p.green, "签名有效")
            }
            Some(Ok(shujing_core::jwt::VerifyResult::Invalid)) => status(
                ui,
                &crate::icons::CIRCLE_X,
                p.red,
                "签名无效:密钥不对,或内容被改过",
            ),
            Some(Ok(shujing_core::jwt::VerifyResult::Unsupported(m))) => {
                status(ui, &crate::icons::TRIANGLE_ALERT, p.orange, m.clone())
            }
            Some(Err(e)) => status(ui, &crate::icons::TRIANGLE_ALERT, p.orange, e.clone()),
        }
        match shujing_core::jwt::validity(&jwt.payload, now_ms) {
            shujing_core::jwt::Validity::NoExpiry => {
                status(ui, &crate::icons::INFO, p.muted, "未设置过期时间")
            }
            shujing_core::jwt::Validity::NotYetValid(ms) => status(
                ui,
                &crate::icons::TRIANGLE_ALERT,
                p.orange,
                format!("{}后生效", human_duration(ms)),
            ),
            shujing_core::jwt::Validity::Expired(ms) => status(
                ui,
                &crate::icons::CIRCLE_X,
                p.red,
                format!("已过期 {}", human_duration(ms)),
            ),
            shujing_core::jwt::Validity::Valid(ms) => status(
                ui,
                &crate::icons::CIRCLE_CHECK,
                p.green,
                format!("{}后过期", human_duration(ms)),
            ),
        }
    });

    // header 在左、payload 在右(宽时并排,窄了上下排);时间声明跟在 header 下面
    let mut header_text = serde_json::to_string_pretty(&jwt.header).unwrap_or_default();
    let mut payload_text = if jwt.payload_is_json {
        serde_json::to_string_pretty(&jwt.payload).unwrap_or_default()
    } else {
        jwt.payload.as_str().unwrap_or_default().to_string()
    };
    let payload_note = if jwt.payload_is_json { "" } else { "非 JSON 文本" };
    let claims = shujing_core::jwt::time_claims(&jwt.payload);
    let header_rows = header_text.lines().count().clamp(3, 10);
    let payload_rows = payload_text.lines().count().clamp(3, 12);
    ui.add_space(4.0);
    if ui.available_width() >= 640.0 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let left = 236.0;
            let origin = ui.cursor().min;
            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        origin,
                        egui::vec2(left, f32::INFINITY),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    jwt_part(ui, "HEADER", "", "jwt-header", &mut header_text, left, header_rows, actions);
                    jwt_time_claims(ui, &claims);
                },
            );
            // 两列都要各自包一层纵向子区域:horizontal 本身沿 x 排,
            // 直接往里画"标题行 + 框"会被排成一行
            let right = ui.available_width();
            let origin = ui.cursor().min;
            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        origin,
                        egui::vec2(right, f32::INFINITY),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    jwt_part(ui, "PAYLOAD", payload_note, "jwt-payload", &mut payload_text, right, payload_rows, actions);
                },
            );
        });
    } else {
        let w = ui.available_width();
        jwt_part(ui, "HEADER", "", "jwt-header", &mut header_text, w, header_rows, actions);
        ui.add_space(6.0);
        jwt_part(ui, "PAYLOAD", payload_note, "jwt-payload", &mut payload_text, w, payload_rows, actions);
        jwt_time_claims(ui, &claims);
    }

    // 签名段原样展示
    ui.add_space(12.0);
    if jwt.signature_b64.is_empty() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new("签名").size(theme::fs_px(11.0)).strong().color(p.muted));
            ui.label(RichText::new("没有签名段").size(theme::fs_px(11.0)).color(p.muted));
        });
    } else {
        let mut sig = jwt.signature_b64.clone();
        let note = format!("base64url · {} 字节", jwt.signature.len());
        let w = ui.available_width();
        // 签名是一整行长文本:按框内每行放得下的字符数估折行数(HS 一行,RS 要三四行)
        let per_line = ((w - 24.0) / 7.5).max(20.0);
        let rows = ((sig.len() as f32 / per_line).ceil() as usize).clamp(2, 8);
        jwt_part(ui, "签名", &note, "jwt-sig", &mut sig, w, rows, actions);
    }

    // 验签:填了密钥/公钥即自动验
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new("验签").size(theme::fs_px(11.0)).strong().color(p.muted));
        ui.label(
            RichText::new("HS 系列填共享密钥;RS / PS / ES / EdDSA 填 PEM 公钥。填入即自动验签")
                .size(theme::fs_px(11.0))
                .color(p.muted),
        );
    });
    ui.add_space(2.0);
    theme::form_field_multiline(ui, "密钥 / 公钥", "jwt-key", &mut t.key, wide, 3);
}

/// 真机自截图用的样例令牌(现算 HS256,落在有效期内,展示"签名有效")
pub fn toolbox_jwt_sample_for_shot() -> String {
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let h = b64(br#"{"alg":"HS256","typ":"JWT"}"#);
    let now = chrono::Local::now().timestamp();
    let p = b64(
        format!(r#"{{"sub":"demo","name":"数镜 样例","iat":{},"exp":{}}}"#, now - 300, now + 3600)
            .as_bytes(),
    );
    let sig = shujing_core::hashing::hmac_bytes(
        shujing_core::hashing::Algo::Sha256,
        b"k",
        format!("{h}.{p}").as_bytes(),
    );
    format!("{h}.{p}.{}", b64(&sig))
}

/// 测试入口:直接调用转换纯函数(渲染层每帧算,这里给行为测试用)
#[cfg(test)]
pub fn toolbox_codec_convert_for_test(mode: usize, input: &str) -> String {
    codec_convert(mode, input).unwrap_or_default()
}

#[cfg(test)]
pub fn toolbox_timestamp_for_test(v: i64) -> String {
    format_ts(if v > 1_000_000_000_000 { v } else { v * 1000 })
}

#[cfg(test)]
pub fn toolbox_hmac_for_test(algo: usize, key: &str, data: &str, fmt: usize) -> String {
    hmac_out(algo, key, data, fmt)
}

#[cfg(test)]
pub fn toolbox_digest_block_for_test(
    digests: &shujing_core::hashing::Digests,
    upper: bool,
) -> String {
    // 测试要的是可复制的原始文本(不带显示用的续行缩进)
    digest_block(digests, upper, false)
}
