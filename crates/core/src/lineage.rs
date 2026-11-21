//! 字段血缘(完整版):解析最外层 SELECT,给出每个输出列的来源。
//! 覆盖:FROM/JOIN/逗号连接 + 表别名消解 + AS/裸列/表达式/t.*/星号。
//! 诚实降级:CTE(WITH)与子查询返回 Err——来源不在最外层,不假装追踪。

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

/// 提取 FROM/JOIN 表清单:表名(可含 db. 前缀)+ 可选别名
fn parse_tables(from_clause: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    // JOIN 边界:JOIN/STRAIGHT_JOIN 与裸逗号;ON/USING 之后的内容跳到下一个 JOIN
    let mut depth = 0i32;
    let chars: Vec<char> = from_clause.chars().collect();
    let mut seg = String::new();
    let mut i = 0;
    // 先按 JOIN 关键词切块,块内取 表 [AS] 别名
    let low = from_clause.to_ascii_lowercase();
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = find_word(&low[start..], "join") {
        let abs = start + pos;
        segments.push((start, abs));
        start = abs + "join".len();
    }
    segments.push((start, from_clause.len()));
    let _ = (&depth, &chars, &i);
    for (a, b) in segments {
        let mut s = from_clause[a..b].trim().to_string();
        // 去掉 JOIN 前缀词(left/right/inner/outer/cross/straight_join 等)与逗号
        s = s.trim_start_matches(|c: char| c == ',').to_string();
        // 剥 JOIN 修饰词
        loop {
            let low_s = s.to_ascii_lowercase();
            let trimmed = low_s
                .trim_start()
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string();
            if matches!(
                trimmed.as_str(),
                "left" | "right" | "inner" | "outer" | "cross" | "full" | "straight_join" | "natural" | ""
            ) && trimmed != ""
            {
                s = s[trimmed.len()..].trim_start().to_string();
            } else {
                break;
            }
        }
        // ON/USING 之后的部分丢弃
        if let Some(pos) = find_word(&s, "on") {
            s = s[..pos].trim().to_string();
        }
        if let Some(pos) = find_word(&s, "using") {
            s = s[..pos].trim().to_string();
        }
        // 逗号多表(隐式连接):每段取第一 token + 别名
        for part in split_top_commas(&s) {
            let toks: Vec<&str> = part.split_whitespace().collect();
            if toks.is_empty() {
                continue;
            }
            let table = toks[0].trim_matches(|c| c == '`' || c == '"').to_string();
            let mut alias = None;
            if toks.len() >= 2 {
                let second = toks[1].to_ascii_lowercase();
                if second == "as" && toks.len() >= 3 {
                    alias = Some(
                        toks[2].trim_matches(|c| c == '`' || c == '"').to_string(),
                    );
                } else if second != "on" && second != "using" && second != "join" {
                    alias = Some(
                        toks[1].trim_matches(|c| c == '`' || c == '"').to_string(),
                    );
                }
            }
            if !table.is_empty() {
                out.push((table, alias));
            }
        }
    }
    out
}

/// 别名消解:token(可能带 db. 前缀)→ 真实表名.qualifier
fn resolve(token: &str, tables: &[(String, Option<String>)]) -> String {
    let t = token.trim();
    if let Some((tbl, alias)) = t.rsplit_once('.') {
        let tbl = tbl.trim_matches(|c| c == '`' || c == '"');
        let alias = alias.trim();
        for (real, al) in tables {
            if let Some(a) = al {
                if a == tbl || a == alias {
                    return format!("{real}.{alias}");
                }
            }
            if real == tbl {
                return format!("{real}.{alias}");
            }
        }
        format!("{tbl}.{alias}")
    } else {
        // 无点:可能是纯别名(u.*)→ 消解为真实表名
        for (real, al) in tables {
            if let Some(a) = al {
                if a == t {
                    return real.clone();
                }
            }
        }
        t.to_string()
    }
}

/// 解析最外层 SELECT 的输出列血缘。
/// 返回 Err = 无法可靠追踪(CTE/子查询/非 SELECT/结构解析失败),调用方展示降级说明。
pub fn analyze(sql: &str) -> Result<Vec<ColLineage>, LineageError> {
    let s = strip_noise(sql).trim().to_string();
    let low = s.to_ascii_lowercase();
    let low_trim = low.trim_start();
    if !low_trim.starts_with("select") {
        return Err(LineageError("非 SELECT 语句".into()));
    }
    if low_trim.starts_with("select") && low.starts_with("with") {
        return Err(LineageError("CTE(WITH)不追踪".into()));
    }
    // 子查询:最外层 FROM 之前出现 "(SELECT"(函数参数括号不算)→ 降级
    if let Some(sel) = find_word(&s, "select") {
        let from_pos = find_word(&s[sel..], "from").map(|p| p + sel);
        let before_from = from_pos.map(|p| &s[sel..p]).unwrap_or(&s[sel..]);
        if has_subquery(&before_from) {
            return Err(LineageError("子查询不追踪".into()));
        }
    }
    let sel_start = find_word(&s, "select").ok_or(LineageError("无 SELECT".into()))?;
    let from_start = find_word(&s, "from")
        .ok_or(LineageError("无 FROM 子句(SELECT 无来源表)".into()))?;
    let select_list = s[sel_start + 6..from_start].trim();
    if select_list.is_empty() {
        return Err(LineageError("空 SELECT 清单".into()));
    }
    // FROM 子句终点:顶层的 where/group/order/limit/having/union/window/offset
    let rest = &s[from_start + 4..];
    let mut from_end = rest.len();
    for kw in ["where", "group", "order", "limit", "having", "union", "window", "offset"] {
        let probe = format!("{rest}!");
        if let Some(p0) = find_word(&probe, kw) {
            let p = p0.min(rest.len());
            from_end = from_end.min(p);
        }
    }
    let from_clause = rest[..from_end].trim().to_string();
    if from_clause.is_empty() {
        return Err(LineageError("空 FROM 子句".into()));
    }
    let tables = parse_tables(&from_clause);
    if tables.is_empty() {
        return Err(LineageError("FROM 解析失败".into()));
    }
    if low.contains(" union ") {
        return Err(LineageError("UNION 不追踪".into()));
    }

    let mut out = Vec::new();
    for item in split_top_commas(select_list) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        // 输出列名:AS 别名 > 裸列尾段 > 表达式截断
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
        // 来源:星号直引 / 别名消解
        let source = if expr == "*" {
            if tables.len() == 1 {
                format!("{}.*", tables[0].0)
            } else {
                "*".to_string()
            }
        } else if expr.ends_with(".*") {
            let t = expr.trim_end_matches(".*");
            format!("{}.*", resolve(t, &tables))
        } else if !expr.contains('(') {
            if expr.contains('.') {
                resolve(&expr, &tables)
            } else if tables.len() == 1 {
                // 单表裸列:归属唯一表
                format!("{}.{}", tables[0].0, expr)
            } else {
                // 多表裸列:归属有歧义,保留裸名
                expr.clone()
            }
        } else {
            expr.clone()
        };
        out.push(ColineagePush { output, source });
    }
    Ok(out.into_iter().map(|c| ColLineage { output: c.output, source: c.source }).collect())
}

struct ColineagePush {
    output: String,
    source: String,
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
    fn cte_and_subquery_and_union_degrade() {
        assert!(analyze("WITH t AS (SELECT 1) SELECT * FROM t").is_err());
        assert!(analyze("SELECT (SELECT max(id) FROM t) AS m FROM users").is_err());
        assert!(analyze("SELECT id FROM a UNION SELECT id FROM b").is_err());
        assert!(analyze("DELETE FROM t").is_err());
    }

    #[test]
    fn db_qualified_table() {
        let r = analyze("SELECT id FROM shop.users").unwrap();
        assert_eq!(r[0].source, "shop.users.id");
    }
}
