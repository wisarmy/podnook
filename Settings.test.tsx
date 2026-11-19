import { expect, open, test } from "nsh/test";

// Headless 里 `set_locale` 不改变引擎的界面语言（只有真实窗口才会），所以这里
// 断言的是「切换语言之后窗口还在、面板还能开」；语言是否真的变了由
// `nsh test Settings.test.tsx --target macos` 配 PODNOOK_DATA_DIR 覆盖。
test("switching the language keeps the windows usable", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);

  await app.keyboard.press("mod+,");
  await app.waitFor("text:Ignored apps");
  await app.press("text:中文");
  await app.wait(400);

  expect(app.window("settings").state()).not.toBeNull();

  await app.keyboard.press("mod+k");
  await app.wait(600);
  const texts = app.texts();
  expect(texts.includes("粘贴") || texts.includes("Paste")).toBe(true);
});

test("escape hides settings and the shortcut brings it back", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);

  await app.keyboard.press("mod+,");
  await app.waitFor("text:Ignored apps");
  await app.keyboard.press("escape");
  await app.wait(300);

  await app.keyboard.press("mod+,");
  await app.wait(400);
  expect(app.window("settings").state()).not.toBeNull();
});
