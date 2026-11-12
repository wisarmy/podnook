//! 设置项：读写 `settings` 表，并提供默认值。

use podnook_core::db::Store;
use podnook_core::{
    SETTING_AUTO_PASTE, SETTING_HOTKEY, SETTING_RETENTION_COUNT, SETTING_RETENTION_DAYS,
};

/// 默认呼出热键。
pub const DEFAULT_HOTKEY: &str = "alt+mod+v";

/// 可选热键（NSH keystroke）。避开了各 App 常用的 ⌘⇧V。
pub const HOTKEYS: &[&str] = &["alt+mod+v", "alt+mod+c", "ctrl+alt+v", "mod+shift+space"];

#[derive(Debug, Clone)]
pub struct Settings {
    pub hotkey: String,
    pub retention_count: i64,
    pub retention_days: i64,
    pub auto_paste: bool,
    pub ignored_bundles: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            retention_count: 1000,
            retention_days: 30,
            auto_paste: true,
            ignored_bundles: Vec::new(),
        }
    }
}

pub fn load(store: Option<&Store>) -> Settings {
    let Some(store) = store else {
        return Settings::default();
    };
    Settings {
        hotkey: store
            .get_setting(SETTING_HOTKEY)
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_HOTKEY.to_string()),
        retention_count: read_i64(store, SETTING_RETENTION_COUNT, 1000),
        retention_days: read_i64(store, SETTING_RETENTION_DAYS, 30),
        auto_paste: read_bool(store, SETTING_AUTO_PASTE, true),
        ignored_bundles: store.ignored_bundles().unwrap_or_default(),
    }
}

pub fn read_i64(store: &Store, key: &str, default: i64) -> i64 {
    store
        .get_setting(key)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub fn read_bool(store: &Store, key: &str, default: bool) -> bool {
    store
        .get_setting(key)
        .ok()
        .flatten()
        .map(|value| value == "1" || value == "true")
        .unwrap_or(default)
}

pub fn write(store: &Store, key: &str, value: impl ToString) {
    if let Err(error) = store.set_setting(key, &value.to_string()) {
        log::error!("save setting {key}: {error}");
    }
}

pub fn write_bool(store: &Store, key: &str, value: bool) {
    write(store, key, if value { "1" } else { "0" });
}

pub fn save_ignored(store: &Store, bundles: &[String]) {
    match serde_json::to_string(bundles) {
        Ok(json) => write(store, podnook_core::SETTING_IGNORED_BUNDLES, json),
        Err(error) => log::error!("serialize ignored bundles: {error}"),
    }
}
