//! cron 表达式解析与下次触发计算(标准 5 段 crontab + @宏)。
//!
//! 时间在 naive 域计算(不做时区/DST 处理):调用方传本地 naive 时间,
//! 拿回本地 naive 时间——纯函数零时区依赖,DST 缝隙的一分钟误差对
//! "看接下来几次什么时候跑"的场景无关紧要。
//! dom 与 dow 都受限时的 OR 语义与 vixie cron 一致。

use chrono::{Datelike, Duration, NaiveDateTime, Timelike};

/// 五个字段展开后的集合(dom/dow 另记是否覆盖全范围——OR 语义要用)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    pub raw: String,
    minute: [bool; 60],
    hour: [bool; 24],
    dom: [bool; 32],
    month: [bool; 13],
    dow: [bool; 8],
    dom_all: bool,
    dow_all: bool,
}

const MONTH_NAMES: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
const DOW_NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

const MACROS: [(&str, &str); 7] = [
    ("@hourly", "0 * * * *"),
    ("@daily", "0 0 * * *"),
    ("@midnight", "0 0 * * *"),
    ("@weekly", "0 0 * * 0"),
    ("@monthly", "0 0 1 * *"),
    ("@yearly", "0 0 1 1 *"),
    ("@annually", "0 0 1 1 *"),
];

/// 解析。错误消息带字段名,界内越界/非法词都指到具体字段
pub fn parse(expr: &str) -> Result<Cron, String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("表达式为空".into());
    }
    let expanded = MACROS
        .iter()
        .find(|(m, _)| *m == trimmed)
        .map(|(_, e)| e.to_string())
        .unwrap_or_else(|| trimmed.to_string());
    let parts: Vec<String> = expanded.split_whitespace().map(str::to_string).collect();
    if parts.len() != 5 {
        return Err(format!(
            "应为 5 段(分 时 日 月 周),实际 {} 段;宏(@daily 等)或 6 段 Quartz 秒级暂不支持",
            parts.len()
        ));
    }
    let mut c = Cron {
        raw: expanded,
        minute: [false; 60],
        hour: [false; 24],
        dom: [false; 32],
        month: [false; 13],
        dow: [false; 8],
        dom_all: false,
        dow_all: false,
    };
    c.dom_all = parse_field(&parts[2], 1, 31, None, 0, &mut c.dom[1..])
        .map_err(|e| format!("日(第 3 段): {e}"))?;
    c.dow_all = parse_field(&parts[4], 0, 7, Some(&DOW_NAMES), 0, &mut c.dow)
        .map_err(|e| format!("周(第 5 段): {e}"))?;
    parse_field(&parts[0], 0, 59, None, 0, &mut c.minute).map_err(|e| format!("分(第 1 段): {e}"))?;
    parse_field(&parts[1], 0, 23, None, 0, &mut c.hour).map_err(|e| format!("时(第 2 段): {e}"))?;
    parse_field(&parts[3], 1, 12, Some(&MONTH_NAMES), 1, &mut c.month[1..])
        .map_err(|e| format!("月(第 4 段): {e}"))?;
    Ok(c)
}

/// 解析一个字段,写进 `out`(下标从 0 起,调用方已对齐偏移)。
/// 返回该字段是否覆盖全部合法值(= 等价于 *)
fn parse_field(
    spec: &str,
    lo: u32,
    hi: u32,
    names: Option<&[&str]>,
    name_base: u32,
    out: &mut [bool],
) -> Result<bool, String> {
    if spec.is_empty() {
        return Err("字段为空".into());
    }
    let mut hit = 0u32;
    for part in spec.split(',') {
        if part.is_empty() {
            return Err("出现连续逗号".into());
        }
        let (range_part, step) = match part.split_once('/') {
            Some((r, s)) => {
                let n: u32 = s
                    .parse()
                    .map_err(|_| format!("步进 {s:?} 不是正整数"))?;
                if n == 0 {
                    return Err("步进不能为 0".into());
                }
                (r, n)
            }
            None => (part, 1),
        };
        // 单值带步进 = 该值起到上限(vixie cron 语义)
        let (from, to) = if range_part == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range_part.split_once('-') {
            let a = word_or_num(a, names, name_base, lo, hi)?;
            let b = word_or_num(b, names, name_base, lo, hi)?;
            if a > b {
                return Err(format!("区间 {a}-{b} 起点大于终点(不支持回绕)"));
            }
            (a, b)
        } else {
            let v = word_or_num(range_part, names, name_base, lo, hi)?;
            (v, if step == 1 { v } else { hi })
        };
        let mut v = from;
        while v <= to {
            let idx = (v - lo) as usize;
            if !out[idx] {
                out[idx] = true;
                hit += 1;
            }
            v += step;
        }
    }
    Ok(hit == hi - lo + 1)
}

/// 词或数字:月/周名(JAN/SUN 等,大小写不敏感)或界内数字
fn word_or_num(
    w: &str,
    names: Option<&[&str]>,
    name_base: u32,
    lo: u32,
    hi: u32,
) -> Result<u32, String> {
    let up = w.to_ascii_uppercase();
    if let Some(names) = names {
        if let Some(i) = names.iter().position(|n| *n == up) {
            let v = name_base + i as u32;
            if (lo..=hi).contains(&v) {
                return Ok(v);
            }
        }
    }
    let v: u32 = w
        .parse()
        .map_err(|_| format!("{w:?} 不是合法的数字或名称"))?;
    if !(lo..=hi).contains(&v) {
        return Err(format!("{v} 超出范围 {lo}-{hi}"));
    }
    Ok(v)
}

impl Cron {
    /// 从 `after`(naive,秒被忽略)之后的第 `n` 次触发,最多往后找 5 年。
    /// 年内一次都不触发(如 2 月 30 日)返回不足 n 个
    pub fn next_n(&self, after: NaiveDateTime, n: usize) -> Vec<NaiveDateTime> {
        let mut out = Vec::with_capacity(n);
        let mut t = after - Duration::seconds(after.second() as i64) + Duration::minutes(1);
        let deadline = after + Duration::days(366 * 5);
        while out.len() < n && t <= deadline {
            match self.next_after_from(t) {
                Some(x) => {
                    out.push(x);
                    t = x + Duration::minutes(1);
                }
                None => break,
            }
        }
        out
    }

    /// 单次前进:逐字段跳跃(月 → 日 → 时 → 分),5 年内找不到返回 None
    fn next_after_from(&self, mut t: NaiveDateTime) -> Option<NaiveDateTime> {
        for _ in 0..(366 * 5 * 24 * 60) {
            if !self.month[t.month() as usize] {
                t = self.next_month_start(t)?;
                continue;
            }
            if !self.day_matches(t) {
                t = (t + Duration::days(1)).date().and_hms_opt(0, 0, 0)?;
                continue;
            }
            if !self.hour[t.hour() as usize] {
                let next_hour = t + Duration::hours(1);
                t = next_hour.date().and_hms_opt(next_hour.hour(), 0, 0)?;
                continue;
            }
            if !self.minute[t.minute() as usize] {
                t += Duration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }

    /// 跳到下一个匹配月份的 1 日 00:00(最多扫 12 个月)
    fn next_month_start(&self, t: NaiveDateTime) -> Option<NaiveDateTime> {
        let mut m = t.month();
        let mut y = t.year();
        for _ in 0..12 {
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
            if self.month[m as usize] {
                return NaiveDateTime::from_timestamp_opt(0, 0)
                    .and_then(|_| chrono::NaiveDate::from_ymd_opt(y, m, 1))
                    .and_then(|d| d.and_hms_opt(0, 0, 0));
            }
        }
        None
    }

    /// 日匹配(dom 与 dow 都受限时取 OR,与 vixie cron 一致)
    fn day_matches(&self, t: NaiveDateTime) -> bool {
        let dom_ok = self.dom[t.day() as usize];
        let dow = (t.weekday().num_days_from_sunday()) as usize;
        // 0 与 7 都是周日:集合里存的是原始值,匹配时归一
        let dow_ok = self.dow[dow] || (dow == 0 && self.dow[7]);
        if self.dom_all && self.dow_all {
            true
        } else if self.dom_all {
            dow_ok
        } else if self.dow_all {
            dom_ok
        } else {
            dom_ok || dow_ok
        }
    }

    /// 人话描述(中文):"{日期} {时间}",如"每天 02:30"、"每 5 分钟"
    pub fn describe(&self) -> String {
        let time = self.describe_time();
        let date = self.describe_date();
        // "每天" + 频率型描述("每 N 分钟"/"每小时")时,日期前缀是冗余
        if date.is_empty() || (date == "每天" && time.starts_with("每")) {
            time
        } else {
            format!("{date} {time}")
        }
    }

    fn describe_time(&self) -> String {
        let mins = on(&self.minute, 0);
        let hours = on(&self.hour, 0);
        let hour_step = step_of(&hours, 24);
        let min_step = step_of(&mins, 60);
        if mins.len() == 60 && hours.len() == 24 {
            return "每分钟".into();
        }
        let hour_text = if hours.len() == 24 {
            String::new()
        } else if let Some(d) = hour_step {
            format!("每 {d} 小时")
        } else {
            format!("{} 点", values_text(&hours, None))
        };
        if mins.len() == 60 {
            if hour_text.is_empty() {
                "每小时".into()
            } else {
                format!("{hour_text}的每小时")
            }
        } else if hour_text.is_empty() && mins.len() == 1 && mins[0] == 0 {
            // "0 * * * *":整点触发,习惯就叫每小时
            "每小时".into()
        } else if hour_text.is_empty() && min_step.is_none() {
            format!("每小时第 {} 分", values_text(&mins, None))
        } else if let Some(d) = min_step {
            if hour_text.is_empty() {
                format!("每 {d} 分钟")
            } else {
                format!("{hour_text}的每 {d} 分钟")
            }
        } else {
            // 时:分 点列(同一小时内的多个分合并为 hh:mm1、mm2)
            let mut parts = Vec::new();
            for h in &hours {
                let h = *h as usize;
                let ms: Vec<String> = (0..60)
                    .filter(|m| self.minute[*m as usize] && self.hour[h])
                    .map(|m| format!("{m:02}"))
                    .collect();
                if ms.is_empty() {
                    continue;
                }
                parts.push(format!("{h:02}:{}", compress_text(&ms)));
            }
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                parts.join("、")
            }
        }
    }

    fn describe_date(&self) -> String {
        let months = on(&self.month, 0);
        let doms = on(&self.dom, 0);
        let dows = on(&self.dow, 0);
        let dow_all = self.dow_all;
        let dom_all = self.dom_all;
        let mon_all = months.len() == 12;
        let dow_text = || -> String {
            ranges_of(&dows)
                .iter()
                .map(|(a, b)| {
                    if a == b {
                        format!("每周{}", DOW_CN[*a as usize])
                    } else {
                        format!("每周{}-周{}", DOW_CN[*a as usize], DOW_CN[*b as usize])
                    }
                })
                .collect::<Vec<_>>()
                .join("、")
        };
        let dom_text = || -> String {
            format!("每月{}日", values_text(&doms, None))
        };
        let mon_text = |s: &str| -> String {
            format!("{}月{}", values_text(&months, None), s)
        };
        if mon_all && dom_all && dow_all {
            return "每天".into();
        }
        if mon_all {
            if dom_all && dow_all {
                "每天".into()
            } else if dom_all {
                dow_text()
            } else if dow_all {
                dom_text()
            } else {
                format!("{} 或 {}", dom_text(), dow_text())
            }
        } else if dom_all && dow_all {
            mon_text("每天")
        } else if dom_all {
            mon_text(&format!("每周{}", DOW_CN[dows[0] as usize])) // 月份受限+dow:逐月逐周太啰嗦,取首个;完整集合由 next 列表兜底
        } else if dow_all {
            mon_text(&format!("{}日", values_text(&doms, None)))
        } else {
            mon_text(&format!("{}日 或 每周{}", values_text(&doms, None), DOW_CN[dows[0] as usize]))
        }
    }
}

const DOW_CN: [&str; 7] = ["日", "一", "二", "三", "四", "五", "六"];

/// 排序值压成连续区间段 [(a,b)]
fn ranges_of(v: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &x in v {
        match out.last_mut() {
            Some(r) if r.1 + 1 == x => r.1 = x,
            _ => out.push((x, x)),
        }
    }
    out
}

/// 集合里为 true 的下标(按字段基址偏移)
fn on(field: &[bool], base: usize) -> Vec<u32> {
    field
        .iter()
        .enumerate()
        .filter(|(_, v)| **v)
        .map(|(i, _)| (i + base) as u32)
        .collect()
}

/// 集合是等差步进(≥3 个元素、公差 > 1)时返回公差
fn step_of(v: &[u32], max_n: usize) -> Option<u32> {
    if v.len() < 3 || v.len() >= max_n {
        return None;
    }
    let d = v[1] - v[0];
    if d <= 1 {
        return None;
    }
    v.windows(2).all(|w| w[1] - w[0] == d).then_some(d)
}

/// 排序后的值压成 "1-5、15" 形式
fn values_text(v: &[u32], names: Option<&[&str]>) -> String {
    let txt = compress_text(&v.iter().map(|x| x.to_string()).collect::<Vec<_>>());
    match names {
        Some(names) => txt
            .split("、")
            .map(|seg| {
                seg.split('-')
                    .map(|x| match x.parse::<usize>() {
                        Ok(i) if i < names.len() => names[i].to_string(),
                        _ => x.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("-")
            })
            .collect::<Vec<_>>()
            .join("、"),
        None => txt,
    }
}

/// 已格式化的字符串列表压成 "a-b、c":相邻数字合并为区间,
/// 合并的两端与单项都用原文(保留 "00" 这类前导零)
fn compress_text(items: &[String]) -> String {
    let nums: Vec<Option<u32>> = items.iter().map(|s| s.parse::<u32>().ok()).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < nums.len() {
        if let Some(start) = nums[i] {
            let mut j = i;
            while j + 1 < nums.len() && nums[j + 1] == Some(nums[j].unwrap() + 1) {
                j += 1;
            }
            if j > i && nums[j].unwrap() - start > 1 {
                out.push_str(&format!("{}-{}", items[i], items[j]));
            } else {
                out.push_str(&items[i]);
            }
            i = j + 1;
        } else {
            out.push_str(&items[i]);
            i += 1;
        }
        if i < nums.len() {
            out.push_str("、");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/data/cron_vectors.json")).unwrap()
    }

    fn parse_dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    /// python 暴力逐分钟匹配生成的基准(与本实现无共同代码)
    #[test]
    fn next_n_matches_bruteforce_reference() {
        let v = v();
        let start = parse_dt(v["start"].as_str().unwrap());
        for (expr, want) in v["vectors"].as_object().unwrap() {
            let c = parse(expr).unwrap_or_else(|e| panic!("{expr}: {e}"));
            let got: Vec<String> = c
                .next_n(start, 5)
                .iter()
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .collect();
            let want: Vec<String> = want.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
            assert_eq!(got, want, "{expr}");
        }
    }

    /// 非法表达式(与 python 参考口径一致)必须报错,且消息带字段名
    #[test]
    fn rejects_malformed() {
        let bad = [
            ("* * * *", "段"),
            ("* * * * * *", "段"),
            ("60 * * * *", "分"),
            ("* 24 * * *", "时"),
            ("* * 0 * *", "日"),
            ("* * 32 * *", "日"),
            ("* * * 13 *", "月"),
            ("* * * * 8", "周"),
            ("a * * * *", "分"),
            ("*/0 * * * *", "步进"),
            ("1-5-10 * * * *", "分"),
            ("1/5/9 * * * *", "分"),
            ("", "空"),
            ("@never", "段"),
            ("1,,2 * * * *", "逗号"),
            ("* * 5-2 * *", "起点"),
        ];
        for (expr, needle) in bad {
            let err = parse(expr).unwrap_err();
            assert!(err.contains(needle), "{expr:?}: {err}");
        }
    }

    #[test]
    fn dow_seven_is_sunday_and_macros_expand() {
        // 0 与 7 都是周日;宏展开为等价表达式
        // 7 存进集合(原始值),匹配时归一为周日
        let c = parse("0 0 * * 7").unwrap();
        assert!(c.dow[7]);
        assert_eq!(parse("@daily").unwrap().raw, "0 0 * * *");
        let c = parse("@yearly").unwrap();
        assert!(c.month[1] && c.dom[1]);
        // 宏是字面匹配:大小写变体不是宏,按普通表达式解析会失败
        assert!(parse("@YEARLY").is_err());
    }

    #[test]
    fn describe_covers_common_shapes() {
        assert_eq!(parse("* * * * *").unwrap().describe(), "每分钟");
        assert_eq!(parse("*/5 * * * *").unwrap().describe(), "每 5 分钟");
        assert_eq!(parse("0 * * * *").unwrap().describe(), "每小时");
        assert_eq!(parse("30 2 * * *").unwrap().describe(), "每天 02:30");
        assert_eq!(parse("0 9 * * 1-5").unwrap().describe(), "每周一-周五 09:00");
        assert_eq!(parse("0 0 1 * *").unwrap().describe(), "每月1日 00:00");
        assert_eq!(parse("0 0 1 1 *").unwrap().describe(), "1月1日 00:00");
        // dom 与 dow 都受限:OR 语义要体现在描述里
        let d = parse("10 8 1,15 * 1").unwrap().describe();
        assert!(d.contains("每月1、15日") && d.contains("每周一") && d.contains(" 或 "), "{d}");
        // 月份受限
        assert_eq!(parse("0 0 1 1 *").unwrap().describe(), "1月1日 00:00");
        let d = parse("0 12 * JAN-MAR *").unwrap().describe();
        assert!(d.contains("1-3月") && d.contains("12:00"), "{d}");
    }

    /// 无一年内触发的表达式:next_n 返回空而不是死循环
    /// 单值带步进 = 该值起到字段上限(vixie cron 语义):"5/15" → 5,20,35,50
    #[test]
    fn single_value_step_expands_to_max() {
        let c = parse("5/15 * * * *").unwrap();
        let on: Vec<u32> = c
            .minute
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(i, _)| i as u32)
            .collect();
        assert_eq!(on, [5, 20, 35, 50], "{:?}", on);
    }

    #[test]
    fn impossible_field_terminates() {
        // 2 月 30 日不存在
        let c = parse("0 0 30 2 *").unwrap();
        let start = NaiveDateTime::parse_from_str("2026-06-15T10:30:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        assert!(c.next_n(start, 3).is_empty());
    }
}
