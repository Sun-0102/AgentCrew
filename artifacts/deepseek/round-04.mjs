const task=await taskSpace(8);const page=task.page('p1');
await page.click('button[aria-label="关闭弹窗"]');
await page.cdp('Emulation.setDeviceMetricsOverride',{width:900,height:640,deviceScaleFactor:1,mobile:false});
const layout=await page.evaluate(()=>({width:innerWidth,scroll:document.documentElement.scrollWidth,input:document.querySelector('textarea').getBoundingClientRect().toJSON()}));
if(layout.scroll>layout.width||layout.input.bottom>640)throw Error('DeepSeek composer overflow');
console.log(layout);console.log({screenshot:await page.screenshot({path:'D:/work/AI-gent/artifacts/deepseek/private-900.png'})});
console.log(await page.snapshot());
