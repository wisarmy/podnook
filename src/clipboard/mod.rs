//! 剪贴板监听：后台轮询 `NSPasteboard.changeCount`，变化时读取快照落库。
//!
//! NSH 的 `Clipboard` 只能按需读写，没有变化通知；这里直接走 AppKit。

mod macos;

pub use macos::write_files;

use std::sync::{Arc, Once};
use std::thread;
use std::time::Duration;

use podnook_core::{db::Store, now_secs, POLL_MS};

static STARTED: Once = Once::new();

/// 启动监听线程（幂等）。没有数据目录时不启动。
pub fn start_collector() {
    STARTED.call_once(|| {
        let Some(store) = crate::history::init() else {
            log::debug!("clipboard collector not started: no data directory");
            return;
        };
        let spawned = thread::Builder::new()
            .name("podnook-clipboard".into())
            .spawn(move || run(store));
        if let Err(e) = spawned {
            log::error!("spawn clipboard thread: {e}");
        }
    });
}

fn run(store: Arc<Store>) {
    let mut last = macos::change_count();
    log::info!("clipboard collector started at changeCount {last}");
    loop {
        thread::sleep(Duration::from_millis(POLL_MS));
        let count = macos::change_count();
        if count == last {
            continue;
        }
        last = count;
        let Some(snapshot) = macos::read_snapshot() else {
            continue;
        };
        if let Err(e) = store.capture(snapshot, now_secs()) {
            log::error!("capture clipboard: {e}");
        } else {
            crate::history::bump();
        }
    }
}
