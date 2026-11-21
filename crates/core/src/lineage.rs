//! 字段血缘 v2:解析最外层 SELECT,给出每个输出列的来源。
//! 覆盖:FROM/JOIN/逗号连接 + 表别名消解 + AS/裸列/表达式/t.*/星号 +
//! CTE(WITH)递归追踪 + FROM 子查询追踪(有限深度,防无限递归)。
//! 诚实降级:UNION / 嵌套超限返回 Err——来源不在可解析范围,不假装追踪。

/// 一个输出列的血缘
#[derive(Debug, Clone, PartialEq)]
pub struct ColLineage {
    /// 输出列名(结果集里叫什么)
    pub output: String,
    /// 来源表达式(表别名已消解为真实表名);"*" 表示全列
    pub source: String,
}

/// 解析失败的原因(调用方展示降级说明)
#[derive(Debug)]
pub struct LineageError(pub String);

/// CTE/子查询展开层数上限(防无限递归;超过诚实降级)
const MAX_DEPTH: u32 = 2;

// ---------------------------------------------------------------- 预处理

/// 剥注释/字符串/反引号,保留结构与换行(避免注释里的 "select"/"join" 干扰)
fn strip_noise(sql: &str) -> String {
    let b: Vec<char> = sql.chars().collect();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    let mut in_bt = false;
    while i < b.len() {
        let c = b[i];
        if in_bt {
            out.push(if c == '`' { ' ' } else { 'x' });
            if c == '`' {
                in_bt = false;
            }
            i += 1;
            continue;
        }
        if in_str {
            out.push('x');
            if c == '\\' && i + 1 < b.len() {
                out.push('x');
                i += 2;
                continue;
            }
            if c == '\'' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                out.push('x');
                in_str = true;
                i += 1;
            }
            '`' => {
                out.push(' ');
                in_bt = true;
                i += 1;
            }
            '-' if i + 1 < b.len() && b[i + 1] == '-' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '#' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < b.len() && b[i + 1] == '*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    if b[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.into_iter().collect()
}

/// 括号深度感知的顶层逗号拆分
fn split_top_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// 词边界查找(大小写不敏感);返回匹配起点
fn find_word(s: &str, word: &str) -> Option<usize> {
    let low: Vec<char> = s.to_ascii_lowercase().chars().collect();
    let w: Vec<char> = word.chars().collect();
    let mut i = 0;
    while i + w.len() <= low.len() {
        if low[i..i + w.len()] == w[..] {
            let before_ok = i == 0 || !is_word_char(low[i - 1]);
            let after_ok = i + w.len() == low.len() || !is_word_char(low[i + w.len()]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 括号深度 0 的词查找(子查询内部的 FROM/JOIN/limit 不干扰主查询定位)
fn find_top_word(s: &str, word: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && is_word_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
            if w == word.to_ascii_lowercase() {
                return Some(start);
            }
            continue;
        }
        i += 1;
    }
    None
}

/// "(":其后(跳过空白)紧跟 select 词才算子查询——函数参数 COUNT(x)/UPPER(x) 不误判
fn has_subquery(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '(' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let rest: String = chars[j..].iter().collect();
            if find_word(&rest, "select").map(|p| p == 0).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// 从 open_idx 处的 '(' 开始取平衡括号内容;返回 (内部文本, 闭括号后位置)
fn balanced_span(s: &str, open_idx: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = s.chars().collect();
    if open_idx >= chars.len() || chars[open_idx] != '(' {
        return None;
    }
    let mut depth = 0i32;
    let mut i = open_idx;
    while i < chars.len() {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[open_idx + 1..i].iter().collect();
                    return Some((inner, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn read_word(s: &str, mut i: usize) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let start = i;
    while i < chars.len() && is_word_char(chars[i]) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

// ---------------------------------------------------------------- 表/来源

/// 一个"表"来源:真表 或 CTE/子查询(带列映射)
#[derive(Debug, Clone)]
struct TableSrc {
    /// 真实表名;CTE/子查询为定义名
    name: String,
    alias: Option<String>,
    /// 列映射(Some = CTE/子查询:output → 来源)
    cols: Option<Vec<ColLineage>>,
}

/// 提取 FROM/JOIN 表清单:表名(可含 db. 前缀)+ 可选别名 + 子查询列映射
fn parse_tables(from_clause: &str, depth: u32) -> Result<Vec<TableSrc>, LineageError> {
    let mut out = Vec::new();
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = find_top_word(&from_clause[start..], "join") {
        let abs = start + pos;
        segments.push((start, abs));
        start = abs + "join".len();
    }
    segments.push((start, from_clause.len()));
    for (a, b) in segments {
        let mut s = from_clause[a..b].trim().to_string();
        s = s.trim_start_matches(|c: char| c == ',').to_string();
        // 剥 JOIN 修饰词
        loop {
            let low_s = s.to_ascii_lowercase();
            let trimmed = low_s.trim_start().split(' ').next().unwrap_or("").to_string();
            if matches!(
                trimmed.as_str(),
                "left" | "right" | "inner" | "outer" | "cross" | "full" | "straight_join" | "natural" | ""
            ) && !trimmed.is_empty()
            {
                s = s[trimmed.len()..].trim_start().to_string();
            } else {
                break;
            }
        }
        // ON/USING 之后的部分丢弃
        if let Some(pos) = find_top_word(&s, "on") {
            s = s[..pos].trim().to_string();
        }
        if let Some(pos) = find_top_word(&s, "using") {
            s = s[..pos].trim().to_string();
        }
        // 逗号多表(隐式连接):每段取 表/子查询 + 别名
        for part in split_top_commas(&s) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if part.starts_with('(') {
                // FROM (SELECT ...) [AS] alias —— 子查询作为表
                let Some((body, after)) = balanced_span(part, 0) else {
                    return Err(LineageError("FROM 子查询括号不配对".into()));
                };
                let cols = Some(analyze_with(&body, depth + 1)?);
                let tail = part[after..].trim();
                let toks: Vec<&str> = tail.split_whitespace().collect();
                let alias = match toks.first() {
                    Some(&"AS") | Some(&"as") => toks.get(1).map(|x| x.to_string()),
                    Some(x) if !matches!(x.to_ascii_lowercase().as_str(), "on" | "using") => {
                        Some(x.to_string())
                    }
                    _ => None,
                };
                out.push(TableSrc { name: "(子查询)".into(), alias, cols });
                continue;
            }
            let toks: Vec<&str> = part.split_whitespace().collect();
            if toks.is_empty() {
                continue;
            }
            let table = toks[0].trim_matches(|c| c == '`' || c == '"').to_string();
            let mut alias = None;
            if toks.len() >= 2 {
                let second = toks[1].to_ascii_lowercase();
                if second == "as" && toks.len() >= 3 {
                    alias = Some(toks[2].trim_matches(|c| c == '`' || c == '"').to_string());
                } else if second != "on" && second != "using" && second != "join" {
                    alias = Some(toks[1].trim_matches(|c| c == '`' || c == '"').to_string());
                }
            }
            if !table.is_empty() {
                out.push(TableSrc { name: table, alias, cols: None });
            }
        }
    }
    Ok(out)
}

/// 输出行消解:星号展开/列映射透传/真表限定
fn resolve_rows(expr: &str, output: String, tables: &[TableSrc]) -> Vec<ColLineage> {
    let e = expr.trim();
    if e == "*" {
        if tables.len() == 1 {
            if let Some(cols) = &tables[0].cols {
                return cols.clone();
            }
            return vec![ColLineage { output, source: format!("{}.*", tables[0].name) }];
        }
        return vec![ColLineage { output, source: "*".into() }];
    }
    if e.ends_with(".*") {
        let t = e.trim_end_matches(".*");
        if let Some(ts) = find_table(t, tables) {
            if let Some(cols) = &ts.cols {
                return cols.clone();
            }
            return vec![ColLineage { output, source: format!("{}.*", ts.name) }];
        }
        return vec![ColLineage { output, source: format!("{e}.*") }];
    }
    if has_subquery(e) {
        return vec![ColLineage { output, source: "(子查询)".into() }];
    }
    if e.contains('.') && !e.contains('(') {
        let (t, col) = e.rsplit_once('.').unwrap();
        if let Some(ts) = find_table(t, tables) {
            if let Some(cols) = &ts.cols {
                if let Some(hit) = cols.iter().find(|c| c.output == col) {
                    return vec![ColLineage { output, source: hit.source.clone() }];
                }
            }
            return vec![ColLineage { output, source: format!("{}.{}", ts.name, col) }];
        }
        return vec![ColLineage { output, source: e.to_string() }];
    }
    if !e.contains('(') {
        if tables.len() == 1 {
            let ts = &tables[0];
            if let Some(cols) = &ts.cols {
                if let Some(hit) = cols.iter().find(|c| c.output == e) {
                    return vec![ColLineage { output, source: hit.source.clone() }];
                }
            }
            return vec![ColLineage { output, source: format!("{}.{}", ts.name, e) }];
        }
        // 多表:某 CTE/子查询有该输出列 → 消解
        for ts in tables {
            if let Some(cols) = &ts.cols {
                if let Some(hit) = cols.iter().find(|c| c.output == e) {
                    return vec![ColLineage { output, source: hit.source.clone() }];
                }
            }
        }
        return vec![ColLineage { output, source: e.to_string() }];
    }
    vec![ColLineage { output, source: e.to_string() }]
}

fn find_table<'a>(token: &str, tables: &'a [TableSrc]) -> Option<&'a TableSrc> {
    tables
        .iter()
        .find(|t| Some(token) == t.alias.as_deref() || t.name == token)
}

/// 解析最外层 SELECT 的输出列血缘(CTE/FROM 子查询递归展开,深度 ≤2)。
/// 返回 Err = 无法可靠追踪(UNION/嵌套超限/结构失败),调用方展示降级说明。
pub fn analyze(sql: &str) -> Result<Vec<ColLineage>, LineageError> {
    analyze_with(sql, 0)
}

pub fn analyze_with(sql: &str, depth: u32) -> Result<Vec<ColLineage>, LineageError> {
    if depth > MAX_DEPTH {
        return Err(LineageError("嵌套过深".into()));
    }
    let s = strip_noise(sql).trim().to_string();
    let low = s.to_ascii_lowercase();
    if !low.starts_with("select") && !low.starts_with("with") {
        return Err(LineageError("非 SELECT 语句".into()));
    }

    // ── CTE:WITH name AS (SELECT ...), ... 主查询 ──
    let mut cte_maps: Vec<(String, Vec<ColLineage>)> = Vec::new();
    let mut main_sql = s.clone();
    if low.starts_with("with") {
        let mut i = 4usize;
        loop {
            i = skip_ws(&s, i);
            if i >= s.len() {
                return Err(LineageError("WITH 解析失败".into()));
            }
            let (name, ni) = read_word(&s, i);
            if name.is_empty() {
                return Err(LineageError("WITH 解析失败(缺 CTE 名)".into()));
            }
            i = skip_ws(&s, ni);
            if find_word(&s[i..], "as").map(|p| p == 0).unwrap_or(false) {
                i = skip_ws(&s, i + 2);
            }
            if i >= s.len() || s.chars().nth(i) != Some('(') {
                return Err(LineageError("WITH 解析失败(缺括号定义)".into()));
            }
            let Some((body, after)) = balanced_span(&s, i) else {
                return Err(LineageError("WITH 括号不配对".into()));
            };
            let lin = analyze_with(&body, depth + 1)?;
            cte_maps.push((name, lin));
            i = skip_ws(&s, after);
            if s.chars().nth(i) == Some(',') {
                i += 1;
                continue;
            }
            main_sql = s[i..].trim().to_string();
            break;
        }
    }

    let low = main_sql.to_ascii_lowercase();
    if !low.starts_with("select") {
        return Err(LineageError("主查询非 SELECT".into()));
    }
    if low.contains(" union ") {
        return Err(LineageError("UNION 不追踪".into()));
    }
    let sel_start = find_word(&main_sql, "select").ok_or(LineageError("无 SELECT".into()))?;
    let from_start = find_top_word(&main_sql, "from")
        .ok_or(LineageError("无 FROM 子句(SELECT 无来源表)".into()))?;
    let select_list = main_sql[sel_start + 6..from_start].trim();
    if select_list.is_empty() {
        return Err(LineageError("空 SELECT 清单".into()));
    }
    // FROM 子句终点:顶层的 where/group/order/limit/having/union/window/offset
    let rest = &main_sql[from_start + 4..];
    let mut from_end = rest.len();
    for kw in ["where", "group", "order", "limit", "having", "union", "window", "offset"] {
        if let Some(p0) = find_top_word(rest, kw) {
            let p = p0.min(rest.len());
            from_end = from_end.min(p);
        }
    }
    let from_clause = rest[..from_end].trim().to_string();
    if from_clause.is_empty() {
        return Err(LineageError("空 FROM 子句".into()));
    }
    let mut tables = parse_tables(&from_clause, depth)?;
    if tables.is_empty() {
        return Err(LineageError("FROM 解析失败".into()));
    }
    // CTE 注册为虚拟表(列映射透传给消解)
    for (name, lin) in &cte_maps {
        for t in tables.iter_mut() {
            if t.alias.as_deref() == Some(name.as_str()) || t.name == *name {
                if t.cols.is_none() {
                    t.name = name.clone();
                    t.cols = Some(lin.clone());
                }
            }
        }
    }

    let mut out: Vec<ColLineage> = Vec::new();
    for item in split_top_commas(select_list) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (expr, alias) = match find_word(item, "as") {
            Some(p) => (
                item[..p].trim().to_string(),
                Some(item[p + 2..].trim().to_string()),
            ),
            None => (item.to_string(), None),
        };
        let output = alias.unwrap_or_else(|| {
            if expr.contains('.') && !expr.ends_with('*') && !expr.contains('(') {
                expr.rsplit('.').next().unwrap_or(expr.as_str()).to_string()
            } else if expr == "*" {
                "*".to_string()
            } else {
                let clipped: String = expr.chars().take(24).collect();
                clipped
            }
        });
        out.extend(resolve_rows(&expr, output, &tables));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_table_explicit_columns() {
        let r = analyze("SELECT id, name, age FROM users").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].output, "id");
        assert_eq!(r[0].source, "users.id");
    }

    #[test]
    fn join_alias_resolved() {
        let r = analyze(
            "SELECT u.name AS uname, o.total, o.id AS oid FROM users u JOIN orders o ON o.uid = u.id",
        )
        .unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].output, "uname");
        assert_eq!(r[0].source, "users.name");
        assert_eq!(r[1].source, "orders.total");
        assert_eq!(r[2].output, "oid");
        assert_eq!(r[2].source, "orders.id");
    }

    #[test]
    fn comma_join_and_star() {
        let r = analyze("SELECT u.*, o.total FROM users u, orders o").unwrap();
        assert_eq!(r[0].source, "users.*");
        assert_eq!(r[1].source, "orders.total");
    }

    #[test]
    fn functions_and_expr_kept_verbatim() {
        let r = analyze("SELECT COUNT(*) AS cnt, UPPER(name) FROM users").unwrap();
        assert_eq!(r[0].output, "cnt");
        assert_eq!(r[0].source, "COUNT(*)");
        assert_eq!(r[1].output, "UPPER(name)");
        assert_eq!(r[1].source, "UPPER(name)");
    }

    #[test]
    fn comments_and_strings_ignored() {
        let r = analyze("SELECT name -- select * from secrets\n, age FROM users WHERE name='x,y'").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].source, "users.name");
    }

    #[test]
    fn db_qualified_table() {
        let r = analyze("SELECT id FROM shop.users").unwrap();
        assert_eq!(r[0].source, "shop.users.id");
    }

    // ── v2:CTE / 子查询追踪 ──

    #[test]
    fn cte_traced() {
        let r = analyze(
            "WITH t AS (SELECT id, name FROM users) SELECT t.name FROM t",
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].output, "name");
        assert_eq!(r[0].source, "users.name");
    }

    #[test]
    fn cte_alias_and_multi_defs() {
        let r = analyze(
            "WITH a AS (SELECT id FROM x), b AS (SELECT n FROM y) SELECT aa.id AS i, b.n FROM a aa JOIN b ON b.aid = aa.id",
        )
        .unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].output, "i");
        assert_eq!(r[0].source, "x.id");
        assert_eq!(r[1].source, "y.n");
    }

    #[test]
    fn cte_star_expands() {
        let r = analyze("WITH t AS (SELECT id, name FROM users) SELECT t.* FROM t").unwrap();
        assert_eq!(r.len(), 2, "CTE 星号按其定义列铺开: {r:?}");
        assert_eq!(r[0].source, "users.id");
        assert_eq!(r[1].source, "users.name");
    }

    #[test]
    fn from_subquery_traced() {
        let r = analyze(
            "SELECT s.total FROM (SELECT SUM(v) AS total FROM sales) s",
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].output, "total");
        assert_eq!(r[0].source, "SUM(v)", "子查询内表达式透传: {r:?}");
    }

    #[test]
    fn scalar_subquery_annotated() {
        let r = analyze("SELECT (SELECT max(id) FROM t) AS m FROM users").unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].output, "m");
        assert_eq!(r[0].source, "(子查询)");
    }

    #[test]
    fn union_still_degrades() {
        assert!(analyze("SELECT id FROM a UNION SELECT id FROM b").is_err());
        assert!(analyze("DELETE FROM t").is_err());
    }

    #[test]
    fn depth_limit_degrades() {
        // 三层 CTE 套娃:超出 MAX_DEPTH=2 → 诚实降级
        let sql = "WITH a AS (WITH b AS (WITH c AS (SELECT 1 AS x) SELECT * FROM c) SELECT * FROM b) SELECT * FROM a";
        assert!(analyze(sql).is_err());
    }
}
