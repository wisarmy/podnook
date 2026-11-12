#!/bin/sh
# 创建（或修复）Podnook 的本地自签名代码签名证书。
#
# 为什么要固定签名：macOS 把「辅助功能」权限绑定到 App 的代码签名。
# ad-hoc 签名的指定要求是二进制的 cdhash，每次重新打包都会变，权限随之失效。
# 用固定证书签名后，指定要求只包含 bundle id 和证书指纹，重建不会重置权限。
#
# 私钥只保存在本机登录钥匙串，不进入仓库。
set -eu

IDENTITY="Podnook Local Signing"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

valid_identity() {
    security find-identity -v -p codesigning 2>/dev/null | grep -F "\"${IDENTITY}\"" >/dev/null 2>&1
}

if valid_identity; then
    echo "✓ 已存在签名证书：${IDENTITY}"
    security find-identity -v -p codesigning | grep -F "\"${IDENTITY}\""
    exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

# 证书已存在但不受信任：只补信任设置，不重复生成密钥。
if security find-certificate -c "$IDENTITY" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "发现已存在的证书，补加代码签名信任…"
    security find-certificate -c "$IDENTITY" -p "$KEYCHAIN" > "$TMP/cert.pem"
    security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$TMP/cert.pem"
    if valid_identity; then
        echo "✓ 修复完成"
        security find-identity -v -p codesigning | grep -F "\"${IDENTITY}\""
        exit 0
    fi
    echo "✗ 证书存在但仍不是有效的代码签名身份，请到「钥匙串访问」里检查信任设置。" >&2
    exit 1
fi

echo "创建本地签名证书：${IDENTITY}"
PASS=$(openssl rand -hex 16)

openssl req -x509 -newkey rsa:2048 \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" \
    -days 3650 -nodes -subj "/CN=${IDENTITY}" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning" \
    -addext "basicConstraints=critical,CA:false" >/dev/null 2>&1

LEGACY=""
if openssl pkcs12 -help 2>&1 | grep -q -- "-legacy"; then
    LEGACY="-legacy"
fi

# shellcheck disable=SC2086
openssl pkcs12 -export $LEGACY \
    -out "$TMP/id.p12" -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -passout "pass:${PASS}" -name "${IDENTITY}" >/dev/null 2>&1

security import "$TMP/id.p12" -k "$KEYCHAIN" -P "$PASS" \
    -T /usr/bin/codesign -T /usr/bin/security >/dev/null

if ! security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$TMP/cert.pem"; then
    echo "✗ 添加证书信任失败。" >&2
    echo "  打开「钥匙串访问」→ 找到 ${IDENTITY} → 显示简介 → 信任 → 代码签名：始终信任。" >&2
    exit 1
fi

if ! valid_identity; then
    echo "✗ 证书已导入，但未被识别为有效的代码签名身份。" >&2
    exit 1
fi

echo "✓ 完成。后续打包用这个身份签名，辅助功能权限不再随构建失效。"
security find-identity -v -p codesigning | grep -F "\"${IDENTITY}\""
