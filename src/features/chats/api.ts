import { invoke, isTauri } from "@tauri-apps/api/core";
export const desktop = isTauri();
export interface Group { id: string; name: string; members: string[]; project_id: string | null; created_at: string; preview: string | null; updated_at: string | null; status: string | null }
export interface Model { model: string; displayName: string; isDefault: boolean }
export interface Probe { version: string; executable: string; auth: string | null; requires_auth: boolean; models: Model[]; sidebar_warning: string | null }
export interface Message { id: string; role: string; text: string; created_at: string | null }
export interface Snapshot {
  session: { thread_id: string; status: string; model: string; error: string | null } | null;
  messages: Message[];
  approvals: { id: string; method: string; command: string | null; reason: string | null; cwd: string | null }[];
}
export interface Scope { projectId: string | null; groupId: string | null }
export const emptySnapshot: Snapshot = { session: null, messages: [], approvals: [] };
export const agents = [{ id: "codex", name: "Codex", description: "写代码、执行任务、整理交付", available: true }, { id: "deepseek", name: "DeepSeek", description: "讨论思路、检查方案", available: false }];
const previewKey = "ai-gent.preview.groups.v1";
const previewMessagesKey = "ai-gent.preview.messages.v1";
function readPreview<T>(key: string, fallback: T): T { const raw = localStorage.getItem(key); if (!raw) return fallback; try { return JSON.parse(raw) as T; } catch { throw new Error("本机预览记录无法读取，请检查浏览器存储。"); } }
export const getGroups = async () => desktop ? invoke<Group[]>("group_list") : readPreview<Group[]>(previewKey, []);
export async function saveGroup(name: string, members: string[], projectId: string | null, existingId?: string) {
  if (desktop) return existingId ? invoke<Group>("group_update", { id: existingId, name, members }) : invoke<Group>("group_create", { name, members, projectId });
  const groups = await getGroups();
  const existing = groups.find(g => g.id === existingId);
  const group: Group = { id: existingId ?? crypto.randomUUID(), name: name.trim(), members, project_id: existing?.project_id ?? projectId, created_at: existing?.created_at ?? new Date().toISOString(), preview: existing ? "群成员已更新" : "群聊已创建", updated_at: new Date().toISOString(), status: null };
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  messages[group.id] = [...messages[group.id] ?? [], { id: crypto.randomUUID(), role: "system", text: existing ? "群成员已更新" : "群聊已创建", created_at: new Date().toISOString() }];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewKey, JSON.stringify([group, ...groups.filter(g => g.id !== group.id)]));
  return group;
}
export const getSnapshot = async (scope: Scope): Promise<Snapshot> => desktop ? invoke<Snapshot>("codex_snapshot", { ...scope }) : { ...emptySnapshot, messages: readPreview<Record<string, Message[]>>(previewMessagesKey, {})[scope.groupId ?? "personal"] ?? [] };
export const probeCodex = () => invoke<Probe>("codex_probe");
export const running = (status?: string) => ["starting", "running", "waiting_approval", "cancel_requested", "stopping"].includes(status ?? "");
export const statusLabels: Record<string, string> = { idle: "就绪", starting: "正在启动", running: "正在处理", waiting_approval: "等待你批准", cancel_requested: "正在取消", stopping: "清理进程中", completed: "本轮完成", cancelled: "已取消", failed: "执行失败", interrupted: "执行已中断" };
export function clock(value: string | null) { return value ? new Date(value).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) : ""; }
