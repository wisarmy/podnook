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

/// 应用当前语言：界面文案、托盘与窗口标题都跟着换。
pub fn apply(store: Option<&Store>) {
    set_locale(current(store).locale());
    crate::install_tray();
    crate::refresh_titles();
}

pub fn set(store: Option<&Store>, choice: Choice) {
    if let Some(store) = store {
        settings::write(store, SETTING_LOCALE, choice.stored());
    }
    apply(store);
}
