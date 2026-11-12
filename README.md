# Podnook

> A pod for everything you keep. / 把每件你在乎的事，都收进一只豆荚。

本地优先、开源的 macOS **剪贴板历史**工具。复制过的一切留在本机，一个快捷键召回、搜索、粘回，绝不上传。

产品定位、领域词汇与决策记录见 [`CONTEXT.md`](CONTEXT.md)，架构决策见 [`docs/adr/`](docs/adr/)。

## 状态

v0.1（M4 打包）。当前能力：菜单栏常驻、无 Dock 图标、全局热键
⌘⇧V 呼出无边框浮层（失焦或 Esc 收起）；后台监听剪贴板（文本 / 图片 /
文件 / 链接）、去重置顶、隐私标记过滤、SQLite 持久化；搜索、类型筛选、
预览与元信息、pin、删除；回车自动粘回前台 App（⌘↵ 仅复制）。
设置窗口（⌘, 或托盘菜单）：热键录制、保留期、自动粘回、开机自启、忽略 App、
清空历史、权限状态；`make install` 打包、固定签名并安装到 `/Applications`。

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
nsh test         # headless 测试（含 Rust 单测）
cargo test -p podnook-core   # 领域层与数据库单测，不依赖 NSH
```

> ⚠️ NSH 的 debug 构建产物很大：首次完整构建 `target/` 约 15 GB（多套 feature 的依赖各编译一份）。
> 磁盘紧张时先 `cargo clean`。日常迭代用增量构建，不会再涨这么多。

`PODNOOK_DATA_DIR` 仅用于开发：让应用指向另一份数据目录（截图、手工验收），不设置时用 NSH 分配的标准目录。

## 安装与签名

```sh
make signing   # 一次性：在本机登录钥匙串创建自签名证书 Podnook Local Signing
make install   # 打包 → 用固定身份重签 → 校验 → 安装到 /Applications → 启动
```

`nsh package` 只会产出 `.nsh/out/Podnook.app`，**不算安装**；`make install` 才会覆盖
`/Applications/Podnook.app` 并重签名。

为什么要固定签名：macOS 把辅助功能权限绑定到 App 的代码签名。ad-hoc 签名的指定要求是
二进制的 cdhash，每次打包都会变，权限随之失效。固定证书签名的指定要求是
`identifier "com.podnook.app" and certificate leaf = H"…"`，重建不会重置权限。

首次使用只需授权一次：设置 → 「辅助功能」→「请求权限」→ 在系统设置里打开开关。
之后 `make install` 更新不会再要求重新授权。

只有以下情况需要重新授权（`tccutil reset Accessibility com.podnook.app` 后重来一次）：
证书被删除/重建、bundle id 改变、换一台机器。私钥只在本机钥匙串，不进入仓库。

## 数据与权限

- 数据目录：`~/Library/Application Support/com.podnook.app/`
  - `history.db`：剪贴板历史
  - `images/<hash>.png`：图片条目缓存
- **辅助功能（Accessibility）权限**：仅用于「粘回」（模拟 ⌘V）。未授权时自动降级为「仅复制」，
  设置里可一键「请求权限」；未授权时自动降级为「仅复制」。
- 应用无 Dock 图标，常驻菜单栏；默认呼出热键 **⌘⇧V**，点菜单栏图标也可打开。若该组合已被其他 App 的全局热键占用，设置里会提示，换一个组合即可。
- 快捷键：`↵` 粘贴、`⌘↵` 仅复制、`⌘P` 固定、`⌫` 删除、`Esc` 收起（搜索非空时先清空）。

## 许可

见 [`LICENSE`](LICENSE)。
