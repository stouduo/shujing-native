//! 结构对比:两张表(可跨连接)按列名对齐,产出差异清单与"把目标对齐到源"
//! 的 ALTER 语句(ALTER 生成复用设计器的 designer_sql,同一套引注/类型规则)。

use crate::designer::{columns_from_structure, designer_sql, ColumnSpec};
use crate::model::{ColumnDef, DbType, TableStructure};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnChange {
    pub name: String,
    pub field: String,
    pub src: String,
    pub dst: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDiff {
    pub added: Vec<ColumnDef>,
    pub removed: Vec<ColumnDef>,
    pub changed: Vec<ColumnChange>,
    /// 把目标表对齐到源表所需 ALTER(设计器同源生成;SQLite 无此能力为空)
    pub alter_sql: Vec<String>,
    pub warnings: Vec<String>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

fn spec_of(c: &ColumnDef, pk: bool, auto_inc: bool) -> ColumnSpec {
    ColumnSpec {
        name: c.name.clone(),
        data_type: c.data_type.clone(),
        length: String::new(),
        nullable: c.nullable,
        pk,
        auto_inc,
        default: c.default.clone().unwrap_or_default(),
        comment: c.comment.clone(),
        existing: true,
    }
}

/// 目标表结构 → 与源对齐的 ColumnSpec 列表(源有目标无=新列,参与 ADD;
/// 顺序按目标表的列序排,新增列追加在尾部——ADD 的语义就是追加)
fn target_specs_aligned(src: &TableStructure, dst: &TableStructure) -> Vec<ColumnSpec> {
    let mut out = Vec::new();
    for c in &dst.columns {
        // ColumnDef.key 带 PRI(schema 各方言统一回填),比索引推断可靠
        out.push(spec_of(c, c.key == "PRI", c.extra.contains("auto_increment")));
    }
    // 追加目标缺失的列(按源顺序)
    for c in &src.columns {
        if !dst.columns.iter().any(|d| d.name == c.name) {
            out.push(spec_of(c, false, false));
        }
    }
    out
}

/// 纯 diff(不加 SQL):测试与 UI 共用
pub fn diff_structures(src: &TableStructure, dst: &TableStructure) -> TableDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for c in &src.columns {
        match dst.columns.iter().find(|d| d.name == c.name) {
            None => added.push(c.clone()),
            Some(d) => {
                if c.data_type != d.data_type {
                    changed.push(ColumnChange { name: c.name.clone(), field: "类型".into(), src: c.data_type.clone(), dst: d.data_type.clone() });
                }
                if c.nullable != d.nullable {
                    changed.push(ColumnChange { name: c.name.clone(), field: "可空".into(), src: c.nullable.to_string(), dst: d.nullable.to_string() });
                }
                let (sd, dd) = (c.default.clone().unwrap_or_default(), d.default.clone().unwrap_or_default());
                if sd != dd {
                    changed.push(ColumnChange { name: c.name.clone(), field: "默认值".into(), src: sd, dst: dd });
                }
                if c.comment != d.comment {
                    changed.push(ColumnChange { name: c.name.clone(), field: "注释".into(), src: c.comment.clone(), dst: d.comment.clone() });
                }
            }
        }
    }
    for d in &dst.columns {
        if !src.columns.iter().any(|c| c.name == d.name) {
            removed.push(d.clone());
        }
    }
    TableDiff { added, removed, changed, alter_sql: Vec::new(), warnings: Vec::new() }
}

/// 完整对比:拉两张表结构 → diff → 生成对齐 ALTER(目标方言)
pub async fn diff_tables(
    service: &crate::Service,
    src: (&str, Option<&str>, &str),
    dst: (&str, Option<&str>, &str),
) -> Result<TableDiff, String> {
    let (sc, sdb, st) = src;
    let (dc, ddb, dt) = dst;
    service.connect_by_id(sc).await?;
    service.connect_by_id(dc).await?;
    let s_struct = service
        .get_table_structure(sc, st, sdb.map(str::to_string))
        .await?;
    let d_struct = service
        .get_table_structure(dc, dt, ddb.map(str::to_string))
        .await?;

    let mut diff = diff_structures(&s_struct, &d_struct);
    // ALTER 生成:目标列规格(与源对齐)为 cols,目标原列序为 baseline
    let d_info = service
        .list_saved_connections()
        .into_iter()
        .find(|c| c.id == dc)
        .ok_or("目标连接不存在")?;
    if d_info.db_type == DbType::Sqlite {
        diff.warnings.push("SQLite 不支持 ALTER 修改列,仅列出差异".into());
    } else {
        let dst_cols = target_specs_aligned(&s_struct, &d_struct);
        let dst_baseline = columns_from_structure(&d_struct);
        let sql = designer_sql(d_info.db_type, false, dt, &dst_cols, &dst_baseline);
        diff.alter_sql = sql.alters;
        diff.warnings.extend(sql.warnings);
    }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(cols: &[(&str, &str, bool)]) -> TableStructure {
        TableStructure {
            columns: cols
                .iter()
                .map(|(n, t, nullable)| ColumnDef {
                    name: n.to_string(),
                    data_type: t.to_string(),
                    nullable: *nullable,
                    key: String::new(),
                    default: None,
                    extra: String::new(),
                    comment: String::new(),
                })
                .collect(),
            indexes: vec![],
            ddl: String::new(),
        }
    }

    #[test]
    fn diff_added_removed_changed() {
        let src = st(&[("id", "INT", false), ("name", "VARCHAR(64)", true), ("age", "INT", true)]);
        let dst = st(&[("id", "INT", false), ("name", "VARCHAR(32)", true), ("phone", "VARCHAR(20)", true)]);
        let d = diff_structures(&src, &dst);
        assert_eq!(d.added.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["age"]);
        assert_eq!(d.removed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["phone"]);
        assert_eq!(d.changed.len(), 1);
        assert_eq!(d.changed[0].name, "name");
        assert_eq!(d.changed[0].field, "类型");
        assert_eq!(d.changed[0].src, "VARCHAR(64)");
        assert_eq!(d.changed[0].dst, "VARCHAR(32)");
        assert!(d.is_empty() == false);
    }

    #[test]
    fn identical_structures_empty_diff() {
        let s = st(&[("id", "INT", false)]);
        assert!(diff_structures(&s, &s).is_empty());
    }
}
