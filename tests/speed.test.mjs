import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
after(() => server.close());
const { speedChoices, selectedSpeed } = await server.ssrLoadModule("/src/features/chats/speed.ts");
const { selectedEffort } = await server.ssrLoadModule("/src/features/chats/reasoning.ts");
const model = { serviceTiers: [{ id: "priority", name: "Fast", description: "More usage" }], defaultReasoningEffort: "high", supportedReasoningEfforts: [{ reasoningEffort: "high" }] };

test("speed and reasoning are independent and only advertised speeds are selectable", () => {
  assert.equal(selectedSpeed(model, "priority"), "priority");
  assert.equal(selectedEffort(model, "high"), "high");
  assert.equal(selectedSpeed(model, "default"), "default");
  assert.equal(selectedEffort(model, "high"), "high");
  assert.equal(selectedSpeed(model, "ultrafast"), "default");
  assert.deepEqual(speedChoices(undefined).map(t => t.id), ["default"]);
  assert.equal(selectedSpeed({ serviceTiers: [] }, "priority"), "default");
  assert.equal(speedChoices({ serviceTiers: [{ id: "future-speed", name: "Future", description: "" }] })[1].name, "Future");
  assert.equal(speedChoices({ serviceTiers: [{ id: "default", name: "Standard", description: "" }] }).length, 1);
});
