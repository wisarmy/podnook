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
  if (!app.texts().includes("Standup note")) {
    console.log("no seeded snippets; skipping");
    return;
  }
  await app.press("text:Standup note");
  await app.wait(200);
  await app.keyboard.press("mod+enter");
  await app.wait(500);
  expect(app.clipboard()).toContain("shipped snippets");
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
  await app.press("text:Edit Snippet");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.type("id:snippet-name", " edited");
  await app.wait(200);
  await app.press("text:Save");
  await app.wait(700);
  expect(app.texts().some((text) => text.includes("edited"))).toBe(true);
});

test("create a snippet with ⌘N", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);
  await app.press("id:source-toggle");
  await app.wait(300);
  await app.keyboard.press("mod+n");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.press("text:Cancel");
  await app.wait(400);
  expect(app.texts().some((text) => text.includes("Snippets"))).toBe(true);
});

test("duplicate a snippet with ⌘D", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);
  await app.press("id:source-toggle");
  await app.wait(400);
  if (!app.texts().includes("Standup note")) {
    console.log("no seeded snippets; skipping");
    return;
  }
  await app.press("text:Standup note");
  await app.wait(200);
  await app.keyboard.press("mod+d");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.press("text:Save");
  await app.wait(700);
  expect(app.texts().some((text) => text.includes("copy"))).toBe(true);
});

test("⌘E edits the selected snippet", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);
  await app.press("id:source-toggle");
  await app.wait(300);
  await app.keyboard.press("mod+n");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.type("id:snippet-name", "Hello");
  await app.type("id:snippet-body", "World");
  await app.press("text:Save");
  await app.wait(600);

  await app.keyboard.press("mod+e");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
});

test("typing { opens the placeholder menu", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);
  await app.press("id:source-toggle");
  await app.wait(300);
  await app.keyboard.press("mod+n");
  await app.waitFor("id:snippet-name", { timeout: 5000 });
  await app.type("id:snippet-name", "Placeholders");
  await app.type("id:snippet-body", "today {");
  await app.wait(400);
  expect(app.texts()).toContain("Custom Format");

  await app.type("id:snippet-body", "da");
  await app.wait(400);
  expect(app.texts()).not.toContain("Time");

  await app.keyboard.press("enter");
  await app.wait(300);
  expect(app.texts()).not.toContain("Date & Time");
});

test("shift+tab cycles the sources", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);
  expect(app.texts()).toContain("Clipboard history");

  await app.keyboard.press("shift+tab");
  await app.wait(400);
  expect(app.texts()).toContain("Snippets");

  await app.keyboard.press("shift+tab");
  await app.wait(400);
  expect(app.texts()).toContain("Clipboard history");
});
