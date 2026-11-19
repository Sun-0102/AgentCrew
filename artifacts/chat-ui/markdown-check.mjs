const task = await taskSpace("Markdown rendering check");
const page = task.page("p1");
console.log({ spaceId: task.spaceId });
await page.goto("http://127.0.0.1:1420");
console.log(await page.snapshot());
