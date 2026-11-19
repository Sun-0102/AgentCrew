const task = await taskSpace("AI-gent 聊天界面验收");
const page = task.page("p1");
await page.goto("http://127.0.0.1:1420/");
await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 880, deviceScaleFactor: 1, mobile: false });
console.log({ taskSpaceId: task.spaceId, page: page.label });
console.log(await page.snapshot());
console.log(await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/chat-initial.png" }));
