import { after, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";

const server = await createServer({
  configFile: false,
  server: { middlewareMode: true, hmr: false, watch: { ignored: ["**/src-tauri/**"] } },
  optimizeDeps: { noDiscovery: true },
  appType: "custom",
});
after(() => server.close());

const { applyStreamDeltas } = await server.ssrLoadModule("/src/features/chats/events.ts");

test("ignores deltas for other conversations without cloning snapshot", () => {
  const snap = {
    conversation_id: "conv-1",
    session: { thread_id: "t1", status: "running", model: "m", error: null },
    messages: [],
    approvals: [],
  };
  const next = applyStreamDeltas(snap, [
    { conversation_id: "conv-2", item_id: "m1", role: "assistant", mode: "append", text: "hi" },
  ]);
  assert.equal(next, snap);
});

test("appends streaming chunks and replaces final item in place", () => {
  const snap = {
    conversation_id: "conv-1",
    session: { thread_id: "t1", status: "running", model: "m", error: null },
    messages: [{ id: "u1", role: "user", text: "hello", created_at: "2026-11-24T00:00:00Z" }],
    approvals: [],
  };
  const afterAppend = applyStreamDeltas(snap, [
    { conversation_id: "conv-1", item_id: "r1", role: "thinking", agent: "codex", mode: "append", text: "先思考" },
    { conversation_id: "conv-1", item_id: "m1", role: "assistant", agent: "codex", mode: "append", text: "你" },
    { conversation_id: "conv-1", item_id: "m1", role: "assistant", agent: "codex", mode: "append", text: "好" },
  ]);
  assert.equal(afterAppend.messages.length, 3);
  assert.equal(afterAppend.messages[1].role, "thinking");
  assert.equal(afterAppend.messages[1].text, "先思考");
  assert.equal(afterAppend.messages[2].text, "你好");

  const afterReplace = applyStreamDeltas(afterAppend, [
    { conversation_id: "conv-1", item_id: "m1", role: "assistant", agent: "codex", mode: "replace", text: "你好！" },
  ]);
  assert.equal(afterReplace.messages.length, 3);
  assert.equal(afterReplace.messages[2].text, "你好！");
});
