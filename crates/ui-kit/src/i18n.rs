//! i18n:极简字符串表(中/英)。t!(key) 取当前语言文案;key 不存在回落 key 本身。
//! 框架先行:宿主按文件逐步迁移;未迁移的键自动回落中文原文(与现有 UI 一致)。

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    EN.store(on, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    EN.load(Ordering::Relaxed)
}

/// t!("English", "中文"):按当前语言取文案,默认中文原文。
#[macro_export]
macro_rules! t {
    ($en:expr, $zh:expr) => {
        $crate::i18n::t($en, $zh)
    };
}

/// 取文案:en 有英文用英文,否则原样(中文原文即默认语言)。
/// 生命周期跟随入参(调用点都是 'static 字面量,直接使用无碍)
pub fn t<'a>(en: &'a str, zh: &'a str) -> &'a str {
    if is_english() {
        en
    } else {
        zh
    }
}

/// 常用界面文案(en 对 / zh 对)。宿主逐步迁移到 t!()
pub mod strings {
    pub const NEW_CONNECTION_ZH: &str = "新建连接";
    pub const NEW_CONNECTION_EN: &str = "New connection";
    pub const SEARCH_TABLES_ZH: &str = "搜索表";
    pub const SEARCH_TABLES_EN: &str = "Search tables";
    pub const SAVE_ZH: &str = "保存";
    pub const SAVE_EN: &str = "Save";
    pub const CANCEL_ZH: &str = "取消";
    pub const CANCEL_EN: &str = "Cancel";
    pub const CLOSE_ZH: &str = "关闭";
    pub const CLOSE_EN: &str = "Close";
    pub const REFRESH_ZH: &str = "刷新";
    pub const REFRESH_EN: &str = "Refresh";
}
