//! Podnook — 本地优先的 macOS 剪贴板历史。
//!
//! 纯 Rust 的 Native Shell 应用：界面在这里组装，系统集成（剪贴板监听、
//! 粘回、登录项、SQLite）在后续里程碑接入 Rust 侧。

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
    ui::shell()
}
