//! Excel(.xlsx)导入导出。
//! 导出:rust_xlsxwriter;大整数按字符串写,防浮点精度损失(Tauri 版同款策略)。
//! 导入:calamine 读为字符串矩阵,复用 CSV 导入的 INSERT 管道。

use crate::model::{es, ConnInfo};
use std::sync::Arc;

/// 结果集 → .xlsx 文件。数字仅在"浮点往返无损"时按数字写,其余按字符串
/// (19 位大整数 f64 存不下,按数字写会被 Excel 截成科学计数法丢精度)。
pub fn export_xlsx(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    path: &str,
) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    sheet.set_name("Sheet1").map_err(es)?;
    let head_fmt = Format::new().set_bold();
    for (c, name) in columns.iter().enumerate() {
        sheet
            .write_with_format(0, c as u16, name, &head_fmt)
            .map_err(es)?;
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let Some(v) = v else { continue };
            if numeric_roundtrip(v) {
                sheet
                    .write_number(r as u32 + 1, c as u16, v.parse::<f64>().unwrap())
                    .map_err(es)?;
            } else {
                sheet
                    .write_string(r as u32 + 1, c as u16, v)
                    .map_err(es)?;
            }
        }
    }
    wb.save(path).map_err(|e| format!("写入 xlsx 失败: {e}"))
}

/// 整数字符串按 f64 往返无损且不带前导零/正号(如 "123"、"-45"、小数)
fn numeric_roundtrip(v: &str) -> bool {
    if v.is_empty() {
        return false;
    }
    let ok_head = v
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() || (c == '-' && v.len() > 1))
        .unwrap_or(false);
    if !ok_head || v.starts_with('0') && v.len() > 1 && !v.starts_with("0.") {
        return false;
    }
    v.parse::<f64>()
        .map(|f| f.is_finite() && f.to_string() == *v)
        .unwrap_or(false)
}

/// 读 .xlsx → (表头, 数据行),与 parse_csv 同构(空行跳过)。
pub fn parse_xlsx(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    use calamine::{Reader, Xlsx};
    let mut wb: Xlsx<_> =
        calamine::open_workbook(path).map_err(|e| format!("打开 xlsx 失败: {e}"))?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or("xlsx 中没有工作表")?
        .map_err(|e| format!("读取 xlsx 失败: {e}"))?;
    let mut rows_iter = range.rows();
    let header = rows_iter
        .next()
        .ok_or("文件为空")?
        .iter()
        .map(cell_to_string)
        .collect();
    let rows = rows_iter
        .map(|r| r.iter().map(cell_to_string).collect())
        // 整行空串的行跳过(与 CSV 空行语义一致)
        .filter(|r: &Vec<String>| !r.iter().all(|c| c.trim().is_empty()))
        .collect();
    Ok((header, rows))
}

fn cell_to_string(d: &calamine::Data) -> String {
    match d {
        calamine::Data::Empty => String::new(),
        calamine::Data::String(s) => s.to_string(),
        calamine::Data::Float(f) => {
            // 整数值不带小数点尾巴
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        calamine::Data::Int(i) => i.to_string(),
        calamine::Data::Bool(b) => b.to_string(),
        calamine::Data::DateTime(dt) => dt.to_string(),
        calamine::Data::DateTimeIso(s) => s.to_string(),
        calamine::Data::DurationIso(s) => s.to_string(),
        calamine::Data::Error(e) => format!("{e:?}"),
    }
}

/// CSV/XLSX 通吃导入:按扩展名分流解析,INSERT 生成共用
pub async fn import_table_file(
    service: &Arc<crate::Service>,
    info: ConnInfo,
    imp: crate::composite::CsvImport,
) -> Result<usize, String> {
    let (_, rows) = if imp.path.to_lowercase().ends_with(".xlsx") {
        parse_xlsx(&imp.path)?
    } else {
        crate::composite::parse_csv(&imp.path)?
    };
    crate::composite::import_rows(service, info, imp, rows).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_roundtrip_and_precision() {
        let dir = std::env::temp_dir().join(format!("sj-xlsx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.xlsx");
        let _ = std::fs::remove_file(&path);
        let cols = vec!["id".to_string(), "name".to_string(), "big".to_string()];
        let rows = vec![
            vec![Some("1".into()), Some("张三".into()), Some("12345678901234567890".into())],
            vec![Some("2.5".into()), None, Some("-42".into())],
        ];
        export_xlsx(&cols, &rows, &path.display().to_string()).unwrap();
        // 读回:表头/数据一致,大整数按字符串原样保留
        let (h, r) = parse_xlsx(&path.display().to_string()).unwrap();
        assert_eq!(h, vec!["id", "name", "big"]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0][0], "1");
        assert_eq!(r[0][1], "张三");
        assert_eq!(r[0][2], "12345678901234567890", "大整数不丢精度");
        assert_eq!(r[1][0], "2.5");
        // 空单元格读为空串
        assert_eq!(r[1][1], "");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn numeric_roundtrip_gate() {
        assert!(numeric_roundtrip("123"));
        assert!(numeric_roundtrip("-42"));
        assert!(numeric_roundtrip("2.5"));
        assert!(numeric_roundtrip("0.5"));
        // 前导零/超精度/非数字按字符串
        assert!(!numeric_roundtrip("007"));
        assert!(!numeric_roundtrip("12345678901234567890"));
        assert!(!numeric_roundtrip("nan"));
        assert!(!numeric_roundtrip("1e5"));
        assert!(!numeric_roundtrip(""));
        assert!(!numeric_roundtrip("-"));
    }
}
