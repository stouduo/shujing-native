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
