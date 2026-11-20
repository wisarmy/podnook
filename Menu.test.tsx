import { expect, open, test } from "nsh/test";

// 需要种子数据（PODNOOK_DATA_DIR + scripts/seed-screenshots.py）才有条目可点；
// 默认 headless 跑的是空库，直接跳过。
const CLIP = "A pod for everything you keep. >> nth=0";

test("right click closes the actions, the next one opens them", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().includes("A pod for everything you keep.")) {
    console.log("no seeded clips; skipping");
    return;
  }

  // 打开操作面板后，右键面板之外只关闭面板。
  await app.press("text:Actions >> nth=0");
  await app.wait(500);
  expect(app.texts()).toContain("Clear all…");

  await app.rightClick(`text:${CLIP}`);
  await app.wait(500);
  expect(app.texts()).not.toContain("Clear all…");

  // 再右键一次才打开，并选中该条。
  await app.rightClick(`text:${CLIP}`);
  await app.wait(500);
  expect(app.texts()).toContain("Clear all…");
});

test("the footer button toggles the actions", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().includes("A pod for everything you keep.")) {
    console.log("no seeded clips; skipping");
    return;
  }

  await app.press("text:Actions >> nth=0");
  await app.wait(500);
  expect(app.texts()).toContain("Clear all…");

  await app.press("text:Actions >> nth=0");
  await app.wait(600);
  expect(app.texts()).not.toContain("Clear all…");
});
