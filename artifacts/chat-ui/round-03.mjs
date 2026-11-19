const task = await taskSpace(6);
const page = task.page("p1");
await page.fill("@32", "产品协作群");
await page.click("@34");
await page.click("loc=role:button[name='创建群聊']");
await page.waitForSelector("loc=role:button[name='邀请']", { state: "visible" });
console.log(await page.snapshot());
console.log(await page.screenshot({ path: "D:/work/AI-gent/artifacts/chat-ui/group-created.png" }));
