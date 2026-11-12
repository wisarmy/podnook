//! Podnook — 本地优先的 macOS 剪贴板历史。
//!
//! 菜单栏常驻：主窗口只作为宿主并立即隐藏，真正的界面是一个命令式创建的
//! 非激活浮层（`WindowKind::Panel`），由全局热键或菜单栏图标呼出。

#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod paste;
mod history;
pub mod theme;
mod ui;

use std::cell::RefCell;
use std::time::Duration;

use nsh_core::desktop::{WindowAttributes, WindowFrameStyle, WindowKind, WindowLevel};
use nsh_core::element::desktop::{
    MenuItem, WindowRef, main_window, open_window, quit, register_global_shortcut, set_tray, tray,
    window as panel_window,
};
use nsh_core::element::{asset, assets};
use nsh_core::platform::Os;
use nsh_core::prelude::*;

thread_local! {
    /// 命令式创建的面板；界面线程与按键回调都在同一个线程。
    static PANEL: RefCell<Option<WindowRef>> = const { RefCell::new(None) };
}

/// 应用暴露给 TSX 的组件集合（`nsh.json` 的 `"rust"`）。
#[native_module("podnook/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
    registry.assets(assets!());
}

/// 应用根组件：宿主窗口的内容。真正的界面在面板里。
#[component]
pub fn App() -> impl IntoElement {
    #[cfg(target_os = "macos")]
    crate::clipboard::start_collector();
    setup_system();
    open_panel_once();

    // 宿主窗口只负责让引擎跑起来，马上藏起来。
    set_timeout(Duration::from_millis(0), || main_window().hide());
    ui::shell()
}

fn panel_attributes() -> WindowAttributes {
    let mut attributes = WindowAttributes::default();
    attributes.frame = Some(WindowFrameStyle::None);
    attributes.transparent = Some(true);
    attributes.shadow = Some(true);
    attributes.level = Some(WindowLevel::Floating);
    attributes.activate = Some(false);
    attributes.focusable = Some(true);
    attributes.close_on_press = Some(false);
    attributes.close_on_escape = Some(false);
    attributes.resizable = Some(true);
    attributes.min_size = Some(Some(size(px(720.0), px(480.0))));
    attributes
}

fn open_panel_once() {
    let opened = PANEL.with(|panel| panel.borrow().is_some());
    if opened {
        return;
    }
    let window = panel_window()
        .id("panel")
        .kind(WindowKind::Panel)
        .title("Podnook")
        .default_size(size(px(980.0), px(640.0)))
        .attributes(panel_attributes())
        .content(ui::shell);
    match open_window(window) {
        Some(window) => {
            // 点到别处就收起面板（不销毁，下次呼出保留状态）。
            let hidden = window.clone();
            window.on_focus_change(move |focused| {
                if !focused {
                    hidden.hide();
                }
            });
            PANEL.with(|panel| *panel.borrow_mut() = Some(window));
        }
        None => log::error!("could not open the Podnook panel"),
    }
}

/// 呼出面板：记住当前前台 App（粘回时还给它），再让面板拿到键盘。
pub fn show_panel() {
    #[cfg(target_os = "macos")]
    crate::paste::remember_frontmost();
    PANEL.with(|panel| match panel.borrow().as_ref() {
        Some(window) => {
            window.focus();
            // 窗口成为 key 之后再聚焦搜索框。
            set_timeout(Duration::from_millis(30), ui::prepare_for_show);
        }
        None => main_window().focus(),
    });
}

/// 收起面板。
pub fn hide_panel() {
    PANEL.with(|panel| match panel.borrow().as_ref() {
        Some(window) => window.hide(),
        None => main_window().hide(),
    });
}

fn setup_system() {
    if !matches!(os(), Os::MacOs) {
        return;
    }

    #[cfg(target_os = "macos")]
    log::info!("accessibility trusted: {}", crate::paste::is_trusted());

    let taken = register_global_shortcut("alt+mod+v", show_panel);
    spawn(async move {
        if !taken.await {
            log::warn!("⌥⌘V is taken by another app");
        }
    });

    set_tray(
        tray()
            .icon(asset!("tray.svg"))
            .tooltip("Podnook")
            .on_click(|_| show_panel())
            .menu([
                MenuItem::new("打开 Podnook").on_select(show_panel),
                #[cfg(target_os = "macos")]
                MenuItem::new("辅助功能设置").on_select(crate::paste::open_accessibility_settings),
                MenuItem::separator(),
                MenuItem::new("退出").on_select(quit),
            ]),
    );
}
