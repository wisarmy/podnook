// M2：真实数据界面的冒烟测试。
// headless 没有数据目录，所以看到的是空态；有数据的交互在 `nsh test --target macos`
// 配合 `PODNOOK_DATA_DIR` 验证。
import { expect, open, test } from "nsh/test";

test("shows the clipboard history shell", async () => {
  const app = await open(".", { width: 980, height: 640, platform: "macos" });
  expect(app.texts()).toContain("全部");
  expect(app.texts()).toContain("剪贴板历史 · 0 条");
  expect(app.texts()).toContain("复制点什么，就会出现在这里");
});
