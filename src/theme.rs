//! Podnook 的视觉基础：暖色、低饱和的本地工具气质。
//!
//! 所有颜色都走 `AdaptiveColor`，跟随系统深浅色。v0.1 的界面只在这里取色，
//! 不直接写死具体色值——M2 接设置与主题时再扩展。

use nsh_core::prelude::*;

/// 主背景。
pub fn bg() -> AdaptiveColor {
    AdaptiveColor::rgb(0xFBF8F4, 0x191817)
}

/// 侧栏与浮起的表面。
pub fn surface() -> AdaptiveColor {
    AdaptiveColor::rgb(0xF6F1EA, 0x1F1E1C)
}

/// 卡片、搜索框、预览底。
pub fn card() -> AdaptiveColor {
    AdaptiveColor::rgb(0xFFFFFF, 0x242321)
}

/// 主文字。
pub fn fg() -> AdaptiveColor {
    AdaptiveColor::rgb(0x241F1A, 0xF0EBE4)
}

/// 次要文字：说明、占位符、元信息。
pub fn muted_fg() -> AdaptiveColor {
    AdaptiveColor::rgb(0x8A8178, 0x9B948A)
}

/// 分隔线与输入框描边。
pub fn border() -> AdaptiveColor {
    AdaptiveColor::rgb(0xE7E0D7, 0x322F2B)
}

/// 选中行、悬停行。
pub fn accent() -> AdaptiveColor {
    AdaptiveColor::rgb(0xF1E4D4, 0x35312C)
}

/// 选中行上的文字。
pub fn accent_fg() -> AdaptiveColor {
    AdaptiveColor::rgb(0x3D2E1F, 0xF5EDE3)
}

/// 主面板的玻璃背景：半透明，让窗口背后的模糊透出来。
pub fn glass_bg() -> AdaptiveColor {
    bg().opacity(0.78)
}

/// 玻璃面板里的侧栏/浮层表面。
pub fn glass_surface() -> AdaptiveColor {
    surface().opacity(0.60)
}

/// 玻璃面板里的卡片、输入框。
pub fn glass_card() -> AdaptiveColor {
    card().opacity(0.86)
}

/// 玻璃面板里的选中行。
pub fn glass_accent() -> AdaptiveColor {
    accent().opacity(0.72)
}

/// 成功状态（新热键已设置等）。
pub fn success() -> AdaptiveColor {
    AdaptiveColor::rgb(0x2F8F5B, 0x5FBF8A)
}

/// 危险操作（删除、清空）。
pub fn danger() -> AdaptiveColor {
    AdaptiveColor::rgb(0xB5432F, 0xE06A55)
}

/// 品牌色：一点陶土暖橙。
pub fn primary() -> AdaptiveColor {
    AdaptiveColor::rgb(0xC4703A, 0xD98A55)
}

/// 侧栏宽度（点）。
pub const SIDEBAR_WIDTH: f32 = 360.0;

/// 窗口打开时的尺寸（点）。
pub const WINDOW_SIZE: (i32, i32) = (980, 640);

/// 窗口标题。
pub const WINDOW_TITLE: &str = "Podnook";

/// 品牌色上的文字（开关、主按钮）。
pub fn on_primary() -> AdaptiveColor {
    AdaptiveColor::rgb(0xFFFFFF, 0x1A1512)
}
