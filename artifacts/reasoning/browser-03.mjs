const task = await taskSpace(13);
const page = task.page("p1");
await page.cdp("Page.addScriptToEvaluateOnNewDocument", { source: `
window.isTauri = true;
window.__TEST_CALLS__ = [];
window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
  window.__TEST_CALLS__.push({command,args});
  const model = (id, efforts, def) => ({model:id,displayName:id,isDefault:true,supportedReasoningEfforts:efforts.map(reasoningEffort => ({reasoningEffort,description:reasoningEffort})),defaultReasoningEffort:def});
  if (command === 'codex_probe' || command === 'deepseek_probe') return {version:'UI test fixture',executable:'fixture',auth:'fixture',requires_auth:false,models:command === 'codex_probe' ? [model('Codex-test',['low','medium','high','xhigh'],'medium'), {...model('Codex-light-test',['low','high'],'low'),isDefault:false}] : [model('DeepSeek-test',['off','low','high','max'],'high')],sidebar_warning:null};
  if (command === 'app_info') return {version:'test',database_path:'fixture'};
  if (command === 'project_list') return [];
  if (command === 'group_list') return [{id:'reasoning-ui-group',name:'思考模式测试群',members:['codex','deepseek'],project_id:null,created_at:'2026-11-19',preview:null,updated_at:null,status:null}];
  if (command.endsWith('_snapshot')) return {session:null,messages:[],approvals:[]};
  return {};
}};
` });
await page.reload();
console.log({spaceId:task.spaceId});
console.log(await page.snapshot());
