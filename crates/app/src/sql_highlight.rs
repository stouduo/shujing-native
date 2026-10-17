//! SQL 轻量语法高亮:TextEdit layouter + 简单 tokenizer(关键字/字符串/数字/注释)。

use crate::theme;
use egui::text::LayoutJob;

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "DROP",
    "ALTER",
    "ADD",
    "INDEX",
    "VIEW",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "ON",
    "AS",
    "ORDER",
    "BY",
    "GROUP",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "UNION",
    "ALL",
    "EXISTS",
    "IN",
    "LIKE",
    "BETWEEN",
    "IS",
    "ASC",
    "DESC",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "UNIQUE",
    "DEFAULT",
    "AUTO_INCREMENT",
    "IF",
    "WITH",
    "REPLACE",
    "TRUNCATE",
    "SHOW",
    "DATABASES",
    "TABLES",
    "DESCRIBE",
    "EXPLAIN",
    "USING",
    "CROSS",
];

/// 生成 SQL 高亮 LayoutJob(供 TextEdit::layouter 使用)
pub fn highlight(sql: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = egui::FontId::monospace(12.0);
    let mut rest = sql;

    while !rest.is_empty() {
        let (token, next) = next_token(rest);
        if token.is_empty() {
            break;
        }
        let color = token_color(token);
        job.append(token, 0.0, egui::TextFormat::simple(font.clone(), color));
        rest = next;
    }
    job
}

fn next_token(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    // 行注释
    if s.starts_with("--") {
        let end = s.find('\n').unwrap_or(s.len());
        return s.split_at(end);
    }
    // 块注释
    if s.starts_with("/*") {
        let end = s.find("*/").map(|i| i + 2).unwrap_or(s.len());
        return s.split_at(end);
    }
    // 字符串字面量
    if bytes[0] == b'\'' {
        let mut i = 1;
        while i < s.len() {
            if bytes[i] == b'\\' {
                i += 2; // 转义
                continue;
            }
            if bytes[i] == b'\'' {
                return s.split_at((i + 1).min(s.len()));
            }
            i += 1;
        }
        return s.split_at(s.len());
    }
    // 标识符/数字/其他:读到下一个分隔符
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
    if is_word(bytes[0]) {
        let mut i = 1;
        while i < bytes.len() && is_word(bytes[i]) {
            i += 1;
        }
        return s.split_at(i);
    }
    // 空白/符号:单个消费(空白成组保持换行缩进)
    if bytes[0].is_ascii_whitespace() {
        let mut i = 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        return s.split_at(i);
    }
    s.split_at(1)
}

fn token_color(tok: &str) -> egui::Color32 {
    if tok.starts_with("--") || tok.starts_with("/*") {
        theme::c().sql_comment
    } else if tok.starts_with('\'') {
        theme::c().sql_string
    } else if tok
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        theme::c().sql_number
    } else {
        let upper: String = tok.to_ascii_uppercase();
        if upper.split_whitespace().all(|w| KEYWORDS.contains(&w)) && !upper.is_empty() {
            theme::c().sql_keyword
        } else {
            theme::c().text2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_covers_all_cases() {
        let sql = "SELECT `name` FROM t -- note\nWHERE id = 'a\\'b' AND n > 12 /* block */";
        let mut rest = sql;
        let mut consumed = String::new();
        let mut guard = 0;
        while !rest.is_empty() && guard < 10_000 {
            let (tok, next) = next_token(rest);
            if tok.is_empty() {
                panic!("zero-length token at {rest:?}");
            }
            consumed.push_str(tok);
            rest = next;
            guard += 1;
        }
        assert_eq!(consumed, sql);
    }

    #[test]
    fn string_literal_with_escape() {
        let (tok, rest) = next_token("'a\\'b' OR 1");
        assert_eq!(tok, "'a\\'b'");
        assert_eq!(rest, " OR 1");
    }
}
