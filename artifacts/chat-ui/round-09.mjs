const task = await taskSpace(6);
const page = task.page("p1");
await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 880, deviceScaleFactor: 1, mobile: false });
await page.waitForSelector(".message-row.incoming .message-body");
await page.click("loc=role:button[name='回复Codex 的消息']");
await page.fill("#chat-input", "我们一起整理一下接下来的工作。");
await page.waitForFunction(() => document.getAnimations().every(a => a.effect?.getTiming().iterations === Infinity || a.playState !== "running"));
console.log(await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/private-desktop.png" }));
console.log(await page.snapshot());
