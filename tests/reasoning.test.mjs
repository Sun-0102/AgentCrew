import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true }, appType: "custom" });
after(() => server.close());
const { selectedEffort } = await server.ssrLoadModule("/src/features/chats/reasoning.ts");
const model = { defaultReasoningEffort: "high", supportedReasoningEfforts: ["off", "low", "high", "max"].map(reasoningEffort => ({ reasoningEffort })) };

test("keeps saved choices including off and falls back only to a supported effort", () => {
  assert.equal(selectedEffort(model, "off"), "off");
  assert.equal(selectedEffort(model, "max"), "max");
  assert.equal(selectedEffort(model, "medium"), "high");
  assert.equal(selectedEffort({ ...model, defaultReasoningEffort: "invalid" }), "off");
  assert.equal(selectedEffort(undefined, "high"), "");
  assert.equal(selectedEffort({ supportedReasoningEfforts: [] }, "high"), "");
});
