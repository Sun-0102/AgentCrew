import { useSyncExternalStore } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
export const desktop = isTauri();
export interface Group { id: string; name: string; members: string[]; project_id: string | null; created_at: string; preview: string | null; updated_at: string | null; status: string | null; session_count?: number }
export interface GroupSession { id: string; group_id: string; name: string; archived: boolean; created_at: string; preview: string | null; updated_at: string | null; status: string | null }
export interface PrivateSession { id: string; agent_id: string; name: string; archived: boolean; is_default?: boolean; created_at: string; preview: string | null; updated_at: string | null; status: string | null }
export interface ProjectSession { id: string; project_id: string; name: string; archived: boolean; is_default?: boolean; created_at: string; preview: string | null; updated_at: string | null; status: string | null }
export interface Model { model: string; displayName: string; isDefault: boolean; supportedReasoningEfforts?: { reasoningEffort: string; description: string }[]; defaultReasoningEffort?: string; serviceTiers?: { id: string; name: string; description: string }[] }
export interface Probe { version: string; executable: string; auth: string | null; requires_auth: boolean; models: Model[]; sidebar_warning: string | null }
export interface Message { agent?: string; id: string; role: string; text: string; created_at: string | null }
export interface GroupTask {
  id: string; conversation_id: string; objective: string; status: string;
  current_agent: string; codex_model: string; deepseek_model: string;
  turn_count: number; queued_count: number; error: string | null;
  members?: { agent: string; status: string; model?: string; queued_count: number; error: string | null }[];
}
export interface TaskSummary { id: string; objective: string; status: string; created_at: string }
export interface FileEvidence { path: string; state: "captured" | "truncated" | "unavailable"; bytes: number | null; text: string | null; note: string | null }
export interface WorkspaceEvidence { root: string; captured_at_ms: number; head: string; git_status: string; diff: string; files: FileEvidence[]; limitations: string[] }
export interface TaskStep {
  id: number; ordinal: number; agent: string; source_agent: string; source_message: string | null;
  instruction: string; status: string; created_at: string; finished_at: string | null;
  before: WorkspaceEvidence; after: WorkspaceEvidence | null;
  messages: { id: string; role: string; text: string; created_at: string }[];
}
export interface TaskResult {
  format_version: number;
  task: TaskSummary & { conversation_id: string; current_agent: string; turn_count: number; note: string | null; workspace_path: string | null; updated_at: string };
  inputs: { message_id: string; target_agent: string; text: string; status: string; created_at: string }[];
  steps: TaskStep[]; limitations: string[];
}
export const taskLabels: Record<string, string> = { idle: "等待新消息", running: "正在处理", paused: "已暂停接力", cancelling: "正在取消", cancelled: "已取消", interrupted: "协作已中断", blocked: "需要你处理", awaiting_review: "待你验收", completed: "已验收", manual: "已转手动指派" };
export interface ContextUsage {
  agentId: string;
  providerKind: string;
  model: string;
  usedTokens: number;
  contextWindow: number;
  percent: number;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  source: "live" | "recorded" | "none";
}
export interface QueuedMessage {
  id: string;
  conversation_id: string;
  agent_id: string;
  model: string;
  text: string;
  reasoning_effort?: string | null;
  service_tier?: string | null;
  status: "queued" | "paused";
  created_at: string;
}
export interface Snapshot {
  conversation_id?: string;
  sessions?: Partial<Record<string, Snapshot["session"]>>;
  context_usage?: Record<string, ContextUsage>;
  agent?: string;
  task?: GroupTask | null;
  queued_messages?: QueuedMessage[];
  session: { workspace_path?: string; thread_id: string; status: string; model: string; error: string | null } | null;
  messages: Message[];
  approvals: { agent_id?: string; id: string; method: string; command: string | null; reason: string | null; cwd: string | null }[];
}
// In runtime commands groupId identifies the selected execution conversation, including the default session.
export interface Scope { projectId: string | null; groupId: string | null; sessionId?: string | null; agent?: string }
export const emptySnapshot: Snapshot = { session: null, messages: [], approvals: [] };
export interface ProviderCapabilities {
  resume?: boolean;
  cancel?: boolean;
  approval?: boolean;
  tools?: boolean;
  streaming?: boolean;
  acp?: boolean;
  steer?: boolean;
}
export interface Agent {
  id: string;
  name: string;
  description: string;
  provider_kind: string;
  runtime_kind?: string;
  runtime_label?: string;
  available: boolean;
  connected: boolean;
  registration_state: string;
  availability: string;
  enabled: boolean;
  runtime_version?: string | null;
  executable_path?: string | null;
  capabilities?: ProviderCapabilities;
  error?: string | null;
}
export interface DiscoveryItem {
  provider_kind: string;
  display_name: string;
  runtime_kind?: string;
  runtime_label?: string;
  status: "connected" | "needs_auth" | "unavailable";
  version: string | null;
  executable: string | null;
  auth?: string | null;
  model_count: number;
  capabilities?: ProviderCapabilities;
  message: string;
  auth_hint?: string | null;
}
export let agents: Agent[] = (desktop ? [] : ["codex", "deepseek"]).map(id => ({ id, name: id === "codex" ? "Codex" : "DeepSeek", description: "本机运行组件", provider_kind: id, available: !desktop, connected: !desktop, registration_state: "active", availability: desktop ? "checking" : "ready", enabled: true }));
const registryListeners = new Set<() => void>();
let registryRequest: Promise<void> | undefined;
export async function refreshAgents() {
  if (!desktop) return;
  const rows = await invoke<(Omit<Agent, "name" | "description" | "available" | "connected"> & {display_name: string})[]>("agent_list");
  agents = rows.map(row => {
    const available = row.enabled && row.registration_state === "active";
    const connected = available && (row.availability === "ready" || row.availability === "checking");
    return { ...row, name: row.display_name, description: row.error ?? `${row.provider_kind} · ${row.availability === "ready" ? "已连接" : row.availability === "checking" ? "正在连接…" : "未连接"}`, available, connected };
  });
  registryListeners.forEach(listener => listener());
}
export async function autoDiscoverAgents(autoEnable = true): Promise<DiscoveryItem[]> {
  if (!desktop) {
    return [
      { provider_kind: "codex", display_name: "Codex", status: "connected", version: "preview", executable: null, model_count: 1, message: "浏览器预览模式" },
      { provider_kind: "deepseek", display_name: "DeepSeek", status: "connected", version: "preview", executable: null, model_count: 1, message: "浏览器预览模式" },
    ];
  }
  const results = await invoke<DiscoveryItem[]>("agent_auto_discover", { autoEnable });
  await refreshAgents();
  return results;
}
export function useAgents() {
  return useSyncExternalStore(listener => { registryListeners.add(listener); return () => { registryListeners.delete(listener); }; }, () => agents, () => agents);
}
export function loadAgents() { return registryRequest ??= refreshAgents().catch(error => { registryRequest = undefined; throw error; }); }
export const agentName = (id: string) => agents.find(agent => agent.id === id)?.name ?? id;
export const providerKind = (id: string) => agents.find(agent => agent.id === id)?.provider_kind ?? id;
export const mentionName = (id: string) => {
  const agent=agents.find(a=>a.id===id);
  return agent && agents.filter(a=>a.name.toLowerCase()===agent.name.toLowerCase()).length===1 ? agent.name : id;
};
const previewKey = "ai-gent.preview.groups.v1";
const previewMessagesKey = "ai-gent.preview.messages.v1";
const previewSessionsKey = "ai-gent.preview.sessions.v1";
function readPreview<T>(key: string, fallback: T): T { const raw = localStorage.getItem(key); if (!raw) return fallback; try { return JSON.parse(raw) as T; } catch { throw new Error("本机预览记录无法读取，请检查浏览器存储。"); } }
export const getGroups = async () => {
  if (desktop) return invoke<Group[]>("group_list");
  const sessions = readPreview<Record<string, GroupSession[]>>(previewSessionsKey, {});
  return readPreview<Group[]>(previewKey, []).map(group => ({ ...group, session_count: sessions[group.id]?.filter(s => !s.archived).length ?? 1 }));
};
export async function getGroupSessions(groupId: string): Promise<GroupSession[]> {
  if (desktop) return invoke<GroupSession[]>("group_session_list", { groupId });
  const group = (await getGroups()).find(g => g.id === groupId);
  if (!group) throw new Error("群聊不存在。");
  return readPreview<Record<string, GroupSession[]>>(previewSessionsKey, {})[groupId] ?? [{ id: groupId, group_id: groupId, name: "默认会话", archived: false, created_at: group.created_at, preview: group.preview, updated_at: group.updated_at, status: null }];
}
export async function saveGroupSession(groupId: string, name: string, existing?: GroupSession, archived = existing?.archived ?? false): Promise<GroupSession> {
  name = name.trim();
  if (!name || [...name].length > 40) throw new Error("会话名称请填写 1–40 个字。");
  if (desktop) return existing ? invoke<GroupSession>("group_session_update", { groupId, id: existing.id, name, archived }) : invoke<GroupSession>("group_session_create", { groupId, name });
  const list = await getGroupSessions(groupId);
  if (existing && !list.some(s => s.id === existing.id)) throw new Error("会话不属于当前群聊。");
  const session: GroupSession = existing ? { ...existing, name, archived } : { id: crypto.randomUUID(), group_id: groupId, name, archived: false, created_at: new Date().toISOString(), preview: null, updated_at: null, status: null };
  const all = readPreview<Record<string, GroupSession[]>>(previewSessionsKey, {});
  all[groupId] = existing ? list.map(s => s.id === session.id ? session : s) : [session, ...list];
  localStorage.setItem(previewSessionsKey, JSON.stringify(all));
  return session;
}
export async function saveGroup(name: string, members: string[], projectId: string | null, existingId?: string) {
  if (desktop) return existingId ? invoke<Group>("group_update", { id: existingId, name, members, projectId }) : invoke<Group>("group_create", { name, members, projectId });
  const groups = await getGroups();
  const existing = groups.find(g => g.id === existingId);
  const group: Group = { id: existingId ?? crypto.randomUUID(), name: name.trim(), members, project_id: projectId, created_at: existing?.created_at ?? new Date().toISOString(), preview: existing ? "群设置已更新" : "群聊已创建", updated_at: new Date().toISOString(), status: null };
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  messages[group.id] = [...messages[group.id] ?? [], { id: crypto.randomUUID(), role: "system", text: existing ? "群设置已更新" : "群聊已创建", created_at: new Date().toISOString() }];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewKey, JSON.stringify([group, ...groups.filter(g => g.id !== group.id)]));
  return group;
}
export async function deleteGroup(id: string) {
  if (desktop) return invoke<void>("group_delete", { id });
  const groups = await getGroups();
  if (!groups.some(group => group.id === id)) throw new Error("群聊不存在或已删除。");
  const all = readPreview<Record<string, GroupSession[]>>(previewSessionsKey, {});
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  for (const conversationId of [id, ...(all[id] ?? []).map(session => session.id)]) delete messages[conversationId];
  delete all[id];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewSessionsKey, JSON.stringify(all));
  localStorage.setItem(previewKey, JSON.stringify(groups.filter(group => group.id !== id)));
}
export async function deleteGroupSession(groupId: string, id: string) {
  if (desktop) return invoke<void>("group_session_delete", { groupId, id });
  const list = await getGroupSessions(groupId);
  if (!list.some(s => s.id === id)) throw new Error("会话不存在或已删除。");
  const all = readPreview<Record<string, GroupSession[]>>(previewSessionsKey, {});
  all[groupId] = list.filter(s => s.id !== id);
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  delete messages[id];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewSessionsKey, JSON.stringify(all));
}
const previewPrivateSessionsKey = "ai-gent.preview.private-sessions.v1";
export async function getPrivateSessions(agentId: string): Promise<PrivateSession[]> {
  if (desktop) return invoke<PrivateSession[]>("private_session_list", { agentId });
  const all = readPreview<Record<string, PrivateSession[]>>(previewPrivateSessionsKey, {});
  return all[agentId] ?? [{ id: agentId === "deepseek" ? "deepseek" : agentId === "codex" ? "personal" : `agent:${agentId}:personal`, agent_id: agentId, name: "默认会话", archived: false, is_default: true, created_at: new Date().toISOString(), preview: null, updated_at: null, status: null }];
}
export async function savePrivateSession(agentId: string, name: string, existing?: PrivateSession, archived = existing?.archived ?? false): Promise<PrivateSession> {
  name = name.trim();
  if (!name || [...name].length > 40) throw new Error("会话名称请填写 1–40 个字。");
  if (desktop) return existing ? invoke<PrivateSession>("private_session_update", { agentId, id: existing.id, name, archived }) : invoke<PrivateSession>("private_session_create", { agentId, name });
  const list = await getPrivateSessions(agentId);
  if (existing && !list.some(s => s.id === existing.id)) throw new Error("会话不属于当前 Agent。");
  const session: PrivateSession = existing ? { ...existing, name, archived } : { id: crypto.randomUUID(), agent_id: agentId, name, archived: false, is_default: false, created_at: new Date().toISOString(), preview: null, updated_at: null, status: null };
  const all = readPreview<Record<string, PrivateSession[]>>(previewPrivateSessionsKey, {});
  all[agentId] = existing ? list.map(s => s.id === session.id ? session : s) : [session, ...list];
  localStorage.setItem(previewPrivateSessionsKey, JSON.stringify(all));
  return session;
}
export async function deletePrivateSession(agentId: string, id: string) {
  if (desktop) return invoke<void>("private_session_delete", { agentId, id });
  const list = await getPrivateSessions(agentId);
  const target = list.find(s => s.id === id);
  if (!target) throw new Error("会话不存在或已删除。");
  const all = readPreview<Record<string, PrivateSession[]>>(previewPrivateSessionsKey, {});
  all[agentId] = target.is_default ? list : list.filter(s => s.id !== id);
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  delete messages[id];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewPrivateSessionsKey, JSON.stringify(all));
}
const previewProjectSessionsKey = "ai-gent.preview.project-sessions.v1";
export async function getProjectSessions(projectId: string): Promise<ProjectSession[]> {
  if (desktop) return invoke<ProjectSession[]>("project_session_list", { projectId });
  const all = readPreview<Record<string, ProjectSession[]>>(previewProjectSessionsKey, {});
  return all[projectId] ?? [{ id: projectId, project_id: projectId, name: "默认会话", archived: false, is_default: true, created_at: new Date().toISOString(), preview: null, updated_at: null, status: null }];
}
export async function saveProjectSession(projectId: string, name: string, existing?: ProjectSession, archived = existing?.archived ?? false): Promise<ProjectSession> {
  name = name.trim();
  if (!name || [...name].length > 40) throw new Error("会话名称请填写 1–40 个字。");
  if (desktop) return existing ? invoke<ProjectSession>("project_session_update", { projectId, id: existing.id, name, archived }) : invoke<ProjectSession>("project_session_create", { projectId, name });
  const list = await getProjectSessions(projectId);
  if (existing && !list.some(s => s.id === existing.id)) throw new Error("会话不属于当前项目。");
  const session: ProjectSession = existing ? { ...existing, name, archived } : { id: crypto.randomUUID(), project_id: projectId, name, archived: false, is_default: false, created_at: new Date().toISOString(), preview: null, updated_at: null, status: null };
  const all = readPreview<Record<string, ProjectSession[]>>(previewProjectSessionsKey, {});
  all[projectId] = existing ? list.map(s => s.id === session.id ? session : s) : [session, ...list];
  localStorage.setItem(previewProjectSessionsKey, JSON.stringify(all));
  return session;
}
export async function deleteProjectSession(projectId: string, id: string) {
  if (desktop) return invoke<void>("project_session_delete", { projectId, id });
  const list = await getProjectSessions(projectId);
  const target = list.find(s => s.id === id);
  if (!target) throw new Error("会话不存在或已删除。");
  const all = readPreview<Record<string, ProjectSession[]>>(previewProjectSessionsKey, {});
  all[projectId] = target.is_default ? list : list.filter(s => s.id !== id);
  const messages = readPreview<Record<string, Message[]>>(previewMessagesKey, {});
  delete messages[id];
  localStorage.setItem(previewMessagesKey, JSON.stringify(messages));
  localStorage.setItem(previewProjectSessionsKey, JSON.stringify(all));
}
export async function updateGroupMembers(groupId: string, add: string[], remove: string[]) {
  if (desktop) return invoke<Group>("group_members_update", { groupId, add, remove });
  const group = (await getGroups()).find(g => g.id === groupId);
  if (!group) throw new Error("群聊不存在。");
  const members = [...new Set([...group.members.filter(agent => !remove.includes(agent)), ...add])];
  if (!members.length || members.some(agent => !agents.some(a => a.id === agent))) throw new Error("群内至少保留一位已接入的 Agent。");
  return saveGroup(group.name, members, group.project_id, groupId);
}
export const getSnapshot = async (scope: Scope): Promise<Snapshot> => desktop ? invoke<Snapshot>("agent_snapshot", { ...scope, agent: scope.agent ?? "codex" }) : { ...emptySnapshot, messages: readPreview<Record<string, Message[]>>(previewMessagesKey, {})[scope.groupId ?? scope.sessionId ?? (scope.agent === "deepseek" ? "deepseek" : "personal")] ?? [] };
export const probeAgent = (agent: string) => invoke<Probe>("agent_probe", { agent });
export const probeCodex = () => probeAgent("codex");
export const probeDeepSeek = () => probeAgent("deepseek");
export const running = (status?: string) => ["starting", "running", "waiting_approval", "cancel_requested", "stopping"].includes(status ?? "");
export const statusLabels: Record<string, string> = { idle: "就绪", starting: "正在启动", running: "正在处理", waiting_approval: "等待你批准", cancel_requested: "正在取消", stopping: "清理进程中", completed: "本轮完成", cancelled: "已取消", failed: "执行失败", interrupted: "执行已中断" };
export function clock(value: string | null) { return value ? new Date(value).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) : ""; }
