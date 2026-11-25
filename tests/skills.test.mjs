import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true, hmr: false }, optimizeDeps: { noDiscovery: true }, appType: "custom" });
after(() => server.close());
const { deleteSkillTokenAtCursor, requestedSkills, skillCommandSpans, skillPrefix } = await server.ssrLoadModule("/src/features/chats/skills.ts");
const { default: MessageBody } = await server.ssrLoadModule("/src/features/chats/MessageBody.tsx");
const render = text => renderToStaticMarkup(createElement(MessageBody, { text }));

test("parses leading skill command spans across mentions while ignoring paths and prose", () => {
  assert.deepEqual(skillCommandSpans("/lovart-api"), [{ start: 0, end: 11, name: "lovart-api" }]);
  assert.deepEqual(skillCommandSpans("/lovart-api /lark-doc 帮我查"), [
    { start: 0, end: 11, name: "lovart-api" },
    { start: 12, end: 21, name: "lark-doc" },
  ]);
  assert.deepEqual(skillCommandSpans("@Claude /lovart-api 帮我查"), [
    { start: 8, end: 19, name: "lovart-api" },
  ]);
  assert.deepEqual(requestedSkills("@Gemini /lovart-api /lovart-api 测试"), ["lovart-api"]);
  assert.deepEqual(skillCommandSpans("/Users/nangua/Documents/file.ts"), []);
  assert.deepEqual(skillCommandSpans("帮我看看 /lovart-api"), []);
  assert.equal(skillPrefix("@Claude /lov")?.[1], "lov");
});

test("deletes whole skill token on Backspace at token end or trailing space", () => {
  assert.deepEqual(deleteSkillTokenAtCursor("/lovart-api 帮我查", 12, 12), {
    text: "帮我查",
    cursor: 0,
  });
  assert.deepEqual(deleteSkillTokenAtCursor("/lovart-api", 11, 11), {
    text: "",
    cursor: 0,
  });
  assert.deepEqual(deleteSkillTokenAtCursor("@Codex /lovart-api ", 19, 19), {
    text: "@Codex ",
    cursor: 7,
  });
  assert.equal(deleteSkillTokenAtCursor("/lovart-api 帮我查", 5, 5), null);
});

test("renders leading skill commands as styled badges alongside mentions in MessageBody", () => {
  const html = render("@Codex /lovart-api 帮我核对");
  assert.match(html, /class="message-mention"[^>]*>@Codex<\/span>/);
  assert.match(html, /class="message-skill"[^>]*data-skill="lovart-api"[^>]*>\/lovart-api<\/span>/);

  const plainPath = render("/Users/nangua/foo.ts");
  assert.doesNotMatch(plainPath, /message-skill/);
});
