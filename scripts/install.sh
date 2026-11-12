#!/bin/sh
# 打包 → 固定身份重签 → 校验 → 安装到 /Applications → 启动。
#
# 每次更新都走这一个命令，签名身份不变，macOS 辅助功能权限就不会重置。
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

export PATH="${HOME}/.local/bin:${PATH}"

IDENTITY="Podnook Local Signing"
APP="/Applications/Podnook.app"
BUILT=".nsh/out/Podnook.app"

if ! command -v nsh >/dev/null 2>&1; then
    echo "✗ 找不到 nsh。先安装 NSH CLI：https://nsh.dev/install.sh" >&2
    exit 1
fi

if ! security find-identity -v -p codesigning 2>/dev/null | grep -F "\"${IDENTITY}\"" >/dev/null 2>&1; then
    echo "==> 没有找到 ${IDENTITY}，先创建签名证书"
    "$ROOT/scripts/setup-signing.sh"
fi

echo "==> 打包"
nsh package --target macos

echo "==> 退出正在运行的 Podnook"
pkill -f "Podnook.app/Contents/MacOS/Podnook" 2>/dev/null || true
i=0
while pgrep -f "Podnook.app/Contents/MacOS/Podnook" >/dev/null 2>&1; do
    i=$((i + 1))
    if [ "$i" -ge 30 ]; then
        echo "✗ Podnook 没有退出，请先手动退出再重试。" >&2
        exit 1
    fi
    sleep 0.1
done

echo "==> 安装到 ${APP}"
rm -rf "$APP"
cp -R "$BUILT" "$APP"

echo "==> 用固定身份重签名"
codesign --force --deep -s "$IDENTITY" "$APP"

echo "==> 校验签名"
codesign --verify --deep --strict "$APP"

echo
codesign -d -r- "$APP" 2>&1 | tail -1
echo "✓ 已安装 ${APP}"

open "$APP"
