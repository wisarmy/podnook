//! 应用侧的历史库入口：把 NSH 的数据目录与全局 `Store` 接起来。
//!
//! 领域逻辑与 SQLite 在 `podnook-core` 里，这里只负责「打开一次、全局共享」。

use std::sync::{Arc, OnceLock};

use nsh_core::element::data_dir;
use podnook_core::db::Store;

static STORE: OnceLock<Option<Arc<Store>>> = OnceLock::new();

/// 打开（或复用）全局历史库。没有数据目录时返回 `None`。
pub fn init() -> Option<Arc<Store>> {
    STORE
        .get_or_init(|| match data_dir() {
            Some(dir) => match Store::open(&dir) {
                Ok(store) => {
                    log::info!("history database at {}", dir.join("history.db").display());
                    Some(Arc::new(store))
                }
                Err(e) => {
                    log::error!("open history database: {e}");
                    None
                }
            },
            None => {
                // 正常应用总有数据目录；headless 测试没有，按调试信息处理。
                log::debug!("no data directory (headless test?)");
                None
            }
        })
        .clone()
}
