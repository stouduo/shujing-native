//! 表设计器:列规格 → CREATE/ALTER SQL 生成。
//! 与 dbclient(Tauri 版) stores/designerActions.ts 行为一致,纯函数可单测。

use crate::model::{DbType, TableStructure};

/// 设计器里一行列定义
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub length: String,
    pub nullable: bool,
    pub pk: bool,
    pub auto_inc: bool,
    pub default: String,
    pub comment: String,
    /// 编辑模式下:true = 已有列(不可改名/删列)
    pub existing: bool,
}

pub struct DesignerSql {
    pub create: Option<String>,
    pub alters: Vec<String>,
    pub warnings: Vec<String>,
}

fn quote_ident(db_type: DbType, name: &str) -> String {
    if db_type == DbType::MySql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// "varchar(255)" / "DECIMAL(10,2)" / "ENUM('a','b')" / "INTEGER AUTOINCREMENT"
/// → (基础类型大写, 括号内原文长度)
/// 长度保留完整括号内容(逗号/引号/大小写原样),DECIMAL(10,2) 不能只剩 "10"
pub fn split_type_len(data_type: &str) -> (String, String) {
    let raw = data_type.trim();
    if let Some(open) = raw.find('(') {
        if let Some(close_rel) = raw[open..].rfind(')') {
            let close = open + close_rel;
            let base = raw[..open].trim().to_uppercase();
            let len = raw[open + 1..close].trim().to_string();
            return (base, len);
        }
    }
    // SQLite 自增字段的 INTEGER AUTOINCREMENT / INTEGER PRIMARY KEY 形态
    let base = raw
        .to_uppercase()
        .replace("AUTOINCREMENT", "")
        .replace("PRIMARY KEY", "")
        .trim()
        .to_string();
    (base, String::new())
}

/// 从现有表结构转列规格(编辑模式初始值),同 dbclient helpers.columnsFromStructure
pub fn columns_from_structure(st: &TableStructure) -> Vec<ColumnSpec> {
    st.columns
        .iter()
        .map(|c| {
            let (base, len) = split_type_len(&c.data_type);
            ColumnSpec {
                name: c.name.clone(),
                data_type: base,
                length: len,
                nullable: c.nullable,
                pk: c.key == "PRI",
                auto_inc: c.extra.to_lowercase().contains("auto_increment")
                    || c.extra.to_lowercase().contains("identity")
                    || c.data_type.to_uppercase().contains("AUTOINCREMENT"),
                default: c.default.clone().unwrap_or_default(),
                comment: c.comment.clone(),
                existing: true,
            }
        })
        .collect()
}

fn type_of(db_type: DbType, c: &ColumnSpec) -> String {
    let mut t = c.data_type.trim().to_uppercase();
    if c.auto_inc && db_type == DbType::Postgres {
        return "SERIAL".to_string();
    }
    if !c.length.trim().is_empty() {
        t = format!("{}({})", t, c.length.trim());
    }
    t
}

fn col_def(db_type: DbType, c: &ColumnSpec) -> String {
    let mut s = format!("{} {}", quote_ident(db_type, &c.name), type_of(db_type, c));
    if c.auto_inc && db_type == DbType::MySql {
        s.push_str(" AUTO_INCREMENT");
    }
    // SQLite:INTEGER PRIMARY KEY AUTOINCREMENT 必须内联(rowid 别名语义)
    if c.pk
        && c.auto_inc
        && db_type == DbType::Sqlite
        && c.data_type.trim().eq_ignore_ascii_case("integer")
    {
        s.push_str(" PRIMARY KEY AUTOINCREMENT");
    }
    if !c.nullable && !(c.pk && c.auto_inc && db_type == DbType::Sqlite) {
        s.push_str(" NOT NULL");
    }
    if !c.default.is_empty() {
        let d = &c.default;
        let numeric = d
            .trim()
            .parse::<f64>()
            .map(|_| true)
            .unwrap_or(false);
        if numeric {
            s.push_str(&format!(" DEFAULT {}", d.trim()));
        } else {
            s.push_str(&format!(" DEFAULT '{}'", d.replace('\'', "''")));
        }
    }
    if !c.comment.is_empty() && db_type == DbType::MySql {
        s.push_str(&format!(" COMMENT '{}'", c.comment.replace('\'', "''")));
    }
    s
}

/// 生成建表/改表 SQL(预览与保存共用)。
/// 编辑模式传 `baseline`(加载时的列规格快照):只对实际改动的列生成 ALTER,
/// 未改动的列不再重复 MODIFY/TYPE(MySQL MODIFY 是全量替换,会丢未建模属性)
pub fn designer_sql(
    db_type: DbType,
    mode_create: bool,
    table_name: &str,
    cols: &[ColumnSpec],
    baseline: &[ColumnSpec],
) -> DesignerSql {
    let qt = quote_ident(db_type, table_name.trim());
    let mut warnings: Vec<String> = Vec::new();

    if mode_create {
        let mut defs: Vec<String> = cols.iter().map(|c| col_def(db_type, c)).collect();
        let pks: Vec<&ColumnSpec> = cols.iter().filter(|c| c.pk).collect();
        // SQLite 单列整型自增主键已内联,其余一律表级 PRIMARY KEY
        // (单列主键此前从不输出:MySQL 自增列无 key 报 1075,PG/SQLite 建出无主键表)
        let sqlite_inline = db_type == DbType::Sqlite
            && pks.len() == 1
            && pks[0].auto_inc
            && pks[0].data_type.trim().eq_ignore_ascii_case("integer");
        if !pks.is_empty() && !sqlite_inline {
            let names: Vec<String> = pks.iter().map(|c| quote_ident(db_type, &c.name)).collect();
            defs.push(format!("PRIMARY KEY ({})", names.join(", ")));
        }
        return DesignerSql {
            create: Some(format!(
                "CREATE TABLE {} (\n  {}\n);",
                qt,
                defs.join(",\n  ")
            )),
            alters: vec![],
            warnings,
        };
    }

    // 编辑模式:diff 生成 ALTER(不提供删除已有列,避免误删数据)
    let mut alters: Vec<String> = Vec::new();
    let added: Vec<&ColumnSpec> = cols.iter().filter(|c| !c.existing).collect();
    let existing: Vec<&ColumnSpec> = cols.iter().filter(|c| c.existing).collect();
    for c in &added {
        let plain = ColumnSpec {
            pk: false,
            auto_inc: false,
            ..(*c).clone()
        };
        alters.push(format!(
            "ALTER TABLE {} ADD COLUMN {};",
            qt,
            col_def(db_type, &plain)
        ));
    }
    for c in &existing {
        // 与加载基线比对:没改动的列不生成任何语句
        let base = baseline.iter().find(|b| b.name == c.name);
        if base == Some(c) {
            continue;
        }
        match db_type {
            DbType::MySql => {
                alters.push(format!(
                    "ALTER TABLE {} MODIFY COLUMN {};",
                    qt,
                    col_def(db_type, c)
                ));
            }
            DbType::Postgres => {
                let qi = quote_ident(db_type, &c.name);
                // SERIAL 是伪类型不能 ALTER TYPE:自增列跳过类型语句
                if !c.auto_inc {
                    let base_type_changed = base
                        .map(|b| b.data_type != c.data_type || b.length != c.length)
                        .unwrap_or(true);
                    if base_type_changed {
                        alters.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} TYPE {};",
                            qt,
                            qi,
                            type_of(db_type, c)
                        ));
                    }
                    let base_null_changed = base.map(|b| b.nullable != c.nullable).unwrap_or(true);
                    if base_null_changed {
                        alters.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} {};",
                            qt,
                            qi,
                            if c.nullable { "DROP NOT NULL" } else { "SET NOT NULL" }
                        ));
                    }
                }
                let base_default = base.map(|b| b.default.clone()).unwrap_or_default();
                if base_default != c.default {
                    if c.default.is_empty() {
                        alters.push(format!("ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;", qt, qi));
                    } else {
                        let d = c.default.trim();
                        let lit = if d.parse::<f64>().is_ok() {
                            d.to_string()
                        } else {
                            format!("'{}'", d.replace('\'', "''"))
                        };
                        alters.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                            qt, qi, lit
                        ));
                    }
                }
                let base_comment = base.map(|b| b.comment.clone()).unwrap_or_default();
                if base_comment != c.comment {
                    alters.push(format!(
                        "COMMENT ON COLUMN {}.{} IS '{}';",
                        qt,
                        qi,
                        c.comment.replace('\'', "''")
                    ));
                }
            }
            DbType::Sqlite => {
                warnings.push(format!("SQLite 无法修改已有列「{}」,已跳过", c.name));
            }
            DbType::Redis => {}
        }
    }
    if alters.is_empty() && warnings.is_empty() {
        warnings.push("没有可保存的变更".to_string());
    }
    DesignerSql {
        create: None,
        alters,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_mysql_with_pk_autoinc() {
        let cols = vec![
            ColumnSpec {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                pk: true,
                auto_inc: true,
                ..Default::default()
            },
            ColumnSpec {
                name: "name".into(),
                data_type: "VARCHAR".into(),
                length: "255".into(),
                default: "无名".into(),
                comment: "姓名".into(),
                nullable: true,
                ..Default::default()
            },
        ];
        let out = designer_sql(DbType::MySql, true, "users", &cols, &[]);
        let sql = out.create.unwrap();
        assert!(sql.starts_with("CREATE TABLE `users` ("));
        assert!(sql.contains("`id` INTEGER AUTO_INCREMENT"));
        assert!(sql.contains("PRIMARY KEY (`id`)"), "{sql}");
        assert!(sql.contains("`name` VARCHAR(255) DEFAULT '无名' COMMENT '姓名'"));
    }

    #[test]
    fn create_sqlite_pk_autoinc_no_not_null() {
        let cols = vec![ColumnSpec {
            name: "id".into(),
            data_type: "INTEGER".into(),
            nullable: false,
            pk: true,
            auto_inc: true,
            ..Default::default()
        }];
        let out = designer_sql(DbType::Sqlite, true, "t", &cols, &[]);
        let sql = out.create.unwrap();
        assert!(sql.contains("\"id\" INTEGER"), "{sql}");
        assert!(sql.contains("PRIMARY KEY AUTOINCREMENT"), "{sql}");
        assert!(!sql.contains("NOT NULL"), "{sql}");
    }

    #[test]
    fn create_composite_pk() {
        let cols = vec![
            ColumnSpec { name: "a".into(), data_type: "INT".into(), pk: true, ..Default::default() },
            ColumnSpec { name: "b".into(), data_type: "INT".into(), pk: true, ..Default::default() },
        ];
        let out = designer_sql(DbType::MySql, true, "t", &cols, &[]);
        assert!(out.create.unwrap().contains("PRIMARY KEY (`a`, `b`)"));
    }

    #[test]
    fn edit_add_and_modify_mysql() {
        let cols = vec![
            ColumnSpec { name: "id".into(), data_type: "INT".into(), nullable: true, existing: true, ..Default::default() },
            ColumnSpec { name: "age".into(), data_type: "INT".into(), nullable: true, existing: false, ..Default::default() },
        ];
        let out = designer_sql(DbType::MySql, false, "t", &cols, &[]);
        assert_eq!(
            out.alters,
            vec![
                "ALTER TABLE `t` ADD COLUMN `age` INT;".to_string(),
                "ALTER TABLE `t` MODIFY COLUMN `id` INT;".to_string(),
            ]
        );
    }

    #[test]
    fn edit_postgres_type_and_null() {
        let cols = vec![ColumnSpec {
            name: "n".into(),
            data_type: "VARCHAR".into(),
            length: "80".into(),
            nullable: false,
            existing: true,
            ..Default::default()
        }];
        let out = designer_sql(DbType::Postgres, false, "t", &cols, &[]);
        assert_eq!(
            out.alters,
            vec![
                "ALTER TABLE \"t\" ALTER COLUMN \"n\" TYPE VARCHAR(80);".to_string(),
                "ALTER TABLE \"t\" ALTER COLUMN \"n\" SET NOT NULL;".to_string(),
            ]
        );
    }

    #[test]
    fn edit_sqlite_warns_and_skips() {
        let cols = vec![ColumnSpec {
            name: "id".into(),
            data_type: "INT".into(),
            existing: true,
            ..Default::default()
        }];
        let out = designer_sql(DbType::Sqlite, false, "t", &cols, &[]);
        assert!(out.alters.is_empty());
        assert_eq!(out.warnings.len(), 1);
    }

    #[test]
    fn default_numeric_vs_string() {
        let cols = vec![
            ColumnSpec { name: "a".into(), data_type: "INT".into(), default: "-5".into(), ..Default::default() },
            ColumnSpec { name: "b".into(), data_type: "TEXT".into(), default: "it's".into(), ..Default::default() },
        ];
        let out = designer_sql(DbType::MySql, true, "t", &cols, &[]);
        let sql = out.create.unwrap();
        assert!(sql.contains("DEFAULT -5"));
        assert!(sql.contains("DEFAULT 'it''s'"));
    }

    #[test]
    fn edit_skips_unchanged_columns() {
        // 与基线完全一致 → 不生成任何 ALTER
        let c = ColumnSpec {
            name: "n".into(),
            data_type: "VARCHAR".into(),
            length: "80".into(),
            nullable: false,
            existing: true,
            ..Default::default()
        };
        let out = designer_sql(DbType::MySql, false, "t", std::slice::from_ref(&c), std::slice::from_ref(&c));
        assert!(out.alters.is_empty(), "{:?}", out.alters);
        assert!(out.warnings.iter().any(|w| w.contains("没有可保存")));

        // 只改 nullable → 只出一条 MODIFY
        let mut c2 = c.clone();
        c2.nullable = true;
        let out = designer_sql(DbType::MySql, false, "t", &[c2], &[c]);
        assert_eq!(out.alters.len(), 1);
    }

    #[test]
    fn edit_pg_serial_skips_type_default_comment() {
        // serial 列改注释:不生成 TYPE SERIAL(非法),只出 COMMENT ON
        let base = ColumnSpec {
            name: "id".into(),
            data_type: "SERIAL".into(),
            nullable: false,
            auto_inc: true,
            existing: true,
            ..Default::default()
        };
        let mut c = base.clone();
        c.comment = "主键".into();
        let out = designer_sql(DbType::Postgres, false, "t", &[c], &[base]);
        assert_eq!(
            out.alters,
            vec!["COMMENT ON COLUMN \"t\".\"id\" IS '主键';".to_string()],
            "{:?}",
            out.alters
        );
    }

    #[test]
    fn decimal_length_keeps_comma() {
        let (base, len) = split_type_len("decimal(10,2)");
        assert_eq!(base, "DECIMAL");
        assert_eq!(len, "10,2");
        let (b2, l2) = split_type_len("enum('a','b')");
        assert_eq!(b2, "ENUM");
        assert_eq!(l2, "'a','b'");
    }

    #[test]
    fn structure_to_specs() {
        let st = TableStructure {
            columns: vec![crate::model::ColumnDef {
                name: "id".into(),
                data_type: "integer".into(),
                nullable: false,
                key: "PRI".into(),
                default: None,
                extra: "auto_increment".into(),
                comment: String::new(),
            }],
            indexes: vec![],
            ddl: String::new(),
        };
        let specs = columns_from_structure(&st);
        assert_eq!(specs[0].data_type, "INTEGER");
        assert!(specs[0].pk);
        assert!(specs[0].auto_inc);
        assert!(specs[0].existing);
    }
}
