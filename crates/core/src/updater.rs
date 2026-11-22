//! 自动更新检查:GitHub Releases latest 与当前版本比较(批次11 只铺了管道,
//! 检查本体此前从未落地——侧栏 ⬆ chip 永远不亮)。
//! 网络/接口失败一律静默(更新检查不该打扰使用)。

/// 查 GitHub 最新 Release;比 `current` 新时返回 (tag, 下载页 URL)
pub async fn check_latest_release(current: &str) -> Result<Option<(String, String)>, String> {
    let resp = reqwest::Client::new()
        .get("https://api.github.com/repos/stouduo/shujing-native/releases/latest")
        .header("User-Agent", "shujing-native")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = v
        .get("tag_name")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let url = v
        .get("html_url")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    if tag.is_empty() || url.is_empty() {
        return Ok(None);
    }
    Ok(version_gt(&tag, current).then_some((tag, url)))
}

/// 宽松版本比较:v 是否比 cur 新。容忍 v 前缀与预发布后缀(按发布段比较,
/// `v1.0.0-rc.1` 视同 1.0.0);逐段数字比,防 0.13.10 < 0.13.9 的字典序错判
fn version_gt(v: &str, cur: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim()
            .trim_start_matches(['v', 'V'])
            .split(['.', '-', '+'])
            .map_while(|p| p.parse::<u64>().ok())
            .collect()
    };
    let (a, b) = (parse(v), parse(cur));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_numeric_and_prefix() {
        assert!(version_gt("v0.14.0", "0.13.2"));
        assert!(version_gt("0.13.10", "0.13.9"), "数字比非字典序");
        assert!(version_gt("V1.0.0", "0.99.99"));
        assert!(!version_gt("0.13.2", "0.13.2"));
        assert!(!version_gt("v0.13.1", "0.13.2"));
        assert!(!version_gt("0.13.2", "0.13.2-beta.1"), "后缀视同发布段");
        assert!(version_gt("1.0.0-rc.1", "0.99.9"), "预发布主体仍参与比较");
        assert!(!version_gt("garbage", "0.13.2"), "解析不出数字段=不更新");
    }
}
