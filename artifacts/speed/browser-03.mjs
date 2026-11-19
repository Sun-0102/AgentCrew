import assert from "node:assert/strict";
const task = await taskSpace(14);
const page = task.page("p1");
await page.cdp("Page.addScriptToEvaluateOnNewDocument", { source: `
window.isTauri = true;
window.__TEST_CALLS__ = [];
window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
  window.__TEST_CALLS__.push({command,args});
  const model = (id, efforts, def) => ({model:id,displayName:id,isDefault:true,supportedReasoningEfforts:efforts.map(reasoningEffort => ({reasoningEffort,description:reasoningEffort})),defaultReasoningEffort:def,serviceTiers:id === "Codex-test" ? [{id:"priority",name:"Fast",description:"Faster, more usage"}] : []});
  if (command === 'codex_probe' || command === 'deepseek_probe') return {version:'UI test fixture',executable:'fixture',auth:'fixture',requires_auth:false,models:command === 'codex_probe' ? [model('Codex-test',['low','medium','high','xhigh'],'medium'), {...model('Codex-light-test',['low','high'],'low'),isDefault:false}] : [model('DeepSeek-test',['off','low','high','max'],'high')],sidebar_warning:null};
  if (command === 'app_info') return {version:'test',database_path:'fixture'};
  if (command === 'project_list') return [];
  if (command === 'group_list') return [{id:'reasoning-ui-group',name:'思考模式测试群',members:['codex','deepseek'],project_id:null,created_at:'2026-11-19',preview:null,updated_at:null,status:null}];
  if (command.endsWith('_snapshot')) return {session:null,messages:[],approvals:[]};
  return {};
}};
` });
await page.reload();

await page.waitForFunction(()=>document.querySelector('select[aria-label="选择速度"]')?.value==='priority');
assert.equal(await page.evaluate(()=>document.querySelector('select[aria-label="选择思考模式"]').value),'high');
const saved=await page.evaluate(()=>({speed:JSON.parse(localStorage.getItem('ai-gent.speed.v1')),effort:JSON.parse(localStorage.getItem('ai-gent.reasoning.v1'))}));
assert.equal(saved.speed['Codex-test'],'priority'); assert.equal(saved.effort['codex:Codex-test'],'high');
await page.cdp('Emulation.setDeviceMetricsOverride',{width:390,height:720,deviceScaleFactor:1,mobile:false});
const layout=await page.evaluate(()=>{const a=document.querySelector('select[aria-label="选择速度"]').getBoundingClientRect();return {width:innerWidth,scroll:document.documentElement.scrollWidth,right:a.right,bottom:a.bottom,height:innerHeight};});
assert.equal(layout.width,layout.scroll); assert.ok(layout.right<=layout.width&&layout.bottom<=layout.height);
await page.screenshot({path:'D:/work/AI-gent/artifacts/speed/mobile-fixture.png'});
await page.evaluate(()=>{const original=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=(cmd,args)=>cmd==='codex_snapshot'?Promise.resolve({session:{thread_id:'fixture',status:'running',model:'Codex-test',error:null},messages:[],approvals:[]}):original(cmd,args);});
await page.waitForFunction(()=>document.querySelector('select[aria-label="选择速度"]')?.disabled);
assert.equal(await page.evaluate(()=>document.querySelector('select[aria-label="选择思考模式"]').disabled),true);
console.log({reloadRestored:{speed:'priority',effort:'high'},runningControlsDisabled:true,layout});
