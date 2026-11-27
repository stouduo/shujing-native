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

/// 视图 DDL 的定义体(MySQL 的 SHOW CREATE VIEW / SQLite·PG 的 CREATE VIEW .. AS)
/// 常是一整行 SELECT:结构页里就是一坨。把 AS 之后的 SELECT 体走 format_sql,
/// 头部(ALGORITHM/DEFINER/VIEW 名等)逐字保留——头部含 DEFINER=`x`@`%` 这类
/// token,过格式化器会被加空格,改变可复制执行的文本。
/// 不是"CREATE .. AS select"形状的输入(普通表的 DDL 等)原样返回。
pub fn format_view_ddl(create_view: &str) -> String {
    let Some(pos) = find_as_select(create_view) else {
        return create_view.to_string();
    };
    // 闸门:头部必须是 CREATE ... VIEW(列注释里"形如 AS select 的注释"这类
    // 字符串不能触发——普通表的 DDL 必须逐字保留)
    let head_is_view = {
        let head_lower = create_view[..pos].to_lowercase();
        head_lower.starts_with("create")
            && head_lower
                .split_inclusive(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_') == "view")
    };
    if !head_is_view {
        return create_view.to_string();
    }
    let head_end = pos + " AS ".len();
    // 头部保留到 " AS " 为止(尾随空白归并)
    let head = create_view[..pos].trim_end();
    let body = format_sql(create_view[head_end..].trim_start());
    format!("{head} AS\n{}", body.trim_end())
}

/// 找 " AS " 且其后(跳过空白)以 SELECT/WITH 开头的那一处;取最后一个满足的
/// (定义体内部还会有 "expr AS alias",但 remainder 不是 SELECT 开头)
fn find_as_select(sql: &str) -> Option<usize> {
    let lower = sql.to_lowercase();
    let mut found = None;
    let mut from = 0usize;
    while let Some(i) = lower[from..].find(" as ") {
        let pos = from + i;
        let rest = sql[pos + 4..].trim_start();
        let is_body = {
            let b = rest.as_bytes();
            (b.len() >= 6 && b[..6].eq_ignore_ascii_case(b"select"))
                || (b.len() >= 4 && b[..4].eq_ignore_ascii_case(b"with"))
        };
        if is_body {
            found = Some(pos);
        }
        from = pos + 4;
    }
    found
}

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
    // 压缩多余空行 + 行尾清理——字符串字面量内部原样保留
    // (多行字符串里的空行/行尾空白属于数据,清了再执行=改数据)
    let mut in_str: Option<char> = None; // 当前字符串引号类型
    let mut lines: Vec<String> = Vec::new();
    for l in out.lines() {
        if in_str.is_some() {
            lines.push(l.to_string()); // 字符串内部:原样
        } else {
            let t = l.trim_end().to_string();
            if !t.trim().is_empty() {
                lines.push(t);
            }
        }
        // 逐字符更新引号状态(跳过 SQL 注释行)
        let is_comment = l.trim_start().starts_with("--") || l.trim_start().starts_with("#");
        if !is_comment {
            let bytes: Vec<char> = l.chars().collect();
            let mut j = 0;
            while j < bytes.len() {
                let c = bytes[j];
                if let Some(q) = in_str {
                    // 双写引号是转义,跳过
                    if c == q {
                        if bytes.get(j + 1) == Some(&q) {
                            j += 2;
                            continue;
                        }
                        in_str = None;
                    }
                } else if c == '\'' || c == '"' || c == '`' {
                    in_str = Some(c);
                }
                j += 1;
            }
        }
    }
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
            i += 1; // 跳过首段开引号
            loop {
                while i < bytes.len() {
                    if bytes[i] as char == quote {
                        // 双写转义
                        if i + 1 < bytes.len() && bytes[i + 1] as char == quote {
                            i += 2;
                            continue;
                        }
                        i += 1; // 越过闭引号
                        break;
                    }
                    // 反斜杠转义(字符串)
                    if quote == '\'' && bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                // 限定名链 `db`.`table`.`col`(及 `t`.col):闭引号后跟 . 且再跟
                // 引号段/词 → 整链是一个词。此前拆成 `a` / . / `b` 三个,格式化后
                // 点两侧被塞进空格,改变可复制执行的文本
                if (quote == '`' || quote == '"')
                    && bytes.get(i) == Some(&b'.')
                    && i + 1 < bytes.len()
                {
                    let n = sql[i + 1..].chars().next().unwrap_or('\0');
                    if n == quote {
                        i += 2; // 越过点与下一段开引号,while 从段内首字符接着扫
                        continue;
                    }
                    if n.is_alphanumeric() || n == '_' {
                        i += 1; // 越过点,再吃整段裸词
                        while i < bytes.len() {
                            let ch = sql[i..].chars().next().unwrap_or('\0');
                            if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                                i += ch.len_utf8();
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                }
                break;
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

    /// `db`.`table`.`col` 限定名链是一个词:点两侧不能被塞进空格
    /// (塞了就改变可复制执行的文本)
    #[test]
    fn dotted_identifier_chain_stays_one_token() {
        let out = format_sql("select `u`.`id` from `shop`.`users` `u`");
        assert!(out.contains("`u`.`id`"), "{out}");
        assert!(out.contains("`shop`.`users`"), "{out}");
        assert!(!out.contains(". "), "{out}");
        assert!(!out.contains(" ."), "{out}");
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

#[cfg(test)]
mod literal_tests {
    use super::format_sql;

    /// 多行字符串字面量内的空行/行尾空白不能被行级后处理改写(改了再执行=改数据)
    #[test]
    fn multiline_string_preserved_verbatim() {
        let sql = "INSERT INTO t (a) VALUES ('line1\n\nline3  ');";
        let out = format_sql(sql);
        assert!(
            out.contains("'line1\n\nline3  '"),
            "字符串字面量必须原样保留,实际: {out:?}"
        );
    }

    #[test]
    fn code_blank_lines_still_compressed() {
        let out = format_sql("SELECT 1;\n\n\n   \nSELECT 2;");
        assert!(!out.contains("\n\n\n"), "代码区多余空行仍应压缩: {out:?}");
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    /// MySQL SHOW CREATE VIEW 的定义体是一整行(服务器不排版):结构页的视图 DDL
    /// 由此变成一坨。头部(ALGORITHM/DEFINER/VIEW 名)必须逐字保留——用户会复制
    /// 执行,过格式化器会被加空格(如 `x`@`%` 变 `x` @ `%`);只有 AS 后的
    /// SELECT 体格式化
    #[test]
    fn view_ddl_formats_body_keeps_header_verbatim() {
        let one_line = "CREATE ALGORITHM=UNDEFINED DEFINER=`prod`@`%` SQL SECURITY DEFINER \
VIEW `shop`.`v_order` AS select o.id, o.user_id, u.name from orders o left join users u \
on o.user_id = u.id where o.status = 1 and o.amount > 100 order by o.id desc limit 50";
        let out = format_view_ddl(one_line);
        // 头部逐字保留
        let head = "CREATE ALGORITHM=UNDEFINED DEFINER=`prod`@`%` SQL SECURITY DEFINER VIEW `shop`.`v_order` AS";
        assert_eq!(
            out.lines().next().unwrap(),
            head,
            "头部必须逐字保留: {out:?}"
        );
        // SELECT 体多行
        assert!(out.contains("\nSELECT o.id"), "{out}");
        assert!(out.contains("\nFROM orders o"), "{out}");
        assert!(out.contains("\nLEFT JOIN users u"), "{out}");
        assert!(out.contains("\n  ON o.user_id = u.id"), "{out}");
        assert!(out.contains("\nWHERE o.status = 1"), "{out}");
        assert!(out.contains("\nORDER BY o.id DESC"), "{out}");
    }

    /// 普通表的 SHOW CREATE TABLE 不能被误改:列注释里就算有 " AS select "
    /// (字符串字面量内)也不该触发
    #[test]
    fn table_ddl_with_as_in_comment_untouched() {
        let ddl = "CREATE TABLE `t` (\n  `a` int DEFAULT NULL COMMENT '形如 AS select 的注释'\n) ENGINE=InnoDB";
        assert_eq!(format_view_ddl(ddl), ddl);
    }

    /// SQLite/PG 风格的 CREATE VIEW .. AS select(单行)同样格式化
    #[test]
    fn plain_create_view_formats() {
        let out = format_view_ddl("CREATE VIEW v AS select a, b from t where a > 1");
        assert_eq!(out.lines().next().unwrap(), "CREATE VIEW v AS");
        assert!(out.contains("\nFROM t"), "{out}");
    }
}
