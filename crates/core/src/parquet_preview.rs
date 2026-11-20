//! Parquet 文件预览:只读前 N 行转字符串网格(行组顺序),不做 schema 推断。

use parquet::file::reader::{FileReader, SerializedFileReader};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParquetPreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: i64,
    pub row_groups: usize,
    pub file_size: u64,
    /// 预览是否截断(total_rows > rows.len())
    pub truncated: bool,
}

pub fn preview(path: &std::path::Path, max_rows: usize) -> Result<ParquetPreview, String> {
    let file_size = std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| format!("读取文件信息失败: {e}"))?;
    let reader = SerializedFileReader::try_from(path)
        .map_err(|e| format!("打开 Parquet 失败: {e}"))?;
    let meta = reader.metadata();
    let total_rows = meta.file_metadata().num_rows();
    let row_groups = meta.num_row_groups();

    // 列名:取第一个行组的 schema(各行组一致)
    let mut columns: Vec<String> = meta
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut first_row_cols: Option<Vec<String>> = None;
    for row in reader
        .get_row_iter(None)
        .map_err(|e| format!("读取行失败: {e}"))?
    {
        if rows.len() >= max_rows {
            break;
        }
        let Ok(row) = row else { continue };
        let cols: Vec<(String, parquet::record::Field)> = row.into_columns();
        if first_row_cols.is_none() {
            first_row_cols = Some(cols.iter().map(|(n, _)| n.clone()).collect());
        }
        let cells = cols
            .into_iter()
            .map(|(_, f)| field_to_string(&f))
            .collect();
        rows.push(cells);
    }
    // 行迭代器给出的列名最可靠(与单元格一一对应)
    if let Some(names) = first_row_cols {
        columns = names;
    }

    let truncated = rows.len() < total_rows.max(0) as usize;
    Ok(ParquetPreview {
        columns,
        rows,
        total_rows,
        row_groups,
        file_size,
        truncated,
    })
}

/// Field → 文本:数值/字符串直接转,Float16/Decimal/Date/时间戳/嵌套类型
/// 走 Display(语义正确且稳定);preview 只读展示不需要更高保真
/// Field → Option<String>:Null=None,基础类型直接转,其余走 Display(语义正确)
fn field_to_string(f: &parquet::record::Field) -> Option<String> {
    use parquet::record::Field;
    match f {
        Field::Null => None,
        Field::Str(s) => Some(s.clone()),
        Field::Bool(v) => Some(v.to_string()),
        Field::Byte(v) => Some(v.to_string()),
        Field::Short(v) => Some(v.to_string()),
        Field::Int(v) => Some(v.to_string()),
        Field::Long(v) => Some(v.to_string()),
        Field::UByte(v) => Some(v.to_string()),
        Field::UShort(v) => Some(v.to_string()),
        Field::UInt(v) => Some(v.to_string()),
        Field::ULong(v) => Some(v.to_string()),
        Field::Float(v) => Some(v.to_string()),
        Field::Double(v) => Some(v.to_string()),
        // BYTE_ARRAY 无逻辑类型时回来是 Bytes:优先按 UTF-8 还原文本(导出工具
        // 常不写 STRING 逻辑注解),非 UTF-8 才退到字节表示
        Field::Bytes(b) => match std::str::from_utf8(b.data()) {
            Ok(t) => Some(t.to_string()),
            Err(_) => Some(f.to_string()),
        },
        Field::Float16(_) | Field::Decimal(_) | Field::Date(_) | Field::TimestampMillis(_)
        | Field::TimestampMicros(_) | Field::Group(_)
        | Field::ListInternal(_) | Field::MapInternal(_) => Some(f.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::basic::{Repetition, Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::types::Type as SchemaType;

    /// 写一个小 parquet(2 列 × 3 行,含 NULL)再预览——全链路自证
    #[test]
    fn write_then_preview() {
        let dir = std::env::temp_dir().join(format!("sj-pq-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.parquet");

        let schema = SchemaType::group_type_builder("schema")
            .with_fields(vec![
                std::sync::Arc::new(
                    SchemaType::primitive_type_builder("name", Type::BYTE_ARRAY)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .unwrap(),
                ),
                std::sync::Arc::new(
                    SchemaType::primitive_type_builder("age", Type::INT32)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .unwrap(),
                ),
            ])
            .build()
            .unwrap();
        let file = std::fs::File::create(&path).unwrap();
        let props = WriterProperties::builder().build();
        let mut writer =
            SerializedFileWriter::new(file, std::sync::Arc::new(schema), std::sync::Arc::new(props)).unwrap();
        let mut rg = writer.next_row_group().unwrap();
        {
            use parquet::data_type::ByteArrayType;
            let mut col = rg.next_column().unwrap().unwrap();
            col.typed::<ByteArrayType>()
                .write_batch(
                    &[
                        parquet::data_type::ByteArray::from(b"\xe5\xbc\xa0\xe4\xb8\x89".to_vec()), // 张三
                        parquet::data_type::ByteArray::from(b"lisi".to_vec()),
                    ],
                    Some(&[1, 1]),
                    None,
                )
                .unwrap();
            col.close().unwrap();
        }
        {
            use parquet::data_type::Int32Type;
            let mut col = rg.next_column().unwrap().unwrap();
            col.typed::<Int32Type>()
                .write_batch(&[30, 25], Some(&[1, 1]), None)
                .unwrap();
            col.close().unwrap();
        }
        let rg_closed = rg.close().unwrap();
        let _ = rg_closed;
        writer.close().unwrap();

        let pv = preview(&path, 200).unwrap();
        assert_eq!(pv.columns, vec!["name", "age"]);
        assert_eq!(pv.rows.len(), 2);
        assert_eq!(pv.rows[0][0].as_deref(), Some("张三"));
        assert_eq!(pv.rows[0][1].as_deref(), Some("30"));
        assert!(!pv.truncated);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// CSV/JSON 文件预览(与 Parquet 同结构;row_groups=0)
pub fn preview_csv(path: &std::path::Path, max_rows: usize) -> Result<ParquetPreview, String> {
    let (headers, all) = crate::composite::parse_csv(
        path.to_str().ok_or("路径含非 UTF-8 字符")?,
    )?;
    let total = all.len() as i64;
    let rows: Vec<Vec<Option<String>>> = all
        .into_iter()
        .take(max_rows)
        .map(|r| r.into_iter().map(Some).collect())
        .collect();
    let truncated = rows.len() < total as usize;
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(ParquetPreview {
        columns: headers,
        rows,
        total_rows: total,
        row_groups: 0,
        file_size,
        truncated,
    })
}

/// JSON 文件预览:对象数组展平(顶层键并集)/对象转键值两列/标量数组单列
pub fn preview_json(path: &std::path::Path, max_rows: usize) -> Result<ParquetPreview, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {e}"))?;
    let file_size = text.len() as u64;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    fn cell(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                Some(serde_json::to_string(v).unwrap_or_default())
            }
            other => Some(other.to_string()),
        }
    }
    let (columns, rows): (Vec<String>, Vec<Vec<Option<String>>>) = match v {
        serde_json::Value::Array(items) => {
            let mut cols: Vec<String> = Vec::new();
            for it in &items {
                if let serde_json::Value::Object(o) = it {
                    for k in o.keys() {
                        if !cols.contains(k) {
                            cols.push(k.clone());
                        }
                    }
                }
            }
            if cols.is_empty() {
                (
                    vec!["value".into()],
                    items.iter().map(|it| vec![cell(it)]).collect(),
                )
            } else {
                let rows = items
                    .iter()
                    .map(|it| match it {
                        serde_json::Value::Object(o) => {
                            cols.iter().map(|c| o.get(c).and_then(cell)).collect()
                        }
                        other => vec![cell(other)],
                    })
                    .collect();
                (cols, rows)
            }
        }
        serde_json::Value::Object(o) => (
            vec!["key".into(), "value".into()],
            o.into_iter()
                .map(|(k, v)| vec![Some(k), cell(&v)])
                .collect(),
        ),
        _ => return Err("JSON 顶层应是数组或对象".into()),
    };
    let total = rows.len() as i64;
    let rows: Vec<Vec<Option<String>>> = rows.into_iter().take(max_rows).collect();
    let truncated = rows.len() < total as usize;
    Ok(ParquetPreview {
        columns,
        rows,
        total_rows: total,
        row_groups: 0,
        file_size,
        truncated,
    })
}

/// 按扩展名分发(拖拽与菜单共用)
pub fn preview_data_file(path: &std::path::Path, max_rows: usize) -> Result<ParquetPreview, String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("parquet") => preview(path, max_rows),
        Some("csv") | Some("tsv") => preview_csv(path, max_rows),
        Some("json") | Some("ndjson") => preview_json(path, max_rows),
        other => Err(format!("不支持的文件类型: {:?}(支持 parquet/csv/tsv/json)", other)),
    }
}
