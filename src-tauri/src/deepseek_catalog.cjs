// Read public model metadata from the installed Harness, without reading credentials.
const { pathToFileURL } = require('node:url');
const { join } = require('node:path');
(async () => {
  const root = process.argv[2];
  const runtime = await import(pathToFileURL(join(root, 'app.asar/dsh/node_modules/@deepseek-ai/dsh-llm-deepseek/lib/index.js')));
  const options = runtime.resolveAdapterOptions({});
  const models = options.models;
  const adapter = new runtime.DeepSeekAdapter({ options: () => options });
  console.log(JSON.stringify(await Promise.all(models.map(async (model, index) => {
    const info = await adapter.resolveModel("deepseek-official", model.id);
    return { model: model.id, displayName: model.name || model.id, isDefault: index === 0,
      supportedReasoningEfforts: info.reasoning.efforts.map(e => ({ reasoningEffort: e.id, description: e.description })),
      defaultReasoningEffort: info.reasoning.defaultEffort };
  }))));
})().catch(() => { console.error('Harness model catalog unavailable'); process.exitCode = 1; });
