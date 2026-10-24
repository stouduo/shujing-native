//! SQL 优化建议规则引擎(纯函数,可单测)。
//! 自 dbclient sqlAdvisor.ts 移植:静态检查(不执行)+ 执行计划解析(EXPLAIN 结果)。

use crate::model::{DbType, ExecResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Danger,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub title: String,
    pub detail: String,
}

fn finding(level: Level, title: &str, detail: &str) -> Finding {
    Finding {
        level,
        title: title.to_string(),
        detail: detail.to_string(),
    }
}

// ── 静态检查 ────────────────────────────────────────

pub fn static_check(sql_raw: &str) -> Vec<Finding> {
    let sql = sql_raw.trim();
    let mut out = Vec::new();
    if sql.is_empty() {
        return out;
    }
    let lower = sql.to_lowercase();
    let head = lower.get(..20).unwrap_or(&lower).to_string();

    if lower.starts_with("select") && regex_like_select_star(&lower) {
        out.push(finding(
            Level::Info,
            "SELECT *",
            "查询所有列会传输多余数据、无法利用覆盖索引;建议按需列出列名",
        ));
    }
    if lower.starts_with("select") && lower.contains("join") && !contains_word(&lower, "on") {
        out.push(finding(
            Level::Danger,
            "JOIN 缺少 ON 条件",
            "将产生笛卡尔积,行数可能爆炸;请补充 JOIN … ON … 条件",
        ));
    }
    if lower.starts_with("update") && !contains_word(&lower, "where") {
        out.push(finding(
            Level::Danger,
            "UPDATE 无 WHERE",
            "将更新整张表的所有行,请确认是否为有意操作",
        ));
    }
    if lower.starts_with("delete") && !contains_word(&lower, "where") {
        out.push(finding(
            Level::Danger,
            "DELETE 无 WHERE",
            "将删除整张表数据,建议先 SELECT 确认范围或加 LIMIT 分批",
        ));
    }
    if lower.contains("like '%") {
        out.push(finding(
            Level::Warn,
            "LIKE 前导通配符",
            "'%xxx' 形式无法使用 B-tree 索引,将全表扫描;若必须前缀模糊,考虑全文索引或倒排表",
        ));
    }
    let where_part = after_word(&lower, "where");
    if ["upper(", "lower(", "substring(", "substr(", "date(", "cast(", "convert(", "trim("]
        .iter()
        .any(|f| where_part.contains(f))
    {
        out.push(finding(
            Level::Warn,
            "WHERE 中对列使用函数",
            "列上的函数运算会让索引失效(除非有对应表达式索引);建议把计算移到常量侧或改写条件",
        ));
    }
    if lower.starts_with("select")
        && contains_word(&lower, "order by")
        && !contains_word(&lower, "limit")
    {
        out.push(finding(
            Level::Info,
            "大结果集排序",
            "ORDER BY 无 LIMIT 时若结果集大,排序开销高;建议分页或让排序列命中索引",
        ));
    }
    if contains_word(&lower, "union") && !contains_word(&lower, "union all") {
        out.push(finding(
            Level::Info,
            "UNION 去重",
            "UNION 会做去重排序,若无需去重用 UNION ALL 更快",
        ));
    }
    if contains_word(&lower, "not in") {
        out.push(finding(
            Level::Info,
            "NOT IN",
            "NOT IN 遇 NULL 语义坑且通常无法走索引;考虑改写为 NOT EXISTS 或 LEFT JOIN … IS NULL",
        ));
    }
    let _ = head;
    out
}

/// SELECT *(排除 select_count 之类:要求 select 后紧跟空白+*)
fn regex_like_select_star(lower: &str) -> bool {
    let rest = match lower.strip_prefix("select") {
        Some(r) => r,
        None => return false,
    };
    let trimmed = rest.trim_start();
    trimmed.starts_with('*')
}

/// 词边界包含(前后非字母数字下划线)
fn contains_word(haystack: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(word) {
        let abs = start + pos;
        let end = abs + word.len();
        let before_ok = abs == 0
            || haystack
                [..abs]
                .chars()
                .next_back()
                .map(|c| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(true);
        let after_ok = end >= haystack.len()
            || haystack[end..]
                .chars()
                .next()
                .map(|c| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        start = end;
    }
    false
}

/// 取关键词之后的片段
fn after_word<'a>(haystack: &'a str, word: &str) -> &'a str {
    haystack
        .find(word)
        .map(|i| &haystack[i + word.len()..])
        .unwrap_or("")
}

// ── 执行计划解析 ────────────────────────────────────

pub fn plan_check(db_type: DbType, r: &ExecResult) -> Vec<Finding> {
    let mut out = Vec::new();
    if r.columns.is_empty() {
        out.push(finding(Level::Info, "无执行计划", "该语句不返回执行计划"));
        return out;
    }
    match db_type {
        DbType::MySql => check_mysql(r, &mut out),
        DbType::Sqlite => check_sqlite(r, &mut out),
        DbType::Postgres => check_pg(r, &mut out),
        DbType::Redis => {}
    }
    out
}

fn col_idx(r: &ExecResult, names: &[&str]) -> i32 {
    let lower: Vec<String> = r.columns.iter().map(|c| c.to_lowercase()).collect();
    for n in names {
        if let Some(i) = lower.iter().position(|c| c == n) {
            return i as i32;
        }
    }
    -1
}

fn cell(row: &[Option<String>], i: i32) -> String {
    if i < 0 {
        return String::new();
    }
    row.get(i as usize)
        .cloned()
        .unwrap_or_default()
        .unwrap_or_default()
}

fn check_mysql(r: &ExecResult, out: &mut Vec<Finding>) {
    let it = col_idx(r, &["type"]);
    let ik = col_idx(r, &["key"]);
    let ir = col_idx(r, &["rows"]);
    let ie = col_idx(r, &["extra"]);
    let iid = col_idx(r, &["id"]);
    for row in &r.rows {
        let scope = if iid >= 0 {
            format!("(表 {})", cell(row, iid - 1))
        } else {
            String::new()
        };
        let ty = cell(row, it);
        let key = cell(row, ik);
        let rows = cell(row, ir);
        let extra = cell(row, ie);

        if ty == "ALL" {
            out.push(finding(
                Level::Danger,
                &format!("全表扫描 {scope}"),
                &format!("type=ALL,预估扫描 {rows} 行;WHERE/JOIN 列建议建立索引"),
            ));
        } else if ty == "index" {
            out.push(finding(
                Level::Warn,
                &format!("全索引扫描 {scope}"),
                "type=index(扫描整个索引),通常比走索引定位慢;检查条件列是否为索引最左前缀",
            ));
        }
        if key.is_empty() && ty != "const" && ty != "system" {
            out.push(finding(
                Level::Warn,
                &format!("未使用索引 {scope}"),
                "key=NULL,该访问路径没有命中索引",
            ));
        }
        if extra.to_lowercase().contains("using filesort") {
            out.push(finding(
                Level::Warn,
                &format!("额外排序 {scope}"),
                "Using filesort:排序未走索引;ORDER BY/GROUP BY 列考虑加索引",
            ));
        }
        if extra.to_lowercase().contains("using temporary") {
            out.push(finding(
                Level::Warn,
                &format!("临时表 {scope}"),
                "Using temporary:常见于 DISTINCT/GROUP BY/UNION;分组列建索引可避免",
            ));
        }
    }
    if out.is_empty() {
        out.push(finding(
            Level::Info,
            "计划良好",
            "未发现全表扫描/临时表/额外排序等信号",
        ));
    }
}

fn check_sqlite(r: &ExecResult, out: &mut Vec<Finding>) {
    let id = col_idx(r, &["detail"]);
    for row in &r.rows {
        let detail = if id >= 0 {
            cell(row, id)
        } else {
            row.iter()
                .map(|v| v.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let dl = detail.to_lowercase();
        // SCAN 表名(无索引可用)
        if let Some(pos) = dl.find("scan ") {
            let rest = dl[pos + 5..].trim_start();
            let table: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ',')
                .collect();
            if !table.is_empty() {
                out.push(finding(
                    Level::Danger,
                    &format!("全表扫描 {table}"),
                    &format!("SCAN {table}:过滤条件列无索引可用;建议建索引或检查条件写法"),
                ));
            }
        }
        // SEARCH … USING INDEX idx
        if let Some(pos) = dl.find("using index ") {
            let idx_name: String = dl[pos + 12..]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ',')
                .collect();
            if let Some(spos) = dl.find("search ") {
                let rest = dl[spos + 7..].trim_start();
                let table: String = rest
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != ',')
                    .collect();
                out.push(finding(
                    Level::Info,
                    &format!("{table} 命中索引 {idx_name}"),
                    "SEARCH … USING INDEX,访问路径良好",
                ));
            }
        }
    }
    if out.is_empty() {
        out.push(finding(
            Level::Info,
            "未发现明显问题",
            "执行计划中没有全表扫描信号",
        ));
    }
}

fn check_pg(r: &ExecResult, out: &mut Vec<Finding>) {
    let text = r
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| v.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let lower = text.to_lowercase();
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("seq scan on ") {
        let after = &rest[pos + 12..];
        let table: String = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '(')
            .collect();
        if !table.is_empty() {
            out.push(finding(
                Level::Danger,
                &format!("顺序扫描 {table}"),
                "Seq Scan:该表走了全表扫描;过滤/连接列建议建索引(大表尤其)",
            ));
        }
        rest = after;
    }
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("index scan using ") {
        let after = &rest[pos + 17..];
        let idx: String = after
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        if !idx.is_empty() {
            out.push(finding(
                Level::Info,
                &format!("命中索引 {idx}"),
                "Index Scan,访问路径良好",
            ));
        }
        rest = after;
    }
    // ANALYZE 模式:rows=est..actual 偏差检测
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("rows=") {
        let after = &rest[pos + 5..];
        let seg: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '.' ).collect();
        if let Some((est_s, actual_s)) = seg.split_once("..") {
            if let (Ok(est), Ok(actual)) = (est_s.parse::<i64>(), actual_s.parse::<i64>()) {
                if actual > est * 10 && actual > 100 {
                    out.push(finding(
                        Level::Warn,
                        "统计信息偏差大",
                        &format!("估算 {est} 行 vs 实际 {actual} 行,偏差超 10 倍;执行 ANALYZE 更新统计信息可改善计划选择"),
                    ));
                    break;
                }
            }
        }
        rest = after;
    }
    if out.is_empty() {
        out.push(finding(
            Level::Info,
            "未发现明显问题",
            "计划中无顺序扫描信号",
        ));
    }
}

/// EXPLAIN 语句构造(按方言)
pub fn explain_sql(db_type: DbType, sql: &str) -> String {
    let s = sql.trim();
    let s = s.strip_suffix(';').unwrap_or(s).trim_end();
    match db_type {
        DbType::Sqlite => format!("EXPLAIN QUERY PLAN {s}"),
        DbType::Postgres => format!("EXPLAIN (ANALYZE, BUFFERS) {s}"),
        _ => format!("EXPLAIN {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_danger_cases() {
        let f = static_check("UPDATE users SET a = 1");
        assert!(f.iter().any(|x| x.title == "UPDATE 无 WHERE" && x.level == Level::Danger));
        let f = static_check("DELETE FROM users");
        assert!(f.iter().any(|x| x.level == Level::Danger));
        let f = static_check("SELECT a FROM t1 JOIN t2");
        assert!(f.iter().any(|x| x.title == "JOIN 缺少 ON 条件"));
    }

    #[test]
    fn static_info_cases() {
        let f = static_check("SELECT * FROM users WHERE id = 1");
        assert!(f.iter().any(|x| x.title == "SELECT *"));
        let f = static_check("SELECT a FROM t ORDER BY a");
        assert!(f.iter().any(|x| x.title == "大结果集排序"));
        let f = static_check("SELECT a FROM t UNION SELECT b FROM u");
        assert!(f.iter().any(|x| x.title == "UNION 去重"));
    }

    #[test]
    fn static_no_false_positive_on_column_names() {
        // 列名含 where/limit 字样不应触发
        let f = static_check("SELECT whatever FROM t1 JOIN t2 ON t1.a = t2.a LIMIT 10");
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn like_and_func_rules() {
        let f = static_check("SELECT a FROM t WHERE name LIKE '%x'");
        assert!(f.iter().any(|x| x.title == "LIKE 前导通配符"));
        let f = static_check("SELECT a FROM t WHERE upper(name) = 'X'");
        assert!(f.iter().any(|x| x.title == "WHERE 中对列使用函数"));
    }

    fn exec(columns: &[&str], rows: Vec<Vec<Option<String>>>) -> ExecResult {
        ExecResult {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            rows,
            affected: 0,
            truncated: false,
            elapsed_ms: 0,
        }
    }

    #[test]
    fn mysql_plan_scan_and_temp() {
        let r = exec(
            &["id", "select_type", "table", "type", "key", "rows", "Extra"],
            vec![vec![
                Some("1".into()),
                Some("SIMPLE".into()),
                Some("users".into()),
                Some("ALL".into()),
                None,
                Some("83210".into()),
                Some("Using where; Using temporary".into()),
            ]],
        );
        let f = plan_check(DbType::MySql, &r);
        assert!(f.iter().any(|x| x.title.contains("全表扫描")));
        assert!(f.iter().any(|x| x.title.contains("未使用索引")));
        assert!(f.iter().any(|x| x.title.contains("临时表")));
    }

    #[test]
    fn mysql_plan_good() {
        let r = exec(
            &["type", "key", "rows", "Extra"],
            vec![vec![
                Some("ref".into()),
                Some("idx_name".into()),
                Some("12".into()),
                None,
            ]],
        );
        let f = plan_check(DbType::MySql, &r);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "计划良好");
    }

    #[test]
    fn sqlite_plan_scan() {
        let r = exec(
            &["detail"],
            vec![vec![Some("SCAN users".into())]],
        );
        let f = plan_check(DbType::Sqlite, &r);
        assert!(f.iter().any(|x| x.title.contains("全表扫描 users")));
    }

    #[test]
    fn pg_plan_seq_and_index() {
        let r = exec(
            &["QUERY PLAN"],
            vec![vec![Some(
                "Seq Scan on users (cost=0..12) / Index Scan using idx_a on t2".into(),
            )]],
        );
        let f = plan_check(DbType::Postgres, &r);
        assert!(f.iter().any(|x| x.title.contains("顺序扫描 users")));
        assert!(f.iter().any(|x| x.title.contains("命中索引 idx_a")));
    }

    #[test]
    fn explain_dialects() {
        assert_eq!(explain_sql(DbType::MySql, "SELECT 1;"), "EXPLAIN SELECT 1");
        assert_eq!(explain_sql(DbType::Sqlite, "SELECT 1"), "EXPLAIN QUERY PLAN SELECT 1");
        assert_eq!(
            explain_sql(DbType::Postgres, "SELECT 1"),
            "EXPLAIN (ANALYZE, BUFFERS) SELECT 1"
        );
    }
}
