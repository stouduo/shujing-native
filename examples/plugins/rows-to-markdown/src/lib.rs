//! 示例 WASM 插件:行集 JSON → Markdown 表格。
//! 输入 {"columns":["a",..],"rows":[["v",..]..]};输出 {"text":"| a |\n|---|\n| v |"}
//! ABI:导出 memory/alloc/transform;输出 = 4 字节 u32 LE 长度 + UTF-8 JSON。

use serde_json::Value;
use std::alloc::Layout;
use std::alloc as raw_alloc_mod;

fn esc(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', "<br>")
}

fn rows_to_markdown(json: &str) -> String {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return format!("{{\"error\":\"输入不是合法 JSON: {e}\"}}"),
    };
    let cols: Vec<String> = v
        .get("columns")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    let rows: Vec<Vec<Value>> = v
        .get("rows")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().map(|r| r.as_array().cloned().unwrap_or_default()).collect())
        .unwrap_or_default();
    let mut out = String::from("| ");
    out.push_str(&cols.iter().map(|c| esc(c)).collect::<Vec<_>>().join(" | "));
    out.push_str(" |\n|");
    out.push_str(&cols.iter().map(|_| "---").collect::<Vec<_>>().join("|"));
    out.push_str("|\n");
    for r in &rows {
        let cells: Vec<String> = r
            .iter()
            .map(|c| match c {
                Value::Null => "NULL".to_string(),
                Value::String(s) => esc(s),
                other => other.to_string(),
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }
    format!("{{\"text\":{}}}", serde_json::to_string(&out).unwrap_or_default())
}

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe { raw_alloc_mod::alloc(Layout::from_size_align(size.max(1) as usize, 1).unwrap()) as i32 }
}

#[no_mangle]
pub extern "C" fn transform(in_ptr: i32, in_len: i32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(in_ptr as *const u8, in_len.max(0) as usize) };
    let json = String::from_utf8_lossy(input);
    let out = rows_to_markdown(&json).into_bytes();
    let total = 4 + out.len();
    unsafe {
        let base = raw_alloc_mod::alloc(Layout::from_size_align(total, 1).unwrap()) as i32;
        std::ptr::copy_nonoverlapping((out.len() as u32).to_le_bytes().as_ptr(), base as *mut u8, 4);
        std::ptr::copy_nonoverlapping(out.as_ptr(), (base + 4) as *mut u8, out.len());
        base
    }
}
