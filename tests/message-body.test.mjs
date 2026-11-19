import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true }, appType: "custom" });
after(() => server.close());
const { default: MessageBody } = await server.ssrLoadModule("/src/features/chats/MessageBody.tsx");
const { default: ChatStage } = await server.ssrLoadModule("/src/features/chats/ChatStage.tsx");
const render = text => renderToStaticMarkup(createElement(MessageBody, { text }));

test("renders the review table with inline code and aligned cells", () => {
  const html = render("核对结论：实现符合要求\n\n| 要求 | 结果 | 依据 |\n| --- | :---: | --- |\n| 初始显示 0 | ✅ | 第 62 行 `<output>` 内容为 0 |\n| 加一 | ✅ | `update(count + 1)` |");
  assert.match(html, /class="message-table"[^>]*tabindex="0"/);
  assert.match(html, /<thead><tr><th>要求<\/th>/);
  assert.match(html, /<td style="text-align:center">✅<\/td>/);
  assert.match(html, /<code>&lt;output&gt;<\/code>/);
  assert.equal((html.match(/<tbody><tr>|<\/tr><tr>/g) ?? []).length, 2);
});

test("renders headings, nested lists, tasks, quotes, links and fenced code", () => {
  const html = render("## 标题\n\n**加粗** *斜体* ~~删除~~\n\n1. 第一项\n   - 子项\n\n- [x] 完成\n\n> 引用\n\n[文档](https://example.com)\n\n````html\n<output>0</output>\n```\n````");
  for (const fragment of ["<h2>标题</h2>", "<strong>加粗</strong>", "<em>斜体</em>", "<del>删除</del>", "<ol>", "<ul>", 'type="checkbox"', 'checked=""', "<blockquote>", 'target="_blank"', 'rel="noopener noreferrer"', '<code class="language-html">&lt;output&gt;0&lt;/output&gt;\n```']) assert.ok(html.includes(fragment), fragment);
});

test("keeps raw HTML inert and rejects executable links", () => {
  const html = render('<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[点击](javascript:alert%281%29)');
  assert.doesNotMatch(html, /<script|<img|href="javascript:/);
  assert.match(html, /&lt;script&gt;/);
});

test("keeps plain text line breaks and renders an unfinished streamed code block", () => {
  assert.match(render("第一行\n第二行"), /<p>第一行\n第二行<\/p>/);
  assert.match(render("```ts\nconst value = 1;"), /<pre><code class="language-ts">const value = 1;\n<\/code><\/pre>/);
});

test("merges consecutive replies across tools and preserves speaker and status boundaries", () => {
  globalThis.localStorage = { getItem: () => null };
  try {
    const messages = [
      { role: "assistant", agent: "codex", text: "第一段" },
      { role: "tool", text: "工具结果" },
      { role: "assistant", agent: "codex", text: "第二段" },
      { role: "assistant", agent: "deepseek", text: "另一个成员" },
      { role: "system", text: "状态边界" },
      { role: "assistant", agent: "deepseek", text: "状态后回复" },
      { role: "user", text: "新问题" },
      { role: "assistant", agent: "deepseek", text: "新回复" },
    ].map((message, index) => ({ ...message, id: String(index), created_at: null }));
    const html = renderToStaticMarkup(createElement(ChatStage, {
      scope: { projectId: null, groupId: null }, scopeKey: "markdown-test", title: "测试",
      snapshot: { session: null, messages, approvals: [] }, probe: null, model: "", connecting: false,
      collaborationModels: { codex: "", deepseek: "" },
    }));
    assert.equal((html.match(/class="message-body"/g) ?? []).length, 5);
    assert.match(html, /第一段<\/p>\n<p>第二段/);
    assert.doesNotMatch(html, /工具结果/);
    assert.equal(messages[0].text, "第一段");
  } finally {
    delete globalThis.localStorage;
  }
});
