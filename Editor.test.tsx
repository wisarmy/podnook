import { expect, open, test } from "nsh/test";

// 需要种子数据：编辑默认选中的文本条目。
test("clip editing stays in the panel", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().some((text) => text.includes("A pod for everything you keep."))) {
    console.log("no seeded clips; skipping");
    return;
  }

  await app.press("text:Actions >> nth=0");
  await app.wait(400);
  await app.press("text:Edit content");
  await app.waitFor("id:clip-body", { timeout: 5000 });
  await app.type("id:clip-body", "X");
  await app.press("text:Save");
  await app.wait(600);
  expect(app.texts()).not.toContain("Snippet text");
  expect(app.texts().some((text) => text.includes("A pod for everything you keep.X"))).toBe(true);
});

test("escape leaves the clip editor", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().some((text) => text.includes("A pod for everything you keep."))) {
    console.log("no seeded clips; skipping");
    return;
  }
  await app.press("text:Actions >> nth=0");
  await app.wait(400);
  await app.press("text:Edit content");
  await app.waitFor("id:clip-body", { timeout: 5000 });
  await app.keyboard.press("escape");
  await app.wait(400);
  expect(app.texts()).toContain("Actions");
});

test("enter inserts a newline in the clip editor", async () => {
  const app = await open(".", { width: 1120, height: 630, platform: "macos" });
  await app.wait(400);
  if (!app.texts().some((text) => text.includes("fn main()"))) {
    console.log("no seeded clips; skipping");
    return;
  }
  await app.press("text:fn main() { let store = Store::open(data_dir)?; println!(\"local-first\"); }");
  await app.wait(200);
  await app.press("text:Actions >> nth=0");
  await app.wait(400);
  await app.press("text:Edit content");
  await app.waitFor("id:clip-body", { timeout: 5000 });
  await app.type("id:clip-body", "\nsecond line");
  await app.press("text:Save");
  await app.wait(600);
  expect(app.texts().some((text) => text.includes("second line"))).toBe(true);
});
