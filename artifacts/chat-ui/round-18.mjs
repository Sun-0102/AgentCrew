const task = await taskSpace(6);
const page = task.page("p1");
await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 880, deviceScaleFactor: 1, mobile: false });
const results = [];
for (let i = 0; i < 3; i++) {
  await page.reload();
  await page.waitForSelector(".conversation-item:has-text('产品协作群')");
  await page.click(".conversation-item:has-text('产品协作群')");
  await page.waitForFunction(() => document.querySelector("#chat-input")?.value.includes("保留这段草稿"));
  await page.click("loc=role:button[name='邀请']");
  await page.waitForSelector("dialog[open]");
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => !document.querySelector("dialog"));
  if (!await page.evaluate(() => document.activeElement?.textContent === "邀请")) throw new Error("Focus was not restored");
  results.push({ run: i + 1, groupAndDraftRestored: true, escapeAndFocus: true });
}
await page.click("loc=role:button[name='查看聊天成员']");
await page.waitForFunction(() => document.getAnimations().every(a => a.effect?.getTiming().iterations === Infinity || a.playState !== "running"));
await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/group-desktop.png" });
console.log(results);
const fs = await import("node:fs/promises");
await fs.writeFile("D:/work/AI-gent/artifacts/chat-ui/final-check.json", JSON.stringify(results, null, 2));
await task.finish({ keep: [] });
