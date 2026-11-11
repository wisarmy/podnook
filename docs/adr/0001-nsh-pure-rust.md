# ADR 0001 — 用 Native Shell v0.5.0 纯 Rust 重写

- 状态：已采纳
- 日期：2026-11-11

## 背景

Podnook 的原型是 Rust + GPUI（`gpui-kit` + `arboard` + `enigo` + `global-hotkey`），只跑通了一部分系统集成，UI 尚未成型。现在决定改用 Native Shell（NSH）重做，第一版功能对齐 Raycast Clipboard History。

NSH 是 Rust-first 的原生 UI 框架：macOS 后端是 AppKit，元素与响应式图在 Rust 里，支持纯 Rust 应用，或 Rust 应用叠 TSX 屏幕。

## 决策

**用 NSH v0.5.0 写一个纯 Rust 应用**：UI 用 NSH 元素构建器 + `nsh/base` 无样式组件 + 自建主题；系统集成（剪贴板监听、按键注入、登录项、SQLite）全部用 Rust 与 macOS 原生 API 实现。

## 备选方案

1. **纯 Rust NSH**（采纳）
2. **Rust 应用 + TSX 屏幕**：UI 迭代快、有 HMR，但多一层 QuickJS 与 `#[native_module]` 桥；本项目的系统集成本就在 Rust，收益不足以抵消复杂度。
3. **自建 AppKit 宿主 + 嵌入 NSH TSX `AppView`**：等于自己维护窗口/引擎生命周期，还要处理 HostChannel；收益更小。
4. **继续用 GPUI Kit**：已有原型，但需要自己解决列表/搜索/预览等全部 UI，且没有 NSH 的 `nsh/base`；与「用 NSH 重做」的目标冲突。

## 理由

- NSH 官方一等公民就是纯 Rust 应用：`nsh run` 直接跑 Rust root，不带 QuickJS，打包产物也不含 `bundle.nshc`。
- 窗口、托盘、全局热键、非激活 panel 都有 Rust API（`open_window`、`set_tray`、`register_global_shortcut`）。
- `nsh_platform_macos` 重导出 `objc2` / `objc2_app_kit` / `objc2_foundation`，`run_on_main_thread` 可调用 AppKit 主线程 API；`data_dir()` 官方建议用来放 SQLite。
- 单一语言、单一构建，出问题时不需要跨 Rust/JS 边界定位。

## 后果

- 没有 TSX 热替换：每次改动走 `nsh run` 的增量编译 + 重启。UI 组件需要拆细。
- 需要自己实现 NSH 缺失的能力：
  - 剪贴板变化监听（`NSPasteboard.changeCount` 轮询）
  - 粘回（`CGEventPost` 发 ⌘V + 辅助功能授权）
  - 开机自启（`SMAppService`，失败回退 LaunchAgent）
  - 历史存储（`rusqlite`，NSH 的 `Storage` 只有 5 MB）
- 锁定 NSH v0.5.0：托盘/热键/浮层都是 0.5 新增，0.4 不可用；0.5 无兼容基线，升级需重读对应 tag 的文档。
- 打包为 ad-hoc 签名 `.app`；v0.1 不做 Developer ID 与公证。
