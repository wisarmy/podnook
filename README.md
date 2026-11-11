# Podnook

> A pod for everything you keep. / 把每件你在乎的事，都收进一只豆荚。

本地优先、开源的 macOS **剪贴板历史**工具。复制过的一切留在本机，一个快捷键召回、搜索、粘回，绝不上传。

产品定位、领域词汇与决策记录见 [`CONTEXT.md`](CONTEXT.md)，架构决策见 [`docs/adr/`](docs/adr/)。

## 状态

v0.1（M0 骨架阶段）。当前能力：可运行的空壳界面。
计划与里程碑见 `CONTEXT.md`。

## 技术栈

- [Native Shell](https://nsh.dev) **v0.5.0**（锁定版本）+ 纯 Rust，无 TSX / QuickJS
- 系统层：`NSPasteboard` 轮询、`CGEventPost` 粘回、`SMAppService` 登录项
- 存储：`rusqlite`（`history.db`）+ 图片文件缓存

> ⚠️ NSH 0.5 刚发布，官方文档站仍停留在 0.4。**一律以 tag `v0.5.0` 的 repo 文档为准**：
> `https://raw.githubusercontent.com/longbridge/nsh/v0.5.0/website/...`

## 环境要求

- macOS 14+
- Rust（edition 2024）
- NSH CLI **v0.5.0**

安装 NSH CLI（**不要用 Homebrew**，它当前只到 v0.4.0 且仅 arm64）：

```sh
curl -fsSL https://nsh.dev/install.sh | sh
# 或下载 v0.5.0 归档：
# https://github.com/longbridge/nsh/releases/download/v0.5.0/nsh-macos-arm64.tar.gz
```

## 开发

```sh
nsh run          # 运行 Rust 应用（增量编译 + 重启）
nsh check        # 类型/编译检查
nsh test         # headless 测试
nsh package --target macos   # 打包为 ad-hoc 签名的 .app
```

## 数据与权限

- 数据目录：`~/Library/Application Support/com.podnook.app/`
  - `history.db`：剪贴板历史
  - `images/<hash>.png`：图片条目缓存
- **辅助功能（Accessibility）权限**：仅用于「粘回」（模拟 ⌘V）。未授权时自动降级为「仅复制」。
- 应用无 Dock 图标，常驻菜单栏；默认呼出热键 **⌥⌘V**。

## 许可

见 [`LICENSE`](LICENSE)。
