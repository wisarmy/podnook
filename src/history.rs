//! 应用侧的历史库入口：把 NSH 的数据目录与全局 `Store` 接起来。
//!
//! 领域逻辑与 SQLite 在 `podnook-core` 里，这里只负责「打开一次、全局共享」。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use nsh_core::element::data_dir;
use podnook_core::db::Store;

static STORE: OnceLock<Option<Arc<Store>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 数据目录：默认用 NSH 给应用分配的目录。
/// `PODNOOK_DATA_DIR` 只用于开发时指向一份已有历史（截图、手工验收）。
pub fn data_directory() -> Option<PathBuf> {
    std::env::var_os("PODNOOK_DATA_DIR")
        .map(PathBuf::from)
        .or_else(data_dir)
}

/// 有新的捕获写入时递增，界面据此判断要不要重新查库。
pub fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// 当前的数据库版本号。
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// 旧版本把 Retina 图片的像素尺寸当成了显示尺寸（998×762 而不是 499×381）。
/// 缓存 PNG 里有 DPI 信息，启动时读一次修正历史条目。
#[cfg(target_os = "macos")]
fn migrate_image_dimensions(store: &Store) {
    use podnook_core::ClipKind;

    const KEY: &str = "image_dimensions_points_v1";
    if matches!(store.get_setting(KEY), Ok(Some(_))) {
        return;
    }

    let mut changed = 0;
    if let Ok(clips) = store.list(5000) {
        for clip in clips {
            if clip.kind != ClipKind::Image {
                continue;
            }
            let Some(path) = clip.image_path.as_deref() else {
                continue;
            };
            let Some((width, height)) = crate::clipboard::image_file_size(path) else {
                continue;
            };
            if clip.image_w != Some(width as i64) || clip.image_h != Some(height as i64) {
                if store.set_image_dimensions(clip.id, width, height).is_ok() {
                    changed += 1;
                }
            }
        }
    }
    if changed > 0 {
        log::info!("migrated {changed} image dimensions from pixels to points");
    }
    let _ = store.set_setting(KEY, "1");
}

/// 打开（或复用）全局历史库。没有数据目录时返回 `None`。
pub fn init() -> Option<Arc<Store>> {
    STORE
        .get_or_init(|| match data_dir() {
            Some(dir) => match Store::open(&dir) {
                Ok(store) => {
                    log::info!("history database at {}", dir.join("history.db").display());
                    let store = Arc::new(store);
                    #[cfg(target_os = "macos")]
                    migrate_image_dimensions(&store);
                    Some(store)
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
