import test from "node:test";
import assert from "node:assert/strict";
import { deriveSessionTitle } from "../src/features/chats/sessionTitle.ts";

test("deriveSessionTitle cleans whitespace and strips leading mentions/skills", () => {
  assert.equal(deriveSessionTitle("  帮我重构一下\n登录流程  "), "帮我重构一下 登录流程");
  assert.equal(deriveSessionTitle("@Codex 修复新建会话弹窗"), "修复新建会话弹窗");
  assert.equal(deriveSessionTitle("/plan   给项目加单元测试"), "给项目加单元测试");
  assert.equal(deriveSessionTitle("@Codex /review 检查这个 PR"), "检查这个 PR");
  assert.equal(deriveSessionTitle("@Codex"), "@Codex");
});

test("deriveSessionTitle truncates long prompts to <= 24 chars", () => {
  const title = deriveSessionTitle("这是一个非常长的需求描述用来测试当用户输入超过二十四个字时是否会自动截断");
  assert.equal([...title].length, 24);
  assert.ok(title.endsWith("…"));
});

test("deriveSessionTitle handles attachment-only messages and empty fallback", () => {
  assert.equal(deriveSessionTitle("", [{ name: "login-mockup.png", isImage: true }]), "图片：login-mockup.png");
  assert.equal(deriveSessionTitle("", [{ name: "spec.pdf", isImage: false }]), "附件：spec.pdf");
  assert.equal(
    deriveSessionTitle("", [
      { name: "a.png", isImage: true },
      { name: "b.png", isImage: true },
    ]),
    "查看 2 张图片",
  );
  assert.equal(
    deriveSessionTitle("", [
      { name: "a.png", isImage: true },
      { name: "b.txt", isImage: false },
    ]),
    "查看 2 个附件",
  );
  assert.equal(deriveSessionTitle("   ", []), "新会话");
});
