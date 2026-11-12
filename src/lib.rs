//! Podnook — 本地优先的 macOS 剪贴板历史。
//!
//! 纯 Rust 的 Native Shell 应用：界面在 `ui` 里组装，系统集成在 `clipboard`
//! 与 `history` 里，M1 只做数据层，界面仍用假数据。

#[cfg(target_os = "macos")]
mod clipboard;
mod history;
pub mod theme;
mod ui;

use nsh_core::prelude::*;

/// 应用暴露给 TSX 的组件集合（`nsh.json` 的 `"rust"`）。
/// v0.1 没有 TSX 入口，这里只为 `nsh test` 与将来的入口保留。
#[native_module("podnook/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// 应用根组件，`nsh.json` 的 `"root"`。
#[component]
pub fn App() -> impl IntoElement {
    #[cfg(target_os = "macos")]
    crate::clipboard::start_collector();
    ui::shell()
}
