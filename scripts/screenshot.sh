#!/bin/sh
# Render the App Stories screenshots from a temporary, hand-seeded history.
#
# The real ~/Library/Application Support/com.podnook.app data is not touched.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

export PATH="${HOME}/.local/bin:${PATH}"

DATA="$ROOT/.nsh/screenshot-data"
OUT="$ROOT/docs/images"
TEST="$ROOT/Screenshot.test.tsx"

command -v nsh >/dev/null 2>&1 || {
    echo "✗ nsh not found. Install the NSH CLI: https://nsh.dev/install.sh" >&2
    exit 1
}

python3 "$ROOT/scripts/seed-screenshots.py" "$DATA"
mkdir -p "$OUT"

cleanup() {
    rm -f "$TEST"
}
trap cleanup EXIT INT TERM

cat > "$TEST" <<EOF
import { open, test } from "nsh/test";

const size = { width: 1120, height: 630, platform: "macos" as const };

test("panel", async () => {
  const app = await open(".", size);
  await app.window("panel").screenshot("$OUT/panel.png");
});

test("editor", async () => {
  const app = await open(".", size);
  await app.wait(300);
  await app.keyboard.press("down");
  await app.keyboard.press("down");
  await app.keyboard.press("down");
  await app.wait(200);
  await app.keyboard.press("mod+e");
  await app.waitFor("text:Edit content");
  await app.wait(500);
  await app.window("editor").screenshot("$OUT/editor.png");
});

test("settings", async () => {
  const app = await open(".", size);
  await app.wait(400);
  await app.keyboard.press("mod+,");
  await app.waitFor("text:Ignored apps");
  await app.wait(400);
  await app.window("settings").screenshot("$OUT/settings.png");
});
EOF

PODNOOK_DATA_DIR="$DATA" nsh test "$TEST" --target macos

echo "✓ wrote $OUT/panel.png, editor.png, settings.png"
