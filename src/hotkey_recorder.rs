//! 在设置窗口里「按下组合键」录制全局热键。
//!
//! 用 AppKit 的本地事件监听捕获下一次 keyDown，转换成 NSH 的 keystroke
//! 字符串（`alt+mod+v`）。录制期间调用方应先注销当前全局热键。

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use nsh_core::element::run_on_main_thread;
use nsh_platform_macos::objc2::rc::Retained;
use nsh_platform_macos::objc2::runtime::AnyObject;
use nsh_platform_macos::objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

/// `Some(None)` = 取消，`Some(Some(key))` = 录到组合键。
static RESULT: Mutex<Option<Option<String>>> = Mutex::new(None);

thread_local! {
    static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// 取走录制结果（由设置窗口轮询）。
pub fn take_result() -> Option<Option<String>> {
    RESULT.lock().ok().and_then(|mut result| result.take())
}

/// 开始录制；必须在主线程装监听。
pub fn start() {
    if let Ok(mut result) = RESULT.lock() {
        *result = None;
    }
    let _ = run_on_main_thread(install);
}

/// 取消录制（例如用户点了「取消」）。
pub fn cancel() {
    let _ = run_on_main_thread(|| {
        remove_monitor();
        set_result(None);
    });
}

fn set_result(value: Option<String>) {
    if let Ok(mut result) = RESULT.lock() {
        *result = Some(value);
    }
}

fn install() {
    remove_monitor();
    let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        handle(event)
    });
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    MONITOR.with(|slot| *slot.borrow_mut() = monitor);
}

fn remove_monitor() {
    MONITOR.with(|slot| {
        if let Some(monitor) = slot.borrow_mut().take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    });
}

fn handle(event: NonNull<NSEvent>) -> *mut NSEvent {
    let event = unsafe { event.as_ref() };
    if event.isARepeat() {
        return std::ptr::null_mut();
    }
    let code = event.keyCode();
    // 修饰键本身不结束录制。
    if matches!(code, 54..=58 | 60..=63) {
        return event as *const NSEvent as *mut NSEvent;
    }
    let flags = event.modifierFlags();
    let modifiers = flags
        & (NSEventModifierFlags::Command
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Option
            | NSEventModifierFlags::Shift);

    // Esc 取消。
    if code == 53 && modifiers.is_empty() {
        remove_monitor();
        set_result(None);
        return std::ptr::null_mut();
    }

    let command = flags.contains(NSEventModifierFlags::Command);
    let control = flags.contains(NSEventModifierFlags::Control);
    let option = flags.contains(NSEventModifierFlags::Option);
    let shift = flags.contains(NSEventModifierFlags::Shift);

    // 全局热键至少要有一个非 Shift 的修饰键，避免抢走普通输入。
    if !(command || control || option) {
        return std::ptr::null_mut();
    }

    let chars = event.charactersIgnoringModifiers().map(|value| value.to_string());
    let Some(key) = key_name(code, chars.as_deref()) else {
        return std::ptr::null_mut();
    };

    let mut parts = Vec::new();
    if control {
        parts.push("ctrl");
    }
    if option {
        parts.push("alt");
    }
    if shift {
        parts.push("shift");
    }
    if command {
        parts.push("mod");
    }
    parts.push(&key);
    let keystroke = parts.join("+");

    remove_monitor();
    set_result(Some(keystroke));
    std::ptr::null_mut()
}

fn key_name(code: u16, chars: Option<&str>) -> Option<String> {
    let special = match code {
        49 => Some("space"),
        36 => Some("enter"),
        48 => Some("tab"),
        51 => Some("backspace"),
        117 => Some("delete"),
        123 => Some("left"),
        124 => Some("right"),
        125 => Some("down"),
        126 => Some("up"),
        115 => Some("home"),
        119 => Some("end"),
        116 => Some("pageup"),
        121 => Some("pagedown"),
        122 => Some("f1"),
        120 => Some("f2"),
        99 => Some("f3"),
        118 => Some("f4"),
        96 => Some("f5"),
        97 => Some("f6"),
        98 => Some("f7"),
        100 => Some("f8"),
        101 => Some("f9"),
        109 => Some("f10"),
        103 => Some("f11"),
        111 => Some("f12"),
        _ => None,
    };
    if let Some(name) = special {
        return Some(name.to_string());
    }

    let text = chars?;
    let character = text.chars().next()?;
    if character.is_ascii_alphanumeric() {
        return Some(character.to_ascii_lowercase().to_string());
    }
    if ",./;'[]\\-=`".contains(character) {
        return Some(character.to_string());
    }
    None
}
