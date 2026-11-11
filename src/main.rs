//! 直接在 macOS 上运行 Rust 应用（`cargo run`）。
//!
//! `nsh run` / `nsh package` 会使用 CLI 生成的 runner；这个文件只服务于
//! 本地用 Cargo 快速迭代。

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<nsh_app_podnook::App>()),
        platform::AppOptions {
            title: nsh_app_podnook::theme::WINDOW_TITLE.to_owned(),
            size: nsh_app_podnook::theme::WINDOW_SIZE,
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Podnook v0.1 只支持 macOS。");
}
