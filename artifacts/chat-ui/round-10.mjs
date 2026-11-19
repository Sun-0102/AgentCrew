const task = await taskSpace(6);
const page = task.page("p1");
await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 880, deviceScaleFactor: 1, mobile: false });
await page.click("loc=role:button[name=/产品协作群/]");
await page.click("loc=role:button[name='查看聊天成员']");
await page.waitForFunction(() => document.getAnimations().every(a => a.effect?.getTiming().iterations === Infinity || a.playState !== "running"));
console.log(await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/group-desktop.png" }));
console.log(await page.snapshot());
