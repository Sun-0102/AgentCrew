import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
after(() => server.close());
const { default: GroupMemberStatus } = await server.ssrLoadModule("/src/features/chats/GroupMemberStatus.tsx");
const render = (codex, deepseek, extra = {}) => renderToStaticMarkup(createElement(GroupMemberStatus, {
  members: ["codex", "deepseek"], snapshot: {
    task: { status: "running", members: [{ agent: "codex", status: codex, queued_count: 0 }, { agent: "deepseek", status: deepseek, queued_count: 1 }] },
    ...extra,
  },
}));

test("shows two independent active statuses and per-member queue counts", () => {
  const html = render("running", "running");
  assert.equal((html.match(/正在处理/g) ?? []).length, 2);
  assert.match(html, /Codex 执行状态/);
  assert.match(html, /DeepSeek 执行状态/);
  assert.match(html, /1 项待处理/);
});

test("one completed reply does not hide the other running member or an approval", () => {
  const html = render("completed", "running");
  assert.match(html, /本轮完成/);
  assert.match(html, /正在处理/);
  const waiting = render("running", "running", { sessions: { codex: { status: "waiting_approval" }, deepseek: { status: "running" } } });
  assert.match(waiting, /等待你批准/);
  assert.match(waiting, /正在处理/);
});

test("failed member shows its own error while the other continues", () => {
  const html = render("blocked", "running", { task: { status: "running", members: [
    { agent: "codex", status: "blocked", queued_count: 0, error: "连接中断" },
    { agent: "deepseek", status: "running", queued_count: 0 },
  ] } });
  assert.match(html, /执行未完成/);
  assert.match(html, /连接中断/);
  assert.match(html, /正在处理/);
});
