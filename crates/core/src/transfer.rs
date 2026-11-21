//! 数据传输:同类型连接间整表迁移(建表[可选先删]+分批 INSERT 流式搬运)。
//! 跨类型传输涉及类型映射,留给后续版本。

use crate::model::{Backend, ConnInfo, DbType};
use crate::Service;

fn sql_literal(v: &str, mysql: bool) -> String {
    if mysql {
        format!("'{}'", v.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        format!("'{}'", v.replace('\'', "''"))
    }
}

fn qualified(table: &str, db: Option<&str>, mysql: bool) -> String {
    let t = crate::edits::quote(table, mysql);
    match db.map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => format!("{}.{}", crate::edits::quote(d, mysql), t),
        None => t,
    }
}

/// 整表传输:返回(搬运行数, 批次数)
pub async fn transfer_table(
    service: &Service,
    src: (&str, Option<&str>, &str),
    dst: (&str, Option<&str>, &str),
    with_data: bool,
    drop_first: bool,
    // 字段映射(源列→目标列);空=同构直插(自动建表)。非空时不自动建表(目标表须已存在)
    col_map: &[(String, String)],
) -> Result<(u64, u64), String> {
    let (sc, sdb, st) = src;
    let (dc, ddb, dt) = dst;
    let infos = service.list_saved_connections();
    let src_info = infos.iter().find(|c| c.id == sc).ok_or("源连接不存在")?.clone();
    let dst_info = infos.iter().find(|c| c.id == dc).ok_or("目标连接不存在")?.clone();
    if src_info.db_type != dst_info.db_type {
        return Err("仅支持相同数据库类型之间的传输(跨类型需要类型映射)".into());
    }
    // 能力矩阵统一判定(此前七个散装 if;文案差异保留 SQLite/Mongo 特例)
    if !src_info.db_type.caps().transfer {
        return match src_info.db_type {
            DbType::Sqlite => {
                Err("SQLite 是单文件库,不支持网络传输;请用导出 SQL + 导入".into())
            }
            DbType::Mongo => {
                Err("Mongo 文档迁移暂不支持(集合结构无法用 SQL 表达)".into())
            }
            DbType::Mssql | DbType::ClickHouse => {
                Err("该类型传输暂不支持(建议导出 SQL + 导入)".into())
            }
            _ => Err("中间件数据源不支持 SQL 传输".into()),
        };
    }
    service.connect_by_id(sc).await?;
    service.connect_by_id(dc).await?;

    let mysql = dst_info.db_type == DbType::MySql;
    let dst_full = qualified(dt, ddb, mysql);
    let mapped = !col_map.is_empty();

    // 1) 建表:同构模式自动建;映射模式目标表须已存在(跳过建表/先删)
    let st_struct = service.get_table_structure(sc, st, sdb.map(str::to_string)).await?;
    let cols = crate::designer::columns_from_structure(&st_struct);
    let create = crate::designer::designer_sql(dst_info.db_type, true, dt, &cols, &[]);
    let create_sql = create
        .create
        .ok_or("无法生成建表语句(结构不完整)")?
        .trim_end_matches(';')
        .to_string();

    if mapped {
        // 映射模式:目标表由用户准备;drop_first/建表都跳过(没有可靠的重建语义)
        if with_data {
            // 仅校验目标表存在(取不到结构即报错)
            service
                .get_table_structure(dc, dt, ddb.map(str::to_string))
                .await
                .map_err(|e| format!("目标表不可达({e});映射模式要求目标表已存在"))?;
        }
    } else {
        // 目标侧可能带库限定:CREATE 生成的表名不带库,统一补
        let create_stmt = if let Some(d) = ddb.map(str::trim).filter(|s| !s.is_empty()) {
            // 表名出现在 "CREATE TABLE `t`" 或 "CREATE TABLE \"t\"" 首个引注处
            let qt = crate::edits::quote(dt, mysql);
            create_sql.replacen(&qt, &dst_full, 1)
        } else {
            create_sql
        };

        // 2) 先删(可选,破坏性)
        if drop_first {
            service
                .run_sql_db(dc, format!("DROP TABLE IF EXISTS {dst_full}"), None, ddb)
                .await?;
        }
        service.run_sql_db(dc, create_stmt, None, ddb).await?;
    }

    if !with_data {
        return Ok((0, 0));
    }

    // 3) 分批搬运:有主键按主键排序(offset 分页才稳定)
    let pk_cols: Vec<String> = st_struct
        .columns
        .iter()
        .filter(|c| c.key == "PRI")
        .map(|c| c.name.clone())
        .collect();
    let order = (!pk_cols.is_empty()).then(|| {
        let list: Vec<String> = pk_cols
            .iter()
            .map(|c| crate::edits::quote(c, mysql))
            .collect();
        format!(" ORDER BY {}", list.join(", "))
    });
    let src_full = qualified(st, sdb, mysql);
    let src_cols: Vec<String> = st_struct.columns.iter().map(|c| c.name.clone()).collect();
    // 列写入计划:(源值下标, 目标列名)。同构=恒等;映射=按 col_map 配对(未映射跳过)
    let plan: Vec<(usize, String)> = if mapped {
        src_cols
            .iter()
            .enumerate()
            .filter_map(|(i, sc)| {
                col_map
                    .iter()
                    .find(|(s, d)| s == sc && !d.trim().is_empty())
                    .map(|(_, d)| (i, d.trim().to_string()))
            })
            .collect()
    } else {
        src_cols
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.clone()))
            .collect()
    };
    if mapped && plan.is_empty() {
        return Err("字段映射为空:没有可写入的目标列".into());
    }
    let quoted_cols: Vec<String> = plan
        .iter()
        .map(|(_, d)| crate::edits::quote(d, mysql))
        .collect();

    let batch = 500;
    let mut offset: u64 = 0;
    let mut total: u64 = 0;
    let mut batches: u64 = 0;
    loop {
        let mut sql = format!("SELECT * FROM {src_full}");
        if let Some(o) = &order {
            sql.push_str(o);
        }
        sql.push_str(&format!(" LIMIT {batch} OFFSET {offset}"));
        let rs = service
            .run_sql_db(sc, sql, Some(batch), sdb)
            .await?
            .pop()
            .ok_or("源查询无结果")?;
        if rs.rows.is_empty() {
            break;
        }
        // 行 → INSERT(多行 VALUES 一条;NULL 语义保真)
        let mut inserts: Vec<String> = Vec::new();
        for row in &rs.rows {
            let vals: Vec<String> = plan
                .iter()
                .map(|(i, _)| match row.get(*i) {
                    None | Some(None) => "NULL".to_string(),
                    Some(Some(s)) => sql_literal(s, mysql),
                })
                .collect();
            inserts.push(format!(
                "INSERT INTO {dst_full} ({}) VALUES ({})",
                quoted_cols.join(", "),
                vals.join(", ")
            ));
        }
        let _ = &inserts;
        // 逐条执行(多语句拼接在事务外有半提交风险;每批一个隐式事务足够,
        // 传输是幂等可重跑的操作:失败重跑先勾"先删除目标表")
        for stmt in &inserts {
            service.run_sql_db(dc, stmt.clone(), None, ddb).await?;
        }
        let n = rs.rows.len() as u64;
        total += n;
        batches += 1;
        if (n as usize) < batch {
            break;
        }
        offset += n;
        // 防御:PG LIMIT+OFFSET 深翻页代价高,超过 50 万行停止并提示用导出导入
        if total > 500_000 {
            return Err(format!(
                "已传输 {total} 行仍未见底:超大表请用「导出 SQL + SQL 文件导入」路径"
            ));
        }
    }
    // 空表也要保证批次计数语义
    let _ = Backend::session_db; // 引用防 unused(实际逻辑在 impl 内)
    Ok((total, batches.max(if total == 0 { 0 } else { 1 })))
}
