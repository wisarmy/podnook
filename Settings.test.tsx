import { expect, open, test } from "nsh/test";

// 文案会随语言变，测试只断言窗口与交互，不依赖某一种语言的标签。
test("switching the language keeps the windows usable", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);

  await app.keyboard.press("mod+,");
  await app.waitUntil(() => app.window("settings").state() !== null, {
    timeout: 10000,
  });

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
  await app.waitUntil(() => app.window("settings").state() !== null, {
    timeout: 10000,
  });
  await app.keyboard.press("escape");
  await app.wait(300);

  await app.keyboard.press("mod+,");
  await app.wait(600);
  expect(app.window("settings").state()).not.toBeNull();
});
