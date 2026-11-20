const { pathToFileURL } = require("node:url");
const { join, dirname } = require("node:path");

exports.name = "ai-gent-skills";
exports.apply = async ctx => {
  // Adapter seam for the pinned Harness SDK, which has no public skill RPC.
  const modules = join(dirname(process.execPath), "resources/app.asar/dsh/node_modules/@deepseek-ai");
  const { HarnessSdkJsonRpcServer } = await import(pathToFileURL(join(modules, "dsh-sdk-jsonrpc-server/lib/index.js")));
  const { isUserInvocable, renderSkillContent } = await import(pathToFileURL(join(modules, "dsh-skill/lib/index.js")));
  const prototype = HarnessSdkJsonRpcServer.prototype;
  const original = prototype.handleRequest;
  if (typeof original !== "function") throw new Error("Harness skill adapter is incompatible");
  prototype.handleRequest = async function(method, params) {
    if (method !== "ai-gent/skills") return original.call(this, method, params);
    if (!Array.isArray(params?.names) || params.names.length > 8) throw new Error("Invalid skill selection");
    if (!this.initialized) throw new Error("Harness SDK is not initialized");
    const registry = this.ctx.get("skills");
    if (!registry) throw new Error("Harness skill registry is unavailable");
    // Match the native cold catalog: resolve the default preset without creating an Agent.
    const lease = await this.ctx.get("agentPresets")?.acquireScope(undefined);
    try {
      const options = { cwd: this.cwd, scope: lease?.key };
      const catalog = (await registry.list(options)).filter(isUserInvocable);
      const skills = [];
      for (const skill of catalog) {
        let content;
        if (params.names.includes(skill.name)) {
          const loaded = await registry.get(skill.name, options);
          if (!loaded || !isUserInvocable(loaded)) throw new Error(`Skill is unavailable: ${skill.name}`);
          content = renderSkillContent(loaded);
        }
        skills.push({ name: skill.name, description: skill.description, path: skill.path ?? "", native: true, content });
      }
      return { skills };
    } finally {
      await lease?.[Symbol.asyncDispose]?.();
    }
  };
  ctx.effect(() => () => { prototype.handleRequest = original; });
};
