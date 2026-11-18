#!/bin/sh
# Build, sign and package Podnook for both Mac architectures.
#
# Required environment:
#   VERSION       user-visible version, e.g. 0.1.0
#   BUILD         build number
#   SIGN_IDENTITY code signing identity, e.g. "Podnook Release Signing"
#   OUT_DIR       output directory (default: dist)
#
# The signing identity must already be in a keychain codesign can use.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

: "${VERSION:?VERSION is required}"
: "${BUILD:?BUILD is required}"
: "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"

export PATH="${HOME}/.local/bin:${PATH}"

command -v nsh >/dev/null 2>&1 || {
    echo "✗ 找不到 nsh。先安装 NSH CLI：https://nsh.dev/install.sh" >&2
    exit 1
}

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

for ARCH in arm64 x86_64; do
    echo "==> 构建 $ARCH"
    PKG_DIR="$OUT_DIR/pkg-$ARCH"
    nsh package --target macos --arch "$ARCH" \
        --app-version "$VERSION" --build "$BUILD" --out "$PKG_DIR"

    APP="$PKG_DIR/Podnook.app"
    [ -d "$APP" ] || {
        echo "✗ 没有找到 $APP" >&2
        exit 1
    }

    echo "==> 签名 $ARCH"
    codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
    codesign --verify --deep --strict "$APP"

    REQUIREMENT=$(codesign -d -r- "$APP" 2>&1 | grep 'designated =>' || true)
    echo "$REQUIREMENT"
    case "$REQUIREMENT" in
        *"certificate leaf"*) ;;
        *)
            echo "✗ $ARCH 不是固定证书签名（看到了 cdhash？），更新会重置权限。" >&2
            exit 1
            ;;
    esac

    echo "==> 打包 $ARCH DMG"
    STAGE="$OUT_DIR/dmg-$ARCH"
    mkdir -p "$STAGE"
    cp -R "$APP" "$STAGE/"
    ln -s /Applications "$STAGE/Applications"
    cat > "$STAGE/Install.txt" <<TXT
Podnook ${VERSION} (${ARCH})

1. Drag Podnook into Applications.
2. The first launch is stopped by Gatekeeper once:
   System Settings → Privacy & Security → Open Anyway;
   or in Terminal: xattr -dr com.apple.quarantine /Applications/Podnook.app
3. Open Podnook and grant Accessibility in Settings (needed to paste back).

The release is signed with a fixed self-signed certificate, so updates keep the
Accessibility permission. It is not notarized by Apple, which is why the first
launch needs the confirmation above.

---

安装说明

1. 把 Podnook 拖进 Applications。
2. 首次打开会被 Gatekeeper 拦一次：
   系统设置 → 隐私与安全性 → 仍要打开；
   或终端执行：xattr -dr com.apple.quarantine /Applications/Podnook.app
3. 打开 Podnook，在设置里授权辅助功能（粘回需要）。

发布包用固定自签名证书签名，后续更新不会重置辅助功能权限；
但没有经过 Apple 公证，所以首次打开仍需上面的确认。
TXT

    DMG="$OUT_DIR/Podnook-$VERSION-$ARCH.dmg"
    hdiutil create -volname "Podnook" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
    shasum -a 256 "$DMG" > "$DMG.sha256"
done

echo "✓ 完成："
ls -lh "$OUT_DIR"/Podnook-*.dmg "$OUT_DIR"/Podnook-*.dmg.sha256
