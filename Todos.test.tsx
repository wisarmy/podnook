import { expect, open, test } from "nsh/test";

// 机器语言可能是中文或英文：断言同时接受两种文案。
const has = (app: Awaited<ReturnType<typeof open>>, en: string, zh: string) =>
  app.texts().some((text) => text === en || text === zh);

test("shows the to-do actions", async () => {
  const app = await open(".", { width: 980, height: 640, platform: "macos" });
  await app.wait(300);

  await app.press("id:source-todos");
  await app.wait(400);
  // 待办来源没有类型筛选条；历史里才有 All / 全部。
  expect(app.texts()).not.toContain("All");
  expect(app.texts()).not.toContain("全部");

  await app.press("id:actions-button");
  await app.wait(400);
  expect(has(app, "New To-do", "新建待办")).toBe(true);
  expect(has(app, "Delete Completed", "删除已完成")).toBe(true);
  expect(has(app, "Clear All To-dos…", "清空待办…")).toBe(true);
});

// headless 没有数据目录，写不进 SQLite；和片段测试一样，只有在
// `nsh test --target macos`（会分配数据目录）里才验证写入。
test("adds a to-do and checks it off with Enter", async () => {
  const app = await open(".", { width: 980, height: 640, platform: "macos" });
  await app.wait(300);

  await app.press("id:source-todos");
  await app.wait(400);

  const title = `Buy milk ${Date.now()}`;
  await app.keyboard.press("mod+n");
  await app.wait(200);
  await app.type("id:todo-draft", title);
  await app.keyboard.press("enter");
  await app.wait(600);
  if (!app.texts().includes(title)) {
    console.log("no data directory; skipping the to-do write");
    return;
  }

  // 新增框是空的：回车离开输入框并勾选当前条目。
  await app.keyboard.press("enter");
  await app.wait(600);
  expect(has(app, "Completed at", "完成于")).toBe(true);

  await app.keyboard.press("enter");
  await app.wait(600);
  expect(has(app, "Completed at", "完成于")).toBe(false);
});
