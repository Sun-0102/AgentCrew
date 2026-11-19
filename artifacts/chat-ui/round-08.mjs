const task = await taskSpace(6);
const page = task.page("p1");
// The greeting is transcribed from the user's attached screenshot of their existing real conversation.
await page.evaluate(() => {
  const key = "ai-gent.preview.messages.v1";
  const messages = JSON.parse(localStorage.getItem(key) ?? "{}");
  messages.personal = [
    { id: "existing-greeting-user", role: "user", text: "你好", created_at: null },
    { id: "existing-greeting-codex", role: "assistant", text: "你好！有什么需要我帮你处理的？", created_at: null },
  ];
  localStorage.setItem(key, JSON.stringify(messages));
});
await page.reload();
await page.cdp("Emulation.setDeviceMetricsOverride", { width: 1360, height: 880, deviceScaleFactor: 1, mobile: false });
await page.waitForSelector("text=你好！有什么需要我帮你处理的？");
await page.click("loc=role:button[name='回复Codex 的消息']");
await page.fill("loc=css:#chat-input", "我们一起整理一下接下来的工作。");
await page.waitForFunction(() => document.getAnimations().every(a => a.effect?.getTiming().iterations === Infinity || a.playState !== "running"));
console.log(await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/private-desktop.png" }));
console.log({ quoteVisible: await page.evaluate(() => !!document.querySelector(".reply-preview")) });
