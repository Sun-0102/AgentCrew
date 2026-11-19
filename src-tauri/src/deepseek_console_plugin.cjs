const { join } = require("node:path");

exports.name = "ai-gent-windows-console";
exports.inject = ["sandbox"];
exports.apply = ctx => {
  if (process.platform !== "win32") return;
  const sandbox = ctx.get("sandbox");
  // Pinned Harness 0.2.1-rc.2 exposes this runner seam. Keep its policy and argv intact.
  if (!sandbox?.internals || typeof sandbox.windowsAclRunnerInvocation !== "function") {
    throw new Error("Harness Windows sandbox runner is incompatible with the console adapter");
  }
  const previous = sandbox.internals.windowsAclRunnerArgs;
  const argv = sandbox.windowsAclRunnerInvocation();
  if (argv[0] !== process.execPath) throw new Error("Unexpected Harness Windows sandbox runner");
  ctx.effect(() => {
    sandbox.internals.windowsAclRunnerArgs = [argv[0], "--require", join(__dirname, "console.cjs"), ...argv.slice(1)];
    return () => { sandbox.internals.windowsAclRunnerArgs = previous; };
  });
};
