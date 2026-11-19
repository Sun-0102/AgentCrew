// Read public model metadata from the installed Harness, without reading credentials.
const { pathToFileURL } = require('node:url');
const { join } = require('node:path');
(async () => {
  const root = process.argv[2];
  const runtime = await import(pathToFileURL(join(root, 'resources/app.asar/dsh/node_modules/@deepseek-ai/dsh-llm-deepseek/lib/index.js')));
  const models = runtime.resolveAdapterOptions({}).models;
  console.log(JSON.stringify(models.map((model, index) => ({ model: model.id, displayName: model.name || model.id, isDefault: index === 0 }))));
})().catch(() => { console.error('Harness model catalog unavailable'); process.exitCode = 1; });
