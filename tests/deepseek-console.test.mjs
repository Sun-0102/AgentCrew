import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const bootstrap = await readFile(new URL("../src-tauri/src/deepseek_console.cjs", import.meta.url), "utf8");
const plugin = await readFile(new URL("../src-tauri/src/deepseek_console_plugin.cjs", import.meta.url), "utf8");

function attach({ existing = false, hideFails = false } = {}) {
  const ids = [0xfffffff6, 0xfffffff5, 0xfffffff4];
  const handles = new Map(ids.map((id, index) => [id, `pipe-${index}`]));
  let consoleAttached = existing;
  let allocations = 0;
  let hides = 0;
  const functions = {
    GetConsoleWindow: () => consoleAttached ? "console" : null,
    GetStdHandle: id => handles.get(id),
    SetStdHandle: (id, handle) => { handles.set(id, handle); return 1; },
    AllocConsole: () => { allocations++; consoleAttached = true; ids.forEach(id => handles.set(id, "console-handle")); return 1; },
    ShowWindow: () => { hides++; return 1; },
    IsWindowVisible: () => hideFails ? 1 : 0,
    GetLastError: () => 0,
  };
  const nativeRequire = () => ({ load: () => ({ func: signature => functions[signature.match(/(\w+)\(/)[1]] }) });
  nativeRequire.resolve = () => "runtime-process.js";
  let error;
  try {
    runInNewContext(bootstrap, {
      process: { platform: "win32", argv: ["host.exe", "runner.js"] },
      require: () => ({ createRequire: () => nativeRequire }),
    });
  } catch (cause) { error = cause; }
  return { allocations, hides, handles: ids.map(id => handles.get(id)), error };
}

test("a new hidden console preserves all JSON-RPC pipe handles", () => {
  assert.deepEqual(attach(), { allocations: 1, hides: 1, handles: ["pipe-0", "pipe-1", "pipe-2"], error: undefined });
});

test("an inherited console is left intact and failures still restore pipes", () => {
  assert.equal(attach({ existing: true }).allocations, 0);
  assert.equal(attach({ existing: true }).hides, 0);
  const failed = attach({ hideFails: true });
  assert.match(failed.error.message, /could not be hidden/);
  assert.deepEqual(failed.handles, ["pipe-0", "pipe-1", "pipe-2"]);
});

test("the adapter adds only a preload and restores the pinned runner on disposal", () => {
  const original = ["host.exe", "runner.js"];
  const sandbox = { internals: {}, windowsAclRunnerInvocation: () => original };
  let dispose;
  const exports = {};
  runInNewContext(plugin, { exports, __dirname: "runtime", process: { platform: "win32", execPath: "host.exe" }, require: () => ({ join: (...parts) => parts.join("/") }) });
  exports.apply({ get: () => sandbox, effect: setup => { dispose = setup(); } });
  assert.deepEqual(Array.from(sandbox.internals.windowsAclRunnerArgs), ["host.exe", "--require", "runtime/console.cjs", "runner.js"]);
  assert.deepEqual(original, ["host.exe", "runner.js"]);
  dispose();
  assert.equal(sandbox.internals.windowsAclRunnerArgs, undefined);
  assert.throws(() => exports.apply({ get: () => ({}) }), /incompatible/);
});
