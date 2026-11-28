//! 开发工具箱标签:左侧工具导航 + 右侧工具页。工具都是"纯函数"型
//! (输入即输出,无网络/无后台任务),用 ui-kit 表单组件直接堆页面。

use crate::app::ToolboxTab;
use crate::table_view::TabAction;
use crate::theme;
use egui::RichText;

/// 工具条目:(分类, 名称)。分类只用于导航分组
const TOOLS: [(&str, &str, Tool); 4] = [
    ("编解码", "Base64 / URL / Hex", Tool::Codec),
    ("时间", "时间戳 ⇄ 日期", Tool::Timestamp),
    ("生成器", "UUID", Tool::Uuid),
    ("格式化", "JSON", Tool::Json),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Codec,
    Timestamp,
    Uuid,
    Json,
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
            Tool::Uuid => draw_uuid(ui, &mut tb.uuid),
            Tool::Json => draw_json(ui, &mut tb.json, wide),
        }
        let _ = &mut actions;
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

/// 输入即输出:每帧重算,失败保留上一次输出并单独展示错误
fn draw_codec(ui: &mut egui::Ui, t: &mut CodecTool, wide: f32) {
    let p = theme::c();
    ui.heading(RichText::new("编解码").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    let mode = theme::form_tabs(ui, &CODEC_MODES, &mut t.mode);
    ui.add_space(8.0);
    theme::form_field_multiline(ui, "输入", "codec-input", &mut t.input, wide, 8);
    ui.add_space(8.0);
    match codec_convert(mode, &t.input) {
        Ok(out) if out != t.output => t.output = out,
        _ => {}
    }
    let mut shown = t.output.clone();
    theme::form_field_multiline(ui, "输出", "codec-output", &mut shown, wide, 8);
    ui.horizontal(|ui| {
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
    });
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

fn draw_uuid(ui: &mut egui::Ui, t: &mut UuidTool) {
    let p = theme::c();
    ui.heading(RichText::new("UUID 生成").size(theme::fs_px(15.0)));
    ui.add_space(8.0);
    theme::form_row(ui, "数量", |ui| {
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
    });
    ui.add_space(8.0);
    if !t.output.is_empty() {
        let mut shown = t.output.clone();
        theme::form_field_multiline(ui, "", "uuid-out", &mut shown, 420.0, 14);
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
    ui.horizontal(|ui| {
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
    theme::form_field_multiline(ui, "输入", "json-in", &mut t.input, wide, 10);
    ui.add_space(8.0);
    theme::form_field_multiline(ui, "输出", "json-out", &mut t.output, wide, 12);
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
