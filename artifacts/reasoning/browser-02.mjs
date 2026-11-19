const task = await taskSpace(13);
const page = task.page('p1');
await page.cdp('Page.addScriptToEvaluateOnNewDocument', {source:'window.isTauri = true;'});
await page.reload();
console.log(await page.snapshot());
