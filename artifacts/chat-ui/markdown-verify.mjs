const task = await taskSpace(12);
const page = task.page("p1");
const fixture = "## Markdown 验收\n\n核对结论：**实现符合要求**。\n\n| 要求 | 结果 | 依据 |\n| --- | :---: | --- |\n| 初始显示 0 | ✅ | 第 62 行 `<output>` 内容为 0 |\n| 加一 +1 | ✅ | `update(count + 1)` |\n| 不得小于 0 | ✅ | `count = Math.max(0, nextCount)` |\n\n- [x] 表格正常显示\n- [ ] 等待确认\n\n1. 检查内容\n   - 保留嵌套列表\n2. 复制 Markdown 原文\n\n> 引用：支持 **加粗**、*斜体* 和 ~~删除线~~。\n\n[文档链接](https://example.com)\n\n```js\nconst longLine = 'abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789';\n```";
const original = await page.evaluate(text => {
  const key = "ai-gent.preview.messages.v1";
  const previous = localStorage.getItem(key);
  const records = JSON.parse(previous ?? "{}");
  records.personal = [{ id: "markdown-fixture", role: "assistant", text, created_at: null }];
  localStorage.setItem(key, JSON.stringify(records));
  return previous;
}, fixture);
try {
  await page.reload();
  await page.waitForSelector(".message-table table");
  await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 1000, deviceScaleFactor: 1, mobile: false });
  await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/markdown-desktop.png" });
  const results = [];
  for (const width of [1360, 900, 680]) {
    await page.cdp("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
    const result = await page.evaluate(() => ({ width: innerWidth, pageOverflow: document.documentElement.scrollWidth > innerWidth, rows: document.querySelectorAll(".message-table tbody tr").length, heading: document.querySelector(".message-body h2")?.textContent, tableWidth: document.querySelector(".message-table")?.clientWidth, tableScrollWidth: document.querySelector(".message-table")?.scrollWidth, codeScrollable: document.querySelector(".message-body pre").scrollWidth > document.querySelector(".message-body pre").clientWidth }));
    if (result.pageOverflow || result.rows !== 3 || result.heading !== "Markdown 验收") throw new Error(JSON.stringify(result));
    results.push(result);
  }
  await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/markdown-narrow.png" });
  console.log(results);
  const fs = await import("node:fs/promises");
  await fs.writeFile("D:/work/AI-gent/artifacts/chat-ui/markdown-results.json", JSON.stringify(results, null, 2));
} finally {
  await page.evaluate(value => {
    const key = "ai-gent.preview.messages.v1";
    if (value === null) localStorage.removeItem(key); else localStorage.setItem(key, value);
  }, original);
  await page.reload();
}
