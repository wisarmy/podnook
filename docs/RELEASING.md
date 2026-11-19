# 发布

发布走 GitHub Actions：打 `v*` tag 会构建 arm64 / x86_64 两个 DMG，签名后创建 GitHub Release；手动运行 workflow 只上传构建产物，不创建 Release。

## 一次性准备：发布签名证书

发布包用一张固定的自签名证书签名。它不需要 Apple Developer 账号，但也不能通过公证，所以首次打开会被 Gatekeeper 拦一次；换来的是**后续更新不会重置辅助功能权限**。

```sh
make release-signing
# 或 ./scripts/setup-release-signing.sh
```

脚本会在仓库根目录生成：

- `release-signing.p12`
- `release-signing.pem`

并打印 `MACOS_SIGN_IDENTITY`、`MACOS_CERT_PASSWORD`、`MACOS_CERT_P12`（base64）和 `MACOS_CERT_PEM`（证书 PEM）。到 GitHub：

**Settings → Secrets and variables → Actions → New repository secret**

| Secret | 值 |
|---|---|
| `MACOS_CERT_P12` | 脚本输出的整段 base64 |
| `MACOS_CERT_PASSWORD` | 脚本输出的密码 |
| `MACOS_CERT_PEM` | 脚本输出的 PEM 全文（含 BEGIN/END 行） |
| `MACOS_SIGN_IDENTITY` | `Podnook Release Signing` |

然后把 `.p12` / `.pem` 移到安全的地方（密码管理器、加密磁盘），不要提交。**不要重新生成证书**：身份变了，所有用户的辅助功能授权会重置一次。确实需要轮换时，先发公告，再换新证书。

证书密码会同时存进登录钥匙串，服务名 `com.podnook.release-signing`：

```sh
security find-generic-password -s com.podnook.release-signing -w
```

换机器或重装系统前，把 `.p12` 和密码一起备份到密码管理器；只备份 `.p12` 而没有密码，等于没有私钥。

## 签名身份与内部说明

- 身份：`Podnook Release Signing`
- SHA-1：`3F:49:68:CB:31:C2:7E:6D:FD:FD:B4:E3:20:72:16:B9:13:0E:F3:A3`
- 当前 designated requirement：`identifier "com.podnook.app" and certificate leaf = H"3f4968cb31c27e6dfdfdb4e3207216b9130ef3a3"`（以 `codesign -d -r-` 输出为准）
- 当前没有 Developer ID / notarization，这是 v1 的已知限制。若将来购买开发者账号并切换签名身份，用户会再被要求授权一次，需要先发公告。
- 公开 Release 只写用户安装步骤和“未公证所以首次需放行”；证书指纹、DR、开发者账号采购计划只留在本文档。

## 发布一个版本

1. 改 `nsh.json` 的 `version`（例如 `0.1.0`），提交。
2. 打 tag，推上去：

```sh
git tag v0.1.0
git push origin v0.1.0
```

3. GitHub Actions 会：
   - 校验 tag 与 `nsh.json` 的 `version` 一致；
   - 跑 `cargo test -p podnook-core` 和 `nsh test`；
   - 用 `nsh package --arch arm64` 和 `--arch x86_64` 各构建一次；
   - 用 `Podnook Release Signing` 重签，并检查 designated requirement 包含 `certificate leaf`（不是 `cdhash`）；
   - 生成 `Podnook-<version>-arm64.dmg` 和 `Podnook-<version>-x86_64.dmg`，附 SHA256；
   - 创建 GitHub Release。

4. Release 说明由 `--generate-notes` 从提交生成，提交标题是中文；发布后把说明改成
   英文段落在前、中文原文附后，用户和英文访客都能读懂当前限制与安装步骤。

手动运行：**Actions → Release → Run workflow**。它只构建并上传 artifact，方便在发 tag 前验证。

## 首次打开

发布包没有 Apple 公证，首次打开会被 Gatekeeper 拦：

1. 双击 App，提示无法验证开发者；
2. 打开 **系统设置 → 隐私与安全性**，在“安全性”里点 **仍要打开**；
3. 或在终端执行：

```sh
xattr -dr com.apple.quarantine /Applications/Podnook.app
```

然后到设置里授权辅助功能。之后的版本只要还是同一张证书，权限不会重置。

## 验证签名

```sh
codesign --verify --deep --strict /Applications/Podnook.app
codesign -d -r- /Applications/Podnook.app
# 应该看到：
# designated => identifier "com.podnook.app" and certificate leaf = H"…"
```

如果看到 `cdhash H"…"`，说明包是 ad-hoc 签名，更新会重置权限，发布流程有问题。
