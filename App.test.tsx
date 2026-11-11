// M0：骨架的冒烟测试。真实交互在 M2 补上。
// headless 默认视口是 390×844（手机尺寸），这里显式用 Podnook 的窗口尺寸，
// 否则窄视口会把右侧栏的文字挤到 0 宽、被 texts() 当成不可见。
import { expect, open, test } from "nsh/test";

const VIEWPORT = { width: 980, height: 640 };

test("shows the clipboard history shell", async () => {
  const app = await open(".", VIEWPORT);
  expect(app.texts()).toContain("全部类型");
  expect(app.texts()).toContain("剪贴板历史");
  expect(app.texts()).toContain("信息");
  expect(app.texts()).toContain("来源");
  expect(app.texts()).toContain("截屏 2026-11-11 21.31.01");
});
