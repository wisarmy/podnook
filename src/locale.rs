//! 界面语言：跟随系统或手动指定，写入 `settings` 表的 `locale` 键。

use nsh_core::i18n::set_locale;
use nsh_core::platform::Locale;
use podnook_core::db::Store;

use crate::settings;

/// `settings` 表里存语言的键。
pub const SETTING_LOCALE: &str = "locale";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    System,
    Chinese,
    English,
}

impl Choice {
    fn stored(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "zh" => Self::Chinese,
            "en" => Self::English,
            _ => Self::System,
        }
    }

    fn locale(self) -> Option<Locale> {
        match self {
            Self::System => None,
            Self::Chinese => Locale::parse("zh"),
            Self::English => Locale::parse("en"),
        }
    }
}

pub fn current(store: Option<&Store>) -> Choice {
    store
        .and_then(|store| store.get_setting(SETTING_LOCALE).ok().flatten())
        .map(|value| Choice::parse(&value))
        .unwrap_or(Choice::System)
}

/// 应用当前语言。托盘与窗口标题不在这里重装：`set_tray` 的回调绑在调用它的
/// owner 上，必须由 App 组件的 effect 触发，否则设置视图一重建菜单就失效。
pub fn apply(store: Option<&Store>) {
    set_locale(current(store).locale());
}

pub fn set(store: Option<&Store>, choice: Choice) {
    if let Some(store) = store {
        settings::write(store, SETTING_LOCALE, choice.stored());
    }
    apply(store);
}
