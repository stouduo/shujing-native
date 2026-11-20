//! SQL 编辑器实时静态检查(零依赖、不连库):UPDATE/DELETE 无 WHERE、
//! 括号不配对、引号不闭合。扫描前先剥注释(--、#、/* */)与字符串字面量
//! (''与\'转义),避免字符串/注释里的关键字与括号误报。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintKind {
    /// UPDATE 无 WHERE
    UpdateNoWhere,
    /// DELETE 无 WHERE
    DeleteNoWhere,
    /// 单引号不闭合
    UnclosedQuote,
    /// 括号不配对
    UnbalancedParens,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub kind: LintKind,
    /// 1-based 行号(0=无法定位);引号不闭合=开引号行
    pub line: usize,
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 词边界匹配:code[pos..] 以关键字整词出现(前后非单词字符)
fn word_at(code: &str, pos: usize, kw: &str) -> bool {
    let b = code.as_bytes();
    if pos + kw.len() > b.len() {
        return false;
    }
    if !code[pos..pos + kw.len()].eq_ignore_ascii_case(kw) {
        return false;
    }
    let before_ok = pos == 0 || !is_word(b[pos - 1]);
    let after = pos + kw.len();
    let after_ok = after >= b.len() || !is_word(b[after]);
    before_ok && after_ok
}

fn line_at(text: &str, pos: usize) -> usize {
    text[..pos.min(text.len())].matches('\n').count() + 1
}

/// 扫描一遍生成 code-only 文本:注释与字符串内容替换为空格(换行保留,行号不变),
/// 反引号标识符内容也剥掉(其中的关键字不参与匹配)。
fn strip_noise(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    #[derive(PartialEq)]
    enum St {
        Normal,
        LineComment,
        BlockComment,
        Str(u8), // ' " `
    }
    let mut st = St::Normal;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match st {
            St::Normal => match c {
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    st = St::LineComment;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                }
                b'#' => {
                    st = St::LineComment;
                    out.push(b' ');
                    i += 1;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    st = St::BlockComment;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                }
                q @ (b'\'' | b'"' | b'`') => {
                    st = St::Str(q);
                    out.push(b' ');
                    i += 1;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            },
            St::LineComment => {
                out.push(if c == b'\n' { b'\n' } else { b' ' });
                if c == b'\n' {
                    st = St::Normal;
                }
                i += 1;
            }
            St::BlockComment => {
                out.push(if c == b'\n' { b'\n' } else { b' ' });
                if c == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    out.push(b' ');
                    st = St::Normal;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            St::Str(q) => {
                out.push(if c == b'\n' { b'\n' } else { b' ' });
                if c == b'\\' && q != b'`' && i + 1 < bytes.len() {
                    // 反斜杠转义:连下一个字符一起吃掉
                    out.push(b' ');
                    i += 2;
                } else if c == q {
                    if q != b'`' && i + 1 < bytes.len() && bytes[i + 1] == q {
                        // '' 翻倍转义:第二个引号一并剥掉
                        out.push(b' ');
                        i += 2;
                    } else {
                        st = St::Normal;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 段 [start,end) 内是否存在整词 WHERE
fn has_where_word(code: &str, start: usize, end: usize) -> bool {
    let seg = &code[start..end];
    let upper = seg.to_ascii_uppercase();
    let mut p = 0;
    while let Some(rel) = upper[p..].find("WHERE") {
        if word_at(code, start + p + rel, "WHERE") {
            return true;
        }
        p += rel + 1;
        if p >= seg.len() {
            break;
        }
    }
    false
}

/// 单引号不闭合:独立复扫原文本(注释内的引号不参与)
fn quote_left_open(sql: &str) -> Option<usize> {
    let bytes = sql.as_bytes();
    let mut st = 0u8; // 0 normal 1 line-comment 2 block-comment 3 single
    let mut open_line = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match st {
            0 => match c {
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    st = 1;
                    i += 2;
                }
                b'#' => {
                    st = 1;
                    i += 1;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    st = 2;
                    i += 2;
                }
                b'\'' => {
                    st = 3;
                    open_line = line_at(sql, i);
                    i += 1;
                }
                _ => i += 1,
            },
            1 => {
                if c == b'\n' {
                    st = 0;
                }
                i += 1;
            }
            2 => {
                if c == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    st = 0;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                if c == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if c == b'\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                        i += 2;
                    } else {
                        st = 0;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    (st == 3).then_some(open_line)
}

/// 静态检查主入口(纯函数,O(n))
pub fn lint(sql: &str) -> Vec<Finding> {
    if sql.trim().is_empty() {
        return Vec::new();
    }
    let code = strip_noise(sql);
    let mut out = Vec::new();

    // 1) 括号配对(仅代码区;多退少补都算不配对)
    let mut depth: i64 = 0;
    let mut first_bad: Option<usize> = None;
    for (i, c) in code.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 && first_bad.is_none() {
                    first_bad = Some(i);
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        out.push(Finding {
            kind: LintKind::UnbalancedParens,
            line: line_at(&code, first_bad.unwrap_or(code.len())),
        });
    }

    // 2) 单引号不闭合
    if let Some(line) = quote_left_open(sql) {
        out.push(Finding {
            kind: LintKind::UnclosedQuote,
            line,
        });
    }

    // 3) UPDATE/DELETE 语句无 WHERE:按分号切段(仅代码区)
    let mut idx = 0usize;
    let bytes = code.as_bytes();
    let mut seg_start: Option<(usize, bool)> = None; // (起点, 是否写语句)
    let mut found: Vec<(LintKind, usize)> = Vec::new();
    loop {
        let at_end = idx == bytes.len();
        if at_end || bytes[idx] == b';' {
            if let Some((start, is_write)) = seg_start.take() {
                if is_write && !has_where_word(&code, start, idx) {
                    let kind = if word_at(&code, start, "DELETE") {
                        LintKind::DeleteNoWhere
                    } else {
                        LintKind::UpdateNoWhere
                    };
                    found.push((kind, start));
                }
            }
            if at_end {
                break;
            }
            idx += 1;
            continue;
        }
        if seg_start.is_none() && !bytes[idx].is_ascii_whitespace() {
            let is_write =
                word_at(&code, idx, "UPDATE") || word_at(&code, idx, "DELETE");
            seg_start = Some((idx, is_write));
        }
        idx += 1;
    }
    for (kind, pos) in found {
        out.push(Finding {
            kind,
            line: line_at(&code, pos),
        });
    }
    out.sort_by_key(|f| f.line);
    out
}

#[cfg(test)]
mod lint_tests {
    use super::*;

    #[test]
    fn update_delete_without_where() {
        let f = lint("UPDATE t SET a=1");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, LintKind::UpdateNoWhere);
        assert_eq!(f[0].line, 1);

        let f = lint("delete from t");
        assert_eq!(f[0].kind, LintKind::DeleteNoWhere);

        let f = lint("UPDATE t SET a=1 WHERE id=2;");
        assert!(f.iter().all(|x| x.kind != LintKind::UpdateNoWhere));
    }

    #[test]
    fn where_in_string_or_comment_not_counted() {
        // WHERE 只在字符串里 = 没有 WHERE
        let f = lint("UPDATE t SET c='WHERE x=1'");
        assert_eq!(f[0].kind, LintKind::UpdateNoWhere);
        // WHERE 只在注释里 = 没有 WHERE
        let f = lint("UPDATE t SET a=1 -- WHERE id=1\n");
        assert_eq!(f[0].kind, LintKind::UpdateNoWhere);
        // # 注释同样剥除
        let f = lint("UPDATE t SET a=1 # WHERE id=1\n");
        assert_eq!(f[0].kind, LintKind::UpdateNoWhere);
    }

    #[test]
    fn multi_statement_segments_independent() {
        let f = lint("UPDATE a SET x=1 WHERE id=1; DELETE FROM b;");
        assert!(f.iter().any(|x| x.kind == LintKind::DeleteNoWhere));
        assert!(f.iter().all(|x| x.kind != LintKind::UpdateNoWhere));
        // 行号:第二段的 DELETE 在第 2 行
        let f = lint("UPDATE a SET x=1 WHERE id=1;\nDELETE FROM b");
        assert_eq!(f[0].line, 2);
    }

    #[test]
    fn parens_balance() {
        assert!(lint("SELECT (1+(2)) FROM t").is_empty());
        assert!(lint("SELECT (1 FROM t")
            .iter()
            .any(|x| x.kind == LintKind::UnbalancedParens));
        // 多出的右括号也报
        assert!(lint("SELECT 1) FROM t")
            .iter()
            .any(|x| x.kind == LintKind::UnbalancedParens));
        // 字符串/注释里的括号不算
        assert!(lint("SELECT '( ) /* ) */' FROM t WHERE a=1").is_empty());
    }

    #[test]
    fn unclosed_quote() {
        let f = lint("SELECT 'abc\nFROM t");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, LintKind::UnclosedQuote);
        assert_eq!(f[0].line, 1);
        // '' 与 \' 转义不触发
        assert!(lint("SELECT 'a''b' FROM t").is_empty());
        assert!(lint("SELECT 'a\\'b' FROM t").is_empty());
        // 注释里的引号不参与
        assert!(lint("SELECT 1 -- it's fine\nFROM t").is_empty());
    }

    #[test]
    fn backtick_and_prefix_words() {
        // 反引号里的 where 不算
        let f = lint("UPDATE `where` SET `where`=1");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, LintKind::UpdateNoWhere);
        // UPDATE 前缀词(updates)不是语句起点
        assert!(lint("SELECT * FROM updates").is_empty());
    }

    #[test]
    fn empty_and_boundary() {
        assert!(lint("").is_empty());
        assert!(lint("   \n  ").is_empty());
        // WHEREX 不算 WHERE
        let f = lint("UPDATE t SET a=1 WHEREX=2");
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn alter_table_delete_has_where_ok() {
        // CH mutation 语法:ALTER TABLE t DELETE WHERE ... 不报
        assert!(lint("ALTER TABLE t DELETE WHERE id=1").is_empty());
    }
}