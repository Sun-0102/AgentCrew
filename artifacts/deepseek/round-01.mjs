const task=await taskSpace('AI-gent DeepSeek private chat verification');
const page=task.page('p1');
await page.goto('http://127.0.0.1:1420');
console.log({spaceId:task.spaceId});
console.log(await page.snapshot());
