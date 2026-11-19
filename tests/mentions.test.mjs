import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true }, appType: "custom" });
after(() => server.close());
const { activeMention, insertMention } = await server.ssrLoadModule("/src/features/chats/mentions.ts");

test("opens mentions after a previous member, body text and a newline", () => {
  for (const text of ["@", "@Codex @", "@Codex 检查后交给 @De", "正文\n@de", "请检查，@De"]) {
    const match = activeMention(text, text.length);
    assert.ok(match, text);
    assert.equal(text[match.start], "@");
    assert.equal(match.query, text.slice(match.start + 1).toLowerCase());
  }
  assert.equal(activeMention("mail@example", 12), null);
  assert.equal(activeMention("@Codex 正文", 9), null);
});

test("completes the second mention without replacing the first member or suffix", () => {
  const text = "@Codex @De 继续核对";
  const next = insertMention(text, 10, 10, "DeepSeek");
  assert.equal(next.text, "@Codex @DeepSeek  继续核对");
  assert.equal(next.cursor, "@Codex @DeepSeek ".length);
  assert.equal(insertMention("@Co", 3, 3, "Codex").text, "@Codex ");
});

test("toolbar inserts at the caret or replaces only the selected text", () => {
  assert.deepEqual(insertMention("@Codex 正文", 7, 7, "DeepSeek"), { text: "@Codex @DeepSeek 正文", cursor: 17 });
  assert.equal(insertMention("前文后文", 2, 2, "Codex").text, "前文 @Codex 后文");
  assert.equal(insertMention("前文 旧名字 后文", 3, 6, "DeepSeek").text, "前文 @DeepSeek  后文");
});
