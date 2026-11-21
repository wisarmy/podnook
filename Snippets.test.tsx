import { expect, open, test } from "nsh/test";

test("switches between history and snippets", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(300);

  await app.press("text:Snippets");
  await app.wait(400);
  expect(app.texts()).not.toContain("All");

  await app.press("text:History");
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
