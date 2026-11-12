//! 粘回：把焦点还给之前的 App，然后合成一次 ⌘V。
//!
//! - 需要 macOS 的「辅助功能」权限（CGEvent 注入）。
//! - 未授权时调用方应降级为「仅复制」。

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::thread;
use std::time::Duration;

use nsh_platform_macos::objc2::rc::Retained;
use nsh_platform_macos::objc2_app_kit::{
    NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
};
use nsh_platform_macos::objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use objc2_application_services::{AXIsProcessTrusted, AXIsProcessTrustedWithOptions};
use objc2_core_foundation::CFDictionary;
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

/// 面板打开前的前台进程，粘回时先把它激活。
static PREVIOUS_PID: AtomicI32 = AtomicI32::new(0);
static PROMPTED: AtomicBool = AtomicBool::new(false);

/// 当前进程是否已有辅助功能权限。
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// 请求辅助功能权限：系统会把本 App 加进「辅助功能」列表并打开设置。
pub fn request_trust() {
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let value = NSNumber::new_bool(true);
    let dict: Retained<NSDictionary<NSString, NSNumber>> =
        NSDictionary::from_slices(&[&*key], &[&*value]);
    // NSDictionary 与 CFDictionary 是 toll-free bridged。
    let cf = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    unsafe {
        AXIsProcessTrustedWithOptions(Some(cf));
    }
}

/// 请求一次辅助功能权限（系统异步弹窗，只在本进程第一次需要时调用）。
pub fn prompt_trust_once() {
    if PROMPTED.swap(true, Ordering::Relaxed) || is_trusted() {
        return;
    }
    request_trust();
}

/// 打开系统设置里的「辅助功能」面板。
pub fn open_accessibility_settings() {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    )) else {
        return;
    };
    let _ = NSWorkspace::sharedWorkspace().openURL(&url);}

/// 打开面板前调用：记住当时的前台 App（不记 Podnook 自己）。
pub fn remember_frontmost() {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
        return;
    };
    let is_self = app
        .bundleIdentifier()
        .map(|id| id.to_string())
        .as_deref()
        == Some("com.podnook.app");
    if !is_self {
        PREVIOUS_PID.store(app.processIdentifier(), Ordering::Relaxed);
    }
}

/// 面板已经隐藏后调用：恢复前台 App，再合成 ⌘V。
pub fn paste_after_hide() {
    if !is_trusted() {
        return;
    }
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(120));
        let pid = PREVIOUS_PID.load(Ordering::Relaxed);
        if pid > 0 {
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                let _ = app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
            }
            thread::sleep(Duration::from_millis(100));
        }
        post_cmd_v();
    });
}

fn post_cmd_v() {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    let source = source.as_ref().map(|source| &**source);
    const V_KEY: u16 = 9; // kVK_ANSI_V

    let Some(down) = CGEvent::new_keyboard_event(source, V_KEY, true) else {
        return;
    };
    CGEvent::set_flags(Some(&down), CGEventFlags::MaskCommand);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));

    if let Some(up) = CGEvent::new_keyboard_event(source, V_KEY, false) {
        CGEvent::set_flags(Some(&up), CGEventFlags::MaskCommand);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    }
}

/// 用系统默认方式打开一个 URL（设置里的 GitHub 链接等）。
pub fn open_external(url: &str) {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return;
    };
    let _ = NSWorkspace::sharedWorkspace().openURL(&url);
}
