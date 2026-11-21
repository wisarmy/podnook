import { expect, open, test } from "nsh/test";

test("switches between history and snippets", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);

  await app.press("id:source-toggle");
  await app.wait(400);
  expect(app.texts()).not.toContain("All");

  await app.press("id:source-toggle");
  await app.wait(400);
  expect(app.texts()).toContain("All");
});

// 需要种子数据：历史里有条目时，操作面板能打开片段编辑器。
test("opens the snippet editor from the actions", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().includes("A pod for everything you keep.")) {
    console.log("no seeded clips; skipping");
    return;
  }

  await app.press("text:Actions >> nth=0");
  await app.wait(400);
  await app.press("text:Save as snippet…");
  await app.waitFor("text:Name", { timeout: 5000 });
  expect(app.texts()).toContain("Placeholders: {date}, {time}, {datetime}");
});

// 测试宿主的剪贴板在内存里：用 app.clipboard() 断言写入。
test("copies a snippet to the clipboard", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.press("id:source-toggle");
  await app.wait(400);
  if (!app.texts().includes("Signature")) {
    console.log("no seeded snippets; skipping");
    return;
  }
  await app.keyboard.press("mod+enter");
  await app.wait(500);
  expect(app.clipboard()).toBe("Best regards,\nWisarmy");
});

test("editing a snippet updates the list", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.press("id:source-toggle");
  await app.wait(400);
  if (!app.texts().includes("Signature")) {
    console.log("no seeded snippets; skipping");
    return;
  }
  await app.press("text:Signature");
  await app.press("text:Actions >> nth=0");
  await app.wait(400);
  await app.press("text:Edit content");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.type("id:snippet-name", " edited");
  await app.wait(200);
  await app.press("text:Save");
  await app.wait(700);
  expect(app.texts().some((text) => text.includes("edited"))).toBe(true);
});
