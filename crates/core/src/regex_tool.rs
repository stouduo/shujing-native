//! 正则测试:编译、全部匹配(含捕获组,字节偏移转字符偏移)、替换预览。
//!
//! 引擎是 `regex` crate(依赖树里 mysql_async 已带入,不新增编译产物):
//! 语法子集与 PCRE 不同——不支持环视与回引,错误消息如实告知;
//! 替换语法是 `$0`/`$1`/`${name}`(与 JS 一致,不是 python 的 `\1`)。

use regex::{Regex, RegexBuilder};

pub const FLAG_I: u8 = 1; // 大小写不敏感
pub const FLAG_M: u8 = 2; // ^ $ 匹配每行
pub const FLAG_S: u8 = 4; // . 匹配换行

/// 一处匹配:文本、字符偏移(u32,与 UI/向量口径一致)、捕获组(第 1 组起)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexResult {
    pub matches: Vec<RegexMatch>,
    pub replaced: String,
}

/// 编译并跑一遍:全部匹配 + 替换预览。pattern/replacement 的语法错误都在 Err 里
pub fn run(pattern: &str, flags: u8, text: &str, replacement: &str) -> Result<RegexResult, String> {
    let re = RegexBuilder::new(pattern)
        .case_insensitive(flags & FLAG_I != 0)
        .multi_line(flags & FLAG_M != 0)
        .dot_matches_new_line(flags & FLAG_S != 0)
        .build()
        .map_err(|e| format!("{e}"))?;
    let mut matches = Vec::new();
    for c in re.captures_iter(text) {
        let whole = c.get(0).expect("第 0 组必在");
        let byte_start = whole.start();
        // regex 的偏移是字节;向量与 UI 用字符偏移
        let to_chars = |b: usize| text[..b].chars().count();
        let groups = c
            .iter()
            .skip(1)
            .map(|g| g.map(|g| g.as_str().to_string()))
            .collect();
        matches.push(RegexMatch {
            text: whole.as_str().to_string(),
            start: to_chars(byte_start),
            end: to_chars(whole.end()),
            groups,
        });
    }
    let replaced = re.replace_all(text, replacement).into_owned();
    Ok(RegexResult { matches, replaced })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/data/regex_vectors.json")).unwrap()
    }

    /// python re 生成、另一实现另一语法的基准(替换语义已按 $n 转写回 python)
    #[test]
    fn matches_and_replace_reference() {
        for case in v()["cases"].as_array().unwrap() {
            let pat = case["pattern"].as_str().unwrap();
            let flags_s = case["flags"].as_str().unwrap();
            let flags = flags_s
                .chars()
                .map(|c| match c {
                    'i' => FLAG_I,
                    'm' => FLAG_M,
                    's' => FLAG_S,
                    _ => 0,
                })
                .sum::<u8>();
            let text = case["text"].as_str().unwrap();
            let r = run(pat, flags, text, case["repl"].as_str().unwrap())
                .unwrap_or_else(|e| panic!("{pat}: {e}"));
            let want: Vec<(String, usize, usize)> = case["matches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    (
                        m["text"].as_str().unwrap().to_string(),
                        m["start"].as_u64().unwrap() as usize,
                        m["end"].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            let got: Vec<(String, usize, usize)> = r
                .matches
                .iter()
                .map(|m| (m.text.clone(), m.start, m.end))
                .collect();
            assert_eq!(got, want, "{pat}: 匹配位置或文本不一致");
            // 捕获组逐组对
            for (m, wm) in r.matches.iter().zip(case["matches"].as_array().unwrap()) {
                let want_groups: Vec<Option<String>> = wm["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|g| g.as_str().map(|s| s.to_string()))
                    .collect();
                assert_eq!(m.groups, want_groups, "{pat}: 捕获组不一致");
            }
            assert_eq!(
                r.replaced,
                case["replaced"].as_str().unwrap(),
                "{pat}: 替换结果不一致"
            );
        }
    }

    /// rust regex 不支持的语法(环视/回引):如实报错,而不是静默吞掉或 panic
    #[test]
    fn unsupported_syntax_is_an_error() {
        for pat in [r"(?=ab)", r"(?<=ab)c", r"(a)\1"] {
            let err = run(pat, 0, "test", "").unwrap_err();
            assert!(!err.is_empty(), "{pat} 应报错");
        }
    }

    /// 常见书写错误(未闭合的组/括号)也有可读错误
    #[test]
    fn syntax_errors_are_readable() {
        let err = run("(ab", 0, "", "").unwrap_err();
        assert!(!err.is_empty());
        let err = run("*", 0, "", "").unwrap_err();
        assert!(!err.is_empty());
    }

    /// 空匹配也逐个产出(regex 与 python 的推进语义一致,已被向量覆盖;
    /// 这里补一个显式用例防退化)
    #[test]
    fn empty_matches_advance() {
        let r = run("a*", 0, "bab", "").unwrap();
        let spans: Vec<(usize, usize)> = r.matches.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, [(0, 0), (1, 2), (3, 3)], "空匹配应逐位推进");
    }
}
