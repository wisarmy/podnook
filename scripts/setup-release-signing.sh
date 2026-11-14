#!/bin/sh
# Create the fixed self-signed certificate that GitHub Actions uses to sign
# release builds, and print the values to store as repository secrets.
#
# The certificate is created locally with openssl; it is never imported into
# your login keychain. Keep the .p12/.pem somewhere safe: the same identity
# must sign every release, otherwise macOS resets Accessibility permissions.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

IDENTITY="${IDENTITY:-Podnook Release Signing}"
OUT="${1:-$ROOT/release-signing.p12}"
PEM="${OUT%.p12}.pem"

if [ -e "$OUT" ] || [ -e "$PEM" ]; then
    echo "✗ ${OUT} 或 ${PEM} 已存在；先备份并移走，避免覆盖旧证书。" >&2
    exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

PASS=$(openssl rand -hex 24)

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
    -out "$OUT" -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -passout "pass:${PASS}" -name "${IDENTITY}" >/dev/null 2>&1

cp "$TMP/cert.pem" "$PEM"

KEYCHAIN_SERVICE="com.podnook.release-signing"
if security add-generic-password -a "$IDENTITY" -s "$KEYCHAIN_SERVICE" -w "$PASS" -U >/dev/null 2>&1; then
    echo "✓ 密码已存入登录钥匙串（服务名：${KEYCHAIN_SERVICE}）"
else
    echo "⚠ 无法写入登录钥匙串；请把密码保存到密码管理器。" >&2
fi

echo "✓ 已生成发布签名证书"
echo
echo "身份（MACOS_SIGN_IDENTITY）："
echo "  ${IDENTITY}"
echo
echo "密码（MACOS_CERT_PASSWORD）："
echo "  ${PASS}"
echo
echo "证书（MACOS_CERT_P12，整段 base64）："
base64 -i "$OUT"
echo
echo "证书 PEM（MACOS_CERT_PEM，整段复制，包含 BEGIN/END 行）："
cat "$PEM"
echo
echo "把上面四个值加到 GitHub → Settings → Secrets and variables → Actions："
echo "  MACOS_CERT_P12"
echo "  MACOS_CERT_PASSWORD"
echo "  MACOS_CERT_PEM"
echo "  MACOS_SIGN_IDENTITY"
echo
echo "然后把 ${OUT} 和 ${PEM} 移到安全的地方（不要提交、不要丢）。"
echo "证书身份一旦更换，已安装用户的辅助功能授权会重置一次。"
