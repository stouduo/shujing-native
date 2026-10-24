//! SQL 格式化:关键字大写 + 主子句换行 + 条件缩进。
//! 务实版(与 sql-formatter 的常用行为对齐,不追求完美):字符串/注释原样保留。

/// 主关键字(另起一行)
const MAJOR: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP BY", "HAVING", "ORDER BY", "LIMIT", "OFFSET", "UNION",
    "UNION ALL", "INSERT INTO", "VALUES", "UPDATE", "SET", "DELETE FROM", "LEFT JOIN",
    "RIGHT JOIN", "INNER JOIN", "OUTER JOIN", "FULL JOIN", "CROSS JOIN", "JOIN",
];
/// 子关键字(WHERE/ON/HAVING 后另起一行并缩进)
const MINOR: &[&str] = &["AND", "OR", "ON", "WHEN", "THEN", "ELSE"];
/// 只大写不换行
const UPPER_ONLY: &[&str] = &[
    "IN", "NOT", "LIKE", "AS", "BETWEEN", "EXISTS", "IS", "ASC", "DESC", "DISTINCT",
];

pub fn format_sql(sql: &str) -> String {
    let tokens = tokenize(sql);
    let mut out = String::with_capacity(sql.len() * 2);
    let mut depth = 0usize;
    let mut first_token = true;
    let mut i = 0usize;

    let emit_newline = |out: &mut String, indent: usize| {
        out.push('\n');
        for _ in 0..indent {
            out.push(' ');
        }
    };

    while i < tokens.len() {
        let t = &tokens[i];
        if t == "(" {
            if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push('(');
            depth += 1;
            emit_newline(&mut out, depth * 2);
            first_token = false;
            i += 1;
            continue;
        }
        if t == ")" {
            depth = depth.saturating_sub(1);
            emit_newline(&mut out, depth * 2);
            out.push(')');
            i += 1;
            continue;
        }
        let upper = t.to_uppercase();
        let two_word = if i + 1 < tokens.len() {
            format!("{upper} {}", tokens[i + 1].to_uppercase())
        } else {
            String::new()
        };
        let major = MAJOR.iter().any(|k| *k == upper || *k == two_word);
        let minor = MINOR.iter().any(|k| *k == upper);
        let indent = depth * 2 + if minor { 2 } else { 0 };

        if (major || minor) && !first_token {
            emit_newline(&mut out, indent);
        } else if t == "," {
            // 逗号:任何深度都换行到同层内容缩进
            out.push(',');
            emit_newline(&mut out, depth * 2);
            first_token = false;
            i += 1;
            continue;
        } else if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
            out.push(' ');
        }

        if major && MAJOR.iter().any(|k| *k == two_word) {
            // 双词主关键字(GROUP BY / LEFT JOIN …)
            out.push_str(&two_word);
            i += 2;
        } else if major || minor || UPPER_ONLY.iter().any(|k| *k == upper) {
            out.push_str(&upper);
            i += 1;
        } else {
            out.push_str(t);
            i += 1;
        }
        first_token = false;
    }
    // 压缩多余空行 + 行尾清理
    let lines: Vec<String> = out
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .collect();
    let mut joined = lines.join("\n");
    // 保持无分号输入不补分号
    joined.push('\n');
    joined
}

/// 分词:字符串/注释原样为一个 token,其余按空白切
fn tokenize(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    // 按"字节索引 + char"推进:bytes[i] as char 会把多字节字符的高位字节
    // 误判为拉丁字母并在字符中间切开(SQLite 允许无引号中文标识符,会 panic)
    let bytes = sql.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = sql[i..].chars().next().unwrap_or('\0');
        let clen = c.len_utf8();
        if c.is_whitespace() {
            i += clen;
            continue;
        }
        // 注释 -- 与 #
        if (c == '-' && i + 1 < bytes.len() && bytes[i + 1] == b'-') || c == '#' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.push(sql[start..i].to_string());
            continue;
        }
        // 块注释
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(sql[start..i].to_string());
            continue;
        }
        // 字符串/标识符引号
        if c == '\'' || c == '"' || c == '`' {
            let start = i;
            let quote = c;
            i += 1;
            while i < bytes.len() {
                if bytes[i] as char == quote {
                    // 双写转义
                    if i + 1 < bytes.len() && bytes[i + 1] as char == quote {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                // 反斜杠转义(字符串)
                if quote == '\'' && bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            out.push(sql[start..i.min(bytes.len())].to_string());
            continue;
        }
        // 普通词/符号
        let start = i;
        if c.is_alphanumeric() || c == '_' || c == '.' || c == '$' {
            while i < bytes.len() {
                let ch = sql[i..].chars().next().unwrap_or('\0');
                let ch_len = ch.len_utf8();
                if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '$' {
                    i += ch_len;
                } else {
                    break;
                }
            }
        } else {
            i += clen;
        }
        out.push(sql[start..i].to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_select() {
        let out = format_sql("select id, name from users where age > 18 order by id limit 10");
        assert!(out.contains("SELECT id"));
        assert!(out.contains("\nFROM users"));
        assert!(out.contains("\nWHERE age > 18"));
        assert!(out.contains("\nORDER BY id"));
        assert!(out.contains("\nLIMIT 10"));
        // 逗号字段换行
        assert!(out.lines().any(|l| l.trim_start().starts_with("name")));
    }

    #[test]
    fn keeps_strings_intact() {
        let out = format_sql("SELECT * FROM t WHERE name = 'where from and or'");
        assert!(out.contains("'where from and or'"));
    }

    #[test]
    fn join_and_and() {
        let out = format_sql("select a from t1 left join t2 on t1.a=t2.a where x=1 and y=2");
        assert!(out.contains("\nLEFT JOIN t2"));
        assert!(out.contains("\n  ON t1.a = t2.a"), "{out}");
        assert!(out.contains("\n  AND y = 2"), "{out}");
    }

    #[test]
    fn update_statement() {
        let out = format_sql("update users set name = 'x', age = 3 where id = 1");
        assert!(out.starts_with("UPDATE users"));
        assert!(out.contains("\nSET name"));
        assert!(out.contains("\nWHERE id = 1"));
    }

    #[test]
    fn parens_indent() {
        let out = format_sql("select * from t where id in (1,2,3)");
        assert!(out.contains("IN (\n  1"));
    }
}

#[cfg(test)]
mod char_tests {
    use super::*;

    #[test]
    fn format_cjk_identifier_no_panic() {
        // SQLite 允许无引号中文标识符:此前按字节切词会 panic
        let out = format_sql("select 姓名 from 订单表 where 金额 > 100");
        assert!(out.to_uppercase().contains("SELECT"), "{out}");
        assert!(out.contains("姓名"), "{out}");
    }
}
