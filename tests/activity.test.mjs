import { after, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
const server = await createServer({ configFile: false, server: { middlewareMode: true, hmr: false, watch: { ignored: ["**/src-tauri/**"] } }, optimizeDeps: { noDiscovery: true }, appType: "custom" });
after(() => server.close());
const { replyBlocks, elapsed } = await server.ssrLoadModule("/src/features/chats/activity.ts");
const { default: TaskActivity } = await server.ssrLoadModule("/src/features/chats/TaskActivity.tsx");
const message = (id, role, text, seconds, agent = "codex") => ({ id, role, text, created_at: `2026-11-19T10:${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}Z`, agent });
const snapshot = messages => ({ session: { status: "completed" }, messages, approvals: [] });

test("retains progress, separates the final answer, and uses the persisted end time", () => {
  const messages = [message("u", "user", "修复滚动", 0), message("s", "task_start", "running", 2), message("p", "assistant", "检查图片", 3), message("c", "tool", "命令：npm test\n状态：completed", 30), message("f", "assistant", "修复完成", 120), message("e", "task_end", "completed", 145)];
  const block = replyBlocks(snapshot(messages), "codex")[1];
  assert.equal(block.text, "修复完成");
  assert.deepEqual(block.activity.map(m => m.id), ["p", "c"]);
  assert.equal(elapsed(block.startedAt, block.endedAt, Date.now()), "2 分 23 秒");
  const html = renderToStaticMarkup(createElement(TaskActivity, { block }));
  assert.match(html, /用时 2分23秒/);
  assert.match(html, /1 次命令/);
  assert.match(html, /aria-expanded="false"/);
  assert.doesNotMatch(html, /检查图片/);
  assert.equal(messages[2].text, "检查图片");
});

test("shows live progress header before the first output and does not reuse a previous answer", () => {
  const s = snapshot([message("s", "task_start", "running", 0)]);
  s.session.status = "running";
  const block = replyBlocks(s, "codex")[0];
  assert.equal(block.active, true);
  assert.equal(block.text, "");
  const html = renderToStaticMarkup(createElement(TaskActivity, { block }));
  assert.match(html, /is-active/);
  assert.match(html, /正在思考/);
  assert.match(html, /aria-expanded="false"/);
});

test("keeps interleaved group members in their own task records", () => {
  const s = snapshot([message("cs", "task_start", "running", 0), message("ds", "task_start", "running", 1, "deepseek"), message("cp", "assistant", "检查代码", 2), message("dp", "assistant", "检查设计", 3, "deepseek"), message("ct", "tool", "命令：test", 4), message("cf", "assistant", "代码完成", 5), message("ce", "task_end", "completed", 6)]);
  s.sessions = { codex: { status: "completed" }, deepseek: { status: "running" } };
  const blocks = replyBlocks(s, "codex");
  assert.equal(blocks.length, 2);
  assert.equal(blocks[0].text, "代码完成");
  assert.equal(blocks[0].active, false);
  assert.equal(blocks[1].active, true);
  assert.equal(blocks[1].text, "检查设计");
  assert.deepEqual(blocks[1].activity, []);
});

test("omits empty worklog for plain legacy replies and handles missing and UTC timestamps", () => {
  const block = replyBlocks(snapshot([message("u", "user", "问题", 0), message("a", "assistant", "回复", 30)]), "codex")[1];
  assert.equal(renderToStaticMarkup(createElement(TaskActivity, { block })), "");
  assert.equal(elapsed("2026-11-19 10:00:00", "2026-11-19T10:01:02Z", 0), "1 分 2 秒");
  assert.equal(elapsed(null, null, 0), "耗时未知");
});


test("keeps generated images visible beside the final reply", () => {
  const block = replyBlocks(snapshot([message("u", "user", "生成图片", 0), message("t", "tool", "命令：draw", 1), message("i", "assistant", "![图片](ai-gent-image:example)", 2), message("a", "assistant", "完成", 3)]), "codex")[1];
  assert.match(block.text, /!\[图片\]/);
  assert.match(block.text, /完成/);
});
