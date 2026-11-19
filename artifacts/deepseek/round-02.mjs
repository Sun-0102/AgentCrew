const task=await taskSpace(8);const page=task.page('p1');
await page.click('@15');
await page.waitForSelector('button:has-text("连接 DeepSeek")');
console.log(await page.snapshot());
console.log({screenshot:await page.screenshot({path:'D:/work/AI-gent/artifacts/deepseek/private-preview.png'})});
