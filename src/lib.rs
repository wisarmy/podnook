//! Podnook — 本地优先的 macOS 剪贴板历史。
//!
//! 菜单栏常驻：主窗口只作为宿主并立即隐藏，真正的界面是一个命令式创建的
//! 非激活浮层（`WindowKind::Panel`），由全局热键或菜单栏图标呼出。

#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod hotkey_recorder;
mod login_item;
#[cfg(target_os = "macos")]
mod paste;
mod history;
mod settings;
pub mod theme;
mod ui;

use std::cell::RefCell;
use std::time::Duration;

use nsh_core::desktop::{WindowAttributes, WindowFrameStyle, WindowKind, WindowLevel, Workspaces};
use nsh_core::element::desktop::{
    MenuItem, WindowRef, main_window, open_window, quit, register_global_shortcut, set_tray, tray,
    unregister_all_global_shortcuts, window as panel_window,
};
use nsh_core::element::{asset, assets};
use nsh_core::platform::Os;
use nsh_core::prelude::*;


thread_local! {
    /// 命令式创建的面板；界面线程与按键回调都在同一个线程。
    static PANEL: RefCell<Option<WindowRef>> = const { RefCell::new(None) };
    static SETTINGS_WINDOW: RefCell<Option<WindowRef>> = const { RefCell::new(None) };
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

/// 让浮层窗口只跟随当前 Space（呼出时移动过去），而不是加入所有 Space。
/// NSH 的 Workspaces 枚举表达不了 `MoveToActiveSpace + FullScreenAuxiliary`，
/// 这里直接调 AppKit：既能覆盖其他 App 的全屏空间，又不会在切 Space 时露一下。
#[cfg(target_os = "macos")]
fn apply_window_behavior() {
    let _ = nsh_core::element::run_on_main_thread(|| {
        use nsh_platform_macos::objc2::MainThreadMarker;
        use nsh_platform_macos::objc2_app_kit::{NSApplication, NSWindowCollectionBehavior};
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let behavior = NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        for window in app.windows().iter() {
            let title = window.title().to_string();
            if title == "Podnook" || title == "Podnook 设置" {
                window.setCollectionBehavior(behavior);
            }
        }
    });
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
    let opened = open_window(window);
    match opened {
        Some(window) => {
            // 点到别处就收起面板（不销毁，下次呼出保留状态）。
            let hidden = window.clone();
            window.on_focus_change(move |focused| {
                if !focused {
                    hidden.hide();
                }
            });
            PANEL.with(|panel| *panel.borrow_mut() = Some(window));
            apply_window_behavior();
            set_timeout(Duration::from_millis(300), apply_window_behavior);
        }
        None => log::error!("could not open the Podnook panel"),
    }
}

/// 呼出面板：记住当前前台 App（粘回时还给它），再让面板拿到键盘。
pub fn show_panel() {
    #[cfg(target_os = "macos")]
    apply_window_behavior();
    #[cfg(target_os = "macos")]
    crate::paste::remember_frontmost();
    // 同一时间只显示 Podnook 的一个窗口：呼出剪贴板时收起设置。
    hide_settings();
    PANEL.with(|panel| match panel.borrow().as_ref() {
        Some(window) => {
            window.focus();
            // 窗口成为 key 之后再聚焦搜索框。
            set_timeout(Duration::from_millis(30), ui::prepare_for_show);
        }
        None => {
            main_window().focus();
        }
    });
}

/// 收起面板。
pub fn hide_panel() {
    PANEL.with(|panel| match panel.borrow().as_ref() {
        Some(window) => window.hide(),
        None => main_window().hide(),
    });
}

/// 收起设置窗口（保留窗口，下次打开时恢复位置和状态）。
pub fn hide_settings() {
    SETTINGS_WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            if window.state().is_some_and(|state| state.visible) {
                window.hide();
            }
        }
    });
}

fn setup_system() {
    if !matches!(os(), Os::MacOs) {
        return;
    }

    #[cfg(target_os = "macos")]
    log::info!("accessibility trusted: {}", crate::paste::is_trusted());

    let store = crate::history::init();
    let config = crate::settings::load(store.as_deref());
    let configured = if nsh_core::keymap::Keystroke::parse(&config.hotkey, Os::MacOs).is_ok() {
        config.hotkey.as_str()
    } else {
        settings::DEFAULT_HOTKEY
    };
    register_hotkey(configured);

    set_tray(
        tray()
            .icon(asset!("tray.svg"))
            .tooltip("Podnook")
            .on_click(|_| show_panel())
            .menu([
                MenuItem::new("打开 Podnook").on_select(show_panel),
                MenuItem::new("设置…").on_select(open_settings),
                MenuItem::separator(),
                MenuItem::new("退出").on_select(quit),
            ]),
    );
}


/// 录制热键前先注销，避免按下旧组合键时面板被呼出。
pub fn suspend_hotkey() {
    if matches!(os(), Os::MacOs) {
        unregister_all_global_shortcuts();
    }
}

/// 注册呼出热键（先清掉旧的）。返回 keystroke 是否合法。
pub fn register_hotkey(keystroke: &str) -> bool {
    register_hotkey_with(keystroke, |_| {})
}

/// 注册呼出热键，并在系统答复是否注册成功时回调。
pub fn register_hotkey_with(
    keystroke: &str,
    on_result: impl Fn(bool) + 'static,
) -> bool {
    if !matches!(os(), Os::MacOs) {
        return false;
    }
    if nsh_core::keymap::Keystroke::parse(keystroke, Os::MacOs).is_err() {
        return false;
    }
    unregister_all_global_shortcuts();
    let taken = register_global_shortcut(keystroke, show_panel);
    let label = keystroke.to_string();
    spawn(async move {
        let ok = taken.await;
        if !ok {
            log::warn!("{label} is taken by another app");
        }
        on_result(ok);
    });
    true
}

/// 打开（或聚焦）设置窗口。设置是浮层窗口，拿到焦点时浮在最前；点其他
/// App 失去焦点后降到普通层级，会退到那个 App 后面（不会一直置顶）。
/// 呼出设置前先收起剪贴板面板，保证同一时间只有一个 Podnook 窗口。
pub fn open_settings() {
    if !matches!(os(), Os::MacOs) {
        return;
    }
    hide_panel();
    if let Some(window) = SETTINGS_WINDOW.with(|slot| slot.borrow().as_ref().cloned()) {
        if window.state().is_some() {
            // 之前失去焦点时降到了普通层级，先浮起来再聚焦。
            let mut attributes = WindowAttributes::default();
            attributes.level = Some(WindowLevel::Floating);
            attributes.workspaces = Some(Workspaces::All);
            window.set(attributes);
            window.focus();
            let refocus = window.clone();
            set_timeout(Duration::from_millis(150), move || refocus.focus());
            apply_window_behavior();
            set_timeout(Duration::from_millis(80), apply_window_behavior);
            return;
        }
    }

    let mut attributes = WindowAttributes::default();
    attributes.activate = Some(false);
    attributes.focusable = Some(true);
    attributes.resizable = Some(false);
    attributes.close_on_escape = Some(true);
    attributes.transparent = Some(true);
    attributes.level = Some(WindowLevel::Floating);
    attributes.workspaces = Some(Workspaces::All);

    let window = panel_window()
        .id("settings")
        .kind(WindowKind::Panel)
        .title("Podnook 设置")
        .default_size(size(px(560.0), px(680.0)))
        .attributes(attributes)
        .content(ui::settings);
    let opened = open_window(window);
    if let Some(window) = opened {
        // 点其他 App 时窗口失去键盘焦点：降到普通层级，退到那个 App 后面。
        // 再点回设置，或用 ⌘, 重新打开时，再浮到最前。
        let settings = window.clone();
        window.on_focus_change(move |focused| {
            let mut attributes = WindowAttributes::default();
            attributes.level = Some(if focused {
                WindowLevel::Floating
            } else {
                WindowLevel::Normal
            });
            // 浮层时跟随所有 Space；失焦变成普通窗口后只留在当前 Space。
            attributes.workspaces = Some(if focused {
                Workspaces::All
            } else {
                Workspaces::Current
            });
            settings.set(attributes);
            if focused {
                apply_window_behavior();
                set_timeout(Duration::from_millis(80), apply_window_behavior);
            }
        });
        window.focus();
        // 面板刚收起时焦点会掉；等窗口真正上屏后再聚焦一次。
        let refocus = window.clone();
        set_timeout(Duration::from_millis(150), move || refocus.focus());
        SETTINGS_WINDOW.with(|slot| *slot.borrow_mut() = Some(window));
        apply_window_behavior();
        set_timeout(Duration::from_millis(80), apply_window_behavior);
    }
}
