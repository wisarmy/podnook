#!/bin/sh
# Render docs/images/panel.png from a temporary, hand-seeded history.
#
# The real ~/Library/Application Support/com.podnook.app data is not touched.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

export PATH="${HOME}/.local/bin:${PATH}"

DATA="$ROOT/.nsh/screenshot-data"
OUT="$ROOT/docs/images/panel.png"
TEST="$ROOT/Screenshot.test.tsx"

command -v nsh >/dev/null 2>&1 || {
    echo "✗ 找不到 nsh。先安装 NSH CLI：https://nsh.dev/install.sh" >&2
    exit 1
}

python3 "$ROOT/scripts/seed-screenshots.py" "$DATA"
mkdir -p "$(dirname "$OUT")"

cleanup() {
    rm -f "$TEST"
}
trap cleanup EXIT INT TERM

cat > "$TEST" <<EOF
import { open, test } from "nsh/test";

test("panel screenshot", async () => {
  const app = await open(".", { width: 980, height: 640, platform: "macos" });
  await app.window("panel").screenshot("$OUT");
});
EOF

PODNOOK_DATA_DIR="$DATA" nsh test "$TEST" --target macos

echo "✓ 已生成 $OUT"
