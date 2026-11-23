import { invoke } from "@tauri-apps/api/core";
import ConnectedChat from "./features/chats/ConnectedChat";
import PairingDialog, { type Invitation } from "./features/chats/PairingDialog";
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { addProject, getAppInfo, listProjects, type AppInfo, type Project } from "./features/projects/api";
import { useAgents, loadAgents, refreshAgents, autoDiscoverAgents, agentName, clock, desktop, emptySnapshot, getGroups, getGroupSessions, getPrivateSessions, getSnapshot, probeCodex, probeDeepSeek, running, saveGroupSession, savePrivateSession, type DiscoveryItem, type Group, type GroupSession, type PrivateSession, type Probe, type Scope, type Snapshot } from "./features/chats/api";
import { discardSessionDraft } from "./features/chats/ChatStage";
import GroupDialog from "./features/chats/GroupDialog";
import { GroupHub, SessionSidebar, SessionDialog, DeleteSessionDialog, type ChatSession } from "./features/chats/GroupSessions";
import Icon from "./components/Icon";
import Avatar from "./components/Avatar";
import Dialog from "./components/Dialog";
import appIcon from "./assets/app-icon.png";

type Selection = { kind: "agent"; id: string } | { kind: "personal" } | { kind: "deepseek" } | { kind: "group"; id: string } | { kind: "project"; id: string };
export default function Workbench() {
  const agents = useAgents();
  const [agentsLoaded, setAgentsLoaded] = useState(!desktop);
  const connectedAgents = agents.filter(agent => agent.connected);
  const checkingAgents = agents.some(agent => agent.available && agent.availability === "checking");
  const hasCodex = agents.some(agent => agent.id === "codex" && agent.available);
  const hasDeepseek = agents.some(agent => agent.id === "deepseek" && agent.available);
  const hasCodexConnected = agents.some(agent => agent.id === "codex" && agent.connected);
  const hasDeepseekConnected = agents.some(agent => agent.id === "deepseek" && agent.connected);
  const [pairing, setPairing] = useState(false);
  const [invitation, setInvitation] = useState<Invitation | null>(null);
  const [paused, setPaused] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [discoveryResults, setDiscoveryResults] = useState<DiscoveryItem[]>([]);
  useEffect(() => { void loadAgents().then(() => setAgentsLoaded(true)).catch(cause => {setError(String(cause));setAgentsLoaded(true);}); }, []);
  useEffect(() => {
    if (!desktop) return;
    let active = true; let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { await refreshAgents(); const value = await invoke<boolean>("dispatch_snapshot"); if(active) setPaused(value); }
      catch(cause) {if(active) setError(String(cause));}
      if(active) timer = setTimeout(poll,3500);
    };
    void poll(); return () => {active=false;clearTimeout(timer);};
  }, []);
  async function toggleDispatch() {try {await invoke("dispatch_control",{paused:!paused});setPaused(!paused);}catch(cause){setError(String(cause));}}
  const [projects, setProjects] = useState<Project[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [groupHub, setGroupHub] = useState(false);
  const [sessions, setSessions] = useState<GroupSession[]>([]);
  const [sessionsOwner, setSessionsOwner] = useState("");
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const sessionsRevision = useRef(0);
  const [privateSessions, setPrivateSessions] = useState<PrivateSession[]>([]);
  const [privateSessionsOwner, setPrivateSessionsOwner] = useState("");
  const [privateSessionsLoading, setPrivateSessionsLoading] = useState(false);
  const privateSessionsRevision = useRef(0);
  const [sessionDialog, setSessionDialog] = useState<"new" | ChatSession | null>(null);
  const [deleteSession, setDeleteSession] = useState<ChatSession | null>(null);
  const [sessionSaving, setSessionSaving] = useState(false);
  const [sessionSelection, setSessionSelection] = useState<Record<string, string>>(() => {
    try { const value = JSON.parse(localStorage.getItem("ai-gent.selected-sessions.v1") ?? "{}"); return value && typeof value === "object" && !Array.isArray(value) ? value : {}; } catch { return {}; }
  });
  const [privateSessionSelection, setPrivateSessionSelection] = useState<Record<string, string>>(() => {
    try { const value = JSON.parse(localStorage.getItem("ai-gent.selected-private-sessions.v1") ?? "{}"); return value && typeof value === "object" && !Array.isArray(value) ? value : {}; } catch { return {}; }
  });
  const [groupTargets, setGroupTargets] = useState<Record<string, string>>({});
  const [selection, setSelection] = useState<Selection>({ kind: "personal" });
  const [mode, setMode] = useState<"chats" | "projects">("chats");
  const [rail, setRail] = useState("all");
  const [tab, setTab] = useState("chats");
  const [query, setQuery] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [deepseekProbe, setDeepseekProbe] = useState<Probe | null>(null);
  const [deepseekConnecting, setDeepseekConnecting] = useState(desktop);
  const [snapshot, setSnapshot] = useState<Snapshot>(emptySnapshot);
  const [privatePreview, setPrivatePreview] = useState("");
  const [connecting, setConnecting] = useState(desktop);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState("");
  const [groupDialog, setGroupDialog] = useState<"new" | Group | null>(null);
  const [settings, setSettings] = useState(false);
  const group = selection.kind === "group" ? groups.find(g => g.id === selection.id) : undefined;
  const groupSessions = sessionsOwner === group?.id ? sessions : [];
  const session = groupSessions.find(s => s.id === sessionSelection[group?.id ?? ""]) ?? groupSessions.find(s => !s.archived) ?? groupSessions[0];
  const inGroup = mode === "chats" && !!group && !groupHub;
  const project = selection.kind === "project" ? projects.find(p => p.id === selection.id) : projects.find(p => p.id === group?.project_id);
  const selectedAgent = selection.kind === "agent" ? selection.id : selection.kind === "deepseek" ? "deepseek" : group ? (group.members.includes(groupTargets[group.id] ?? "codex") ? groupTargets[group.id] ?? "codex" : group.members[0]) : hasCodexConnected ? "codex" : connectedAgents[0]?.id ?? "";
  const selectedExists = agents.some(agent => agent.id === selectedAgent);
  const inAgent = mode === "chats" && !groupHub && !group && selection.kind !== "project" && selectedExists;
  const agentSessions = privateSessionsOwner === selectedAgent ? privateSessions : [];
  const privateSession = agentSessions.find(s => s.id === privateSessionSelection[selectedAgent]) ?? agentSessions.find(s => !s.archived) ?? agentSessions[0];
  const activeSession: ChatSession | undefined = inGroup ? session : inAgent ? privateSession : undefined;
  useEffect(() => {
    if (agentsLoaded && selection.kind === "personal" && !hasCodexConnected && selectedAgent) setSelection(selectedAgent === "deepseek" ? {kind:"deepseek"} : {kind:"agent",id:selectedAgent});
  }, [agentsLoaded, hasCodexConnected, selectedAgent, selection.kind]);
  const scope: Scope = { projectId: selection.kind === "project" ? selection.id : null, groupId: selection.kind === "group" ? session?.id ?? selection.id : null, sessionId: inAgent ? privateSession?.id ?? null : null, agent: selectedAgent };
  const key = selection.kind === "group" ? session?.id === selection.id ? `group:${selection.id}` : `group:${selection.id}:${session?.id ?? "loading"}` : selection.kind === "project" ? `project:${selection.id}` : inAgent ? (privateSession ? (privateSession.is_default ? `private:${selectedAgent}` : `private:${selectedAgent}:${privateSession.id}`) : `private:${selectedAgent}:loading`) : selection.kind === "agent" ? `agent:${selection.id}` : selection.kind;
  const currentKey = useRef(key); currentKey.current = key;
  const pollingKey = useRef(key);
  const visibleSnapshot = pollingKey.current === key ? snapshot : emptySnapshot;
  const title = activeSession?.name ?? group?.name ?? (project ? `${project.name} · ${agentName(selectedAgent)}` : agentName(selectedAgent));
  function openGroup(id: string) { setSelection({ kind: "group", id }); setGroupHub(false); setMode("chats"); setTab("chats"); setRail("groups"); }
  function showGroups() { setGroupHub(true); setMode("chats"); setRail("groups"); }
  function selectSession(value: GroupSession) {
    const next = { ...sessionSelection, [value.group_id]: value.id };
    setSessionSelection(next);
    try { localStorage.setItem("ai-gent.selected-sessions.v1", JSON.stringify(next)); } catch { setError("会话选择未能保存，重启后需要重新选择。"); }
  }
  function selectPrivateSession(value: PrivateSession) {
    const next = { ...privateSessionSelection, [value.agent_id]: value.id };
    setPrivateSessionSelection(next);
    try { localStorage.setItem("ai-gent.selected-private-sessions.v1", JSON.stringify(next)); } catch { setError("会话选择未能保存，重启后需要重新选择。"); }
  }
  async function archiveSession(target: ChatSession | undefined = activeSession) {
    if (!target || sessionSaving) return;
    setSessionSaving(true);
    try {
      if ("group_id" in target) {
        if (!group) return;
        const updated = await saveGroupSession(target.group_id, target.name, target, !target.archived);
        sessionsRevision.current += 1;
        setSessions(current => current.map(s => s.id === updated.id ? updated : s));
        setGroups(await getGroups()); notify(updated.archived ? "会话已归档，记录仍保留" : "会话已恢复");
      } else {
        const updated = await savePrivateSession(target.agent_id, target.name, target, !target.archived);
        privateSessionsRevision.current += 1;
        setPrivateSessions(current => current.map(s => s.id === updated.id ? updated : s));
        notify(updated.archived ? "会话已归档，记录仍保留" : "会话已恢复");
      }
    } catch (cause) { setError(String(cause)); } finally { setSessionSaving(false); }
  }
  function sessionDeleted(deleted: ChatSession) {
    setDeleteSession(null);
    if ("group_id" in deleted) {
      sessionsRevision.current += 1;
      setSessions(current => current.filter(s => s.id !== deleted.id));
      if (session?.id === deleted.id) setSnapshot(emptySnapshot);
      const next = { ...sessionSelection };
      if (next[deleted.group_id] === deleted.id) delete next[deleted.group_id];
      setSessionSelection(next);
      setGroups(current => current.map(g => g.id === deleted.group_id ? { ...g, session_count: Math.max(0, (g.session_count ?? 1) - (deleted.archived ? 0 : 1)) } : g));
      try { discardSessionDraft(deleted); localStorage.setItem("ai-gent.selected-sessions.v1", JSON.stringify(next)); }
      catch { setError("会话记录已删除，但本机草稿或会话选择清理失败，请检查浏览器存储。"); }
    } else {
      privateSessionsRevision.current += 1;
      setPrivateSessions(current => deleted.is_default ? current.map(s => s.id === deleted.id ? { ...s, preview: null, updated_at: null, status: null } : s) : current.filter(s => s.id !== deleted.id));
      if (privateSession?.id === deleted.id) setSnapshot(emptySnapshot);
      const next = { ...privateSessionSelection };
      if (!deleted.is_default && next[deleted.agent_id] === deleted.id) delete next[deleted.agent_id];
      setPrivateSessionSelection(next);
      try { discardSessionDraft(deleted); localStorage.setItem("ai-gent.selected-private-sessions.v1", JSON.stringify(next)); }
      catch { setError("会话记录已删除，但本机草稿或会话选择清理失败，请检查浏览器存储。"); }
    }
    notify("会话已删除");
  }
  function groupDeleted(deleted: Group, deletedSessions: GroupSession[]) {
    setGroupDialog(null);
    setSessionDialog(null);
    setDeleteSession(null);
    setGroups(current => current.filter(g => g.id !== deleted.id));
    sessionsRevision.current += 1;
    setSessions([]); setSessionsOwner(""); setSessionsLoading(false);
    setSnapshot(emptySnapshot);
    const next = { ...sessionSelection };
    delete next[deleted.id];
    setSessionSelection(next);
    setGroupTargets(current => { const targets = { ...current }; delete targets[deleted.id]; return targets; });
    // Clear the parent draft even if the default session was deleted earlier.
    const parentSession: GroupSession = { ...deleted, group_id: deleted.id, archived: false };
    let cleanupFailed = false;
    for (const removed of [parentSession, ...deletedSessions]) {
      try { discardSessionDraft(removed); } catch { cleanupFailed = true; }
    }
    try { localStorage.setItem("ai-gent.selected-sessions.v1", JSON.stringify(next)); } catch { cleanupFailed = true; }
    if (cleanupFailed) setError("群已删除，但部分本机草稿或会话选择清理失败，请检查浏览器存储。");
    setSelection({ kind: "personal" });
    showGroups();
    notify(`已删除群「${deleted.name}」`);
    void getGroups().then(setGroups).catch(cause => setError(`群已删除，刷新群列表失败：${String(cause)}`));
  }
  function notify(message: string) { setToast(message); }
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(""), 3600); return () => clearTimeout(timer); }, [toast]);
  useEffect(() => {
    let active = true;
    void (async () => {
      const results = await Promise.allSettled([desktop ? listProjects() : Promise.resolve([]), desktop ? getAppInfo() : Promise.resolve(null), getGroups()]);
      if (!active) return;
      const [p, i, g] = results;
      if (p.status === "fulfilled") setProjects(p.value);
      if (i.status === "fulfilled") setInfo(i.value);
      if (g.status === "fulfilled") setGroups(g.value);
      const failure = results.find(result => result.status === "rejected");
      if (failure?.status === "rejected") setError(String(failure.reason));
      setLoading(false); setConnecting(false);
    })();
    return () => { active = false; };
  }, []);
  useEffect(() => {
    if (!desktop || !hasCodex) {setConnecting(false);setProbe(null);return;}
    let active=true;
    setConnecting(true);
    void probeCodex().then(result=>{if(active)setProbe(result);}).catch(cause=>{if(active)setError(`Codex 连接失败：${String(cause)}`);}).finally(()=>{if(active)setConnecting(false);});
    return()=>{active=false;};
  }, [hasCodex]);
  useEffect(() => {
    if (!desktop || !hasDeepseek) {setDeepseekConnecting(false);setDeepseekProbe(null);return;}
    let active=true;
    setDeepseekConnecting(true);
    void probeDeepSeek().then(result=>{if(active)setDeepseekProbe(result);}).catch(cause=>{if(active)setError(`DeepSeek 连接失败：${String(cause)}`);}).finally(()=>{if(active)setDeepseekConnecting(false);});
    return()=>{active=false;};
  }, [hasDeepseek]);
  useEffect(() => {
    if (group && session && sessionSelection[group.id] !== session.id) selectSession(session);
  }, [group?.id, session?.id]);
  useEffect(() => {
    if (inAgent && privateSession && privateSessionSelection[selectedAgent] !== privateSession.id) selectPrivateSession(privateSession);
  }, [inAgent, selectedAgent, privateSession?.id]);
  useEffect(() => {
    if (!group?.id) { setSessionsOwner(""); setSessions([]); setSessionsLoading(false); return; }
    const groupId = group.id;
    let active = true; let timer: ReturnType<typeof setTimeout>;
    setSessionsOwner(groupId); setSessions([]); setSessionsLoading(true);
    const refresh = async () => {
      const revision = sessionsRevision.current;
      try { const result = await getGroupSessions(groupId); if (active && revision === sessionsRevision.current) setSessions(result); }
      catch (cause) { if (active) setError(String(cause)); }
      finally { if (active) setSessionsLoading(false); }
      if (active) timer = setTimeout(refresh, 2500);
    };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, [group?.id]);
  useEffect(() => {
    if (!inAgent || !selectedAgent) { setPrivateSessionsOwner(""); setPrivateSessions([]); setPrivateSessionsLoading(false); return; }
    const agentId = selectedAgent;
    let active = true; let timer: ReturnType<typeof setTimeout>;
    setPrivateSessionsOwner(agentId); setPrivateSessions([]); setPrivateSessionsLoading(true);
    const refresh = async () => {
      const revision = privateSessionsRevision.current;
      try { const result = await getPrivateSessions(agentId); if (active && revision === privateSessionsRevision.current) setPrivateSessions(result); }
      catch (cause) { if (active) setError(String(cause)); }
      finally { if (active) setPrivateSessionsLoading(false); }
      if (active) timer = setTimeout(refresh, 2500);
    };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, [inAgent, selectedAgent]);
  useEffect(() => {
    if (!agentsLoaded || !selectedExists || (inAgent && !privateSession) || (inGroup && !session)) {setSnapshot(emptySnapshot);return;}
    let active = true; let timer: ReturnType<typeof setTimeout>;
    if (pollingKey.current !== key) { setSnapshot(emptySnapshot); pollingKey.current = key; }
    const refresh = async () => {
      try { const result = await getSnapshot(scope); if (active) { setSnapshot(result); if (selection.kind === "personal") setPrivatePreview(result.messages.filter(m => m.role !== "tool").at(-1)?.text ?? ""); } }
      catch (cause) { if (active) setError(String(cause)); }
      if (active) timer = setTimeout(refresh, 900);
    };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, [key, scope.agent, agentsLoaded, selectedExists, inAgent, privateSession?.id, inGroup, session?.id]);
  useEffect(() => {
    let active = true; let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => { try { const result = await getGroups(); if (active) setGroups(result); } catch (cause) { if (active) setError(String(cause)); } if (active) timer = setTimeout(refresh, 3500); };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, []);
  async function refreshCurrent() {
    const selected = key;
    const [result, list, conversations, privateList] = await Promise.all([getSnapshot(scope), getGroups(), group ? getGroupSessions(group.id) : Promise.resolve(null), inAgent ? getPrivateSessions(selectedAgent) : Promise.resolve(null)]);
    if (currentKey.current === selected) setSnapshot(result);
    if (currentKey.current === selected && conversations) setSessions(conversations);
    if (currentKey.current === selected && privateList) setPrivateSessions(privateList);
    setGroups(list); if (selection.kind === "personal") setPrivatePreview(result.messages.filter(m => m.role !== "tool").at(-1)?.text ?? "");
  }
  async function connect() {
    if (!desktop) { notify("请在桌面版中连接本机 Codex"); return; }
    setConnecting(true); setError(null);
    try { const result = await probeCodex(); setProbe(result); notify(`已连接 Codex · ${result.models.length} 个模型`); }
    catch (cause) { setError(String(cause)); } finally { setConnecting(false); }
  }
  async function connectDeepseek() {
    if (!desktop) { notify("请在桌面版中连接 DeepSeek Harness"); return; }
    setDeepseekConnecting(true); setError(null);
    try {
      const [result, saved] = await Promise.all([probeDeepSeek(), getSnapshot({ projectId: null, groupId: null, agent: "deepseek" })]);
      setDeepseekProbe(result);
      void saved;
      notify(`DeepSeek Harness 已就绪 · ${result.models.length} 个内置目录模型`);
    } catch (cause) { setDeepseekProbe(null); setError(String(cause)); } finally { setDeepseekConnecting(false); }
  }
  async function discoverLocalAgents(autoEnable = true, silent = false) {
    if (discovering) return;
    setDiscovering(true);
    setError(null);
    try {
      const results = await autoDiscoverAgents(autoEnable);
      setDiscoveryResults(results);
      const codexItem = results.find(item => item.provider_kind === "codex");
      const deepseekItem = results.find(item => item.provider_kind === "deepseek");
      if (desktop && codexItem?.status === "connected") {
        void probeCodex().then(setProbe).catch(() => {});
      } else if (codexItem?.status === "unavailable") {
        setProbe(null);
      }
      if (desktop && deepseekItem?.status === "connected") {
        void probeDeepSeek().then(setDeepseekProbe).catch(() => {});
      } else if (deepseekItem?.status === "unavailable") {
        setDeepseekProbe(null);
      }
      const connectedCount = results.filter(item => item.status === "connected").length;
      if (!silent) {
        notify(connectedCount > 0 ? `已自动连接 ${connectedCount} 个本机 Agent` : "未检测到可直接连接的本机 Agent");
      }
    } catch (cause) {
      setError(`本机 Agent 检测失败：${String(cause)}`);
    } finally {
      setDiscovering(false);
    }
  }
  useEffect(() => {
    if (settings && desktop) void discoverLocalAgents(true, true);
  }, [settings]);
  async function add() {
    if (!desktop) { notify("请在桌面版中选择本地项目目录"); return; }
    setAdding(true); setError(null);
    try { const path = await open({ directory: true, multiple: false, title: "选择项目目录" }); if (typeof path !== "string") return; const p = await addProject(path); setProjects(await listProjects()); setGroupHub(false); setSelection({ kind: "project", id: p.id }); setMode("projects"); }
    catch (cause) { setError(String(cause)); } finally { setAdding(false); }
  }
  function defaultPrivateSelection(): Selection {
    if (hasCodexConnected) return { kind: "personal" };
    const fallback = connectedAgents[0]?.id;
    return fallback === "deepseek" ? { kind: "deepseek" } : fallback ? { kind: "agent", id: fallback } : { kind: "personal" };
  }
  function showAllChats() {
    setGroupHub(false); setRail("all"); setMode("chats"); setTab("chats");
    if (selection.kind === "group" || selection.kind === "project") setSelection(defaultPrivateSelection());
  }
  function selectAgent(id: string) { setGroupHub(false); setRail(id); setMode("chats"); setTab("chats"); setSelection(id === "codex" ? {kind:"personal"} : id === "deepseek" ? {kind:"deepseek"} : {kind:"agent",id}); }
  const filteredGroups = groups.filter(g => g.name.toLowerCase().includes(query.toLowerCase()));
  return <div className="app-shell">
    <header className="app-topbar"><div className="brand"><img className="brand-mark" src={appIcon} alt="" /><span>AgentCrew<span className="brand-edition">桌面版</span><small>多端协同 · 智能体工作台</small></span></div><nav className="top-nav" aria-label="工作区"><button className={mode === "chats" ? "active" : ""} onClick={() => { setGroupHub(false); setMode("chats"); setTab("chats"); if (selection.kind === "project") setSelection(defaultPrivateSelection()); }}>对话</button><button className={mode === "projects" ? "active" : ""} onClick={() => { setGroupHub(false); setMode("projects"); if (projects[0]) setSelection({ kind: "project", id: projects[0].id }); }}>工作空间</button></nav><div className="topbar-end"><span className="local-badge"><Icon name="shield" size={14} />本机工作台</span><button className="profile-button" aria-label="工作台设置" onClick={() => setSettings(true)}>我</button></div></header>
    <div className={`workbench ${groupHub && mode === "chats" ? "hub-mode" : inGroup || inAgent ? "session-mode" : ""}`}><aside className="agent-rail" aria-label="成员导航"><button className={`rail-button ${!groupHub && rail === "all" ? "active" : ""}`} onClick={showAllChats}><span className="rail-icon"><Icon name="chat" size={23} /></span><span>对话</span></button><button className={`rail-button ${rail === "groups" ? "active" : ""}`} onClick={showGroups} aria-label="查看所有群聊"><span className="rail-icon"><Icon name="group" size={23} /></span><span>群聊</span></button><div className="rail-rule" />{connectedAgents.map(agent => <button className={`rail-button ${rail === agent.id ? "active" : ""}`} key={agent.id} onClick={() => selectAgent(agent.id)} title={`${agent.name} 私聊`}><span className="rail-avatar"><Avatar agent={agent.id} /><i className={agent.availability === "ready" ? "ready" : ""} /></span><span>{agent.name}</span></button>)}<button className="rail-button add-agent" onClick={() => setPairing(true)}><span className="rail-icon dashed"><Icon name="plus" size={23} /></span><span>接入</span></button><div className="rail-spacer" /><button className="rail-button" onClick={() => setSettings(true)}><span className="rail-icon"><Icon name="settings" size={21} /></span><span>设置</span></button><span className="rail-version">v{info?.version ?? "0.1.0"}</span></aside>
    {groupHub && mode === "chats" ? <main className="hub-workspace">{error && <div className="global-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError(null)}><Icon name="close" size={16} /></button></div>}{loading ? <div className="loading-conversation">正在读取群聊…</div> : <GroupHub groups={groups} projects={projects} onSelect={g => openGroup(g.id)} onCreate={() => setGroupDialog("new")} />}</main> : <>{inGroup ? <aside className="conversation-sidebar group-sidebar" aria-label="群聊"><div className="sidebar-profile"><div><strong>我的工作台</strong><small>和自己的 Agent 一起工作</small></div></div><div className="group-sidebar-heading"><span>协作群</span><button onClick={showGroups} title="查看所有群聊" aria-label="查看所有群聊">{groups.length}<Icon name="right" size={12} /></button></div><div className="group-sidebar-list">{groups.map(g => <button className={`group-sidebar-row ${g.id === group?.id ? "active" : ""}`} key={g.id} onClick={() => openGroup(g.id)}><Avatar agent="group" /><span className="group-sidebar-copy"><strong title={g.name}>{g.name}</strong><small>{g.project_id ? `关联项目 · ${projects.find(p => p.id === g.project_id)?.name ?? "本地项目"}` : "未关联项目 · 群专用目录"}</small></span><span className="group-sidebar-count">{g.session_count ?? 1}</span></button>)}</div><div className="group-sidebar-heading"><span>群成员</span></div><div className="group-sidebar-members">{group?.members.map(id => <Avatar agent={id} small key={id} />)}<Avatar agent="me" small /></div><div className="sidebar-footer"><span className="presence-dot connected" /><span>{desktop ? "记录保存在本机" : "浏览器预览"}</span></div></aside> : <aside className="conversation-sidebar" aria-label="会话导航"><div className="sidebar-profile"><Avatar agent="me" small /><div><strong>我的工作台</strong><small>和自己的 Agent 一起工作</small></div><button className="icon-button" aria-label="工作台连接设置" onClick={() => setSettings(true)}><Icon name="settings" size={17} /></button></div>
      <label className="sidebar-search"><Icon name="search" size={17} /><input aria-label={mode === "projects" ? "搜索项目" : "搜索会话"} placeholder={mode === "projects" ? "搜索项目" : tab === "contacts" ? "搜索成员" : "搜索会话或 Agent…"} value={query} onChange={e => setQuery(e.target.value)} /><kbd>⌕</kbd></label>
      <div className="conversation-list">{mode === "projects" ? <><div className="list-heading"><span>本地项目</span><span>{projects.length}</span></div>{projects.filter(p => p.name.toLowerCase().includes(query.toLowerCase())).map(p => <button className={`conversation-item ${selection.kind === "project" && selection.id === p.id ? "selected" : ""}`} key={p.id} onClick={() => setSelection({ kind: "project", id: p.id })}><span className="folder-avatar"><Icon name="folder" size={21} /></span><span className="conversation-text"><strong>{p.name}</strong><small title={p.primary_root}>{p.primary_root}</small></span></button>)}{!projects.length && <div className="sidebar-empty"><Icon name="folder" size={30} /><strong>给任务一个工作目录。</strong><p>添加项目后，可围绕项目文件继续聊。</p></div>}<button className="new-group-button" onClick={add} disabled={adding}><Icon name="plus" size={18} />{adding ? "正在添加…" : "添加本地项目"}</button></> : tab === "contacts" ? <><div className="list-heading"><span className="heading-tabs" role="tablist" aria-label="会话与通讯录"><button role="tab" aria-selected={tab !== "contacts"} onClick={() => setTab("chats")}>私聊</button><button role="tab" aria-selected={tab === "contacts"} onClick={() => setTab("contacts")}>通讯录</button></span><span>{agents.length}</span></div>{agents.filter(a => a.name.toLowerCase().includes(query.toLowerCase())).map(a => <button className="contact-item" onClick={() => selectAgent(a.id)} key={a.id}><Avatar agent={a.id} /><span><strong>{a.name}</strong><small>{a.connected ? "已连接 · 已显示在侧边栏" : a.available ? "已接入 · 等待连接就绪" : a.registration_state === "revoked" ? "已移除 · 历史记录保留" : a.registration_state === "active" ? "已停用 · 历史记录保留" : "尚未接入 · 请完成配对检查"}</small></span><Icon name="right" size={15} /></button>)}<div className="contacts-note"><Icon name="shield" size={16} />这里是你本机的成员。</div></> : <><div className="list-heading"><span className="heading-tabs" role="tablist" aria-label="会话与通讯录"><button role="tab" aria-selected={tab !== "contacts"} onClick={() => setTab("chats")}>私聊</button><button role="tab" aria-selected={tab === "contacts"} onClick={() => setTab("contacts")}>通讯录</button></span><span>{connectedAgents.filter(agent => rail === "all" || agent.id === rail).length}</span></div>{hasCodexConnected && rail !== "deepseek" && "codex".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "personal" ? "selected" : ""}`} onClick={() => setSelection({ kind: "personal" })}><Avatar /><span className="conversation-text"><span className="conversation-name"><strong>Codex</strong>{connecting ? <span className="small-spinner" /> : <span className="connected-dot" />}</span><small>{privatePreview || "独立私聊，无需选择项目"}</small></span></button>}{hasDeepseekConnected && rail !== "codex" && "deepseek".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "deepseek" ? "selected" : ""}`} onClick={() => setSelection({ kind: "deepseek" })}><Avatar agent="deepseek" /><span className="conversation-text"><span className="conversation-name"><strong>DeepSeek</strong>{deepseekConnecting ? <span className="small-spinner" /> : <span className="connected-dot" />}</span><small>Harness 独立私聊</small></span></button>}
      {connectedAgents.filter(a => a.id !== "codex" && a.id !== "deepseek" && (rail === "all" || rail === a.id) && a.name.toLowerCase().includes(query.toLowerCase())).map(a => <button className={`conversation-item ${selection.kind === "agent" && selection.id === a.id ? "selected" : ""}`} key={a.id} onClick={() => selectAgent(a.id)}><Avatar agent={a.id}/><span className="conversation-text"><span className="conversation-name"><strong>{a.name}</strong><span className="connected-dot" /></span><small>{a.description}</small></span></button>)}<div className="list-heading group-list-heading"><span>协作群</span><span>{filteredGroups.length}</span></div>{filteredGroups.map(g => <button className={`conversation-item ${selection.kind === "group" && selection.id === g.id ? "selected" : ""}`} key={g.id} onClick={() => openGroup(g.id)}><Avatar agent="group" /><span className="conversation-text"><span className="conversation-name"><strong title={g.name}>{g.name}</strong><time>{clock(g.updated_at)}</time></span><small>{running(g.status ?? "") ? "Agent 正在处理任务…" : g.preview ?? `${g.members.length} 位 Agent · 你`}</small></span></button>)}{!filteredGroups.length && <p className="no-groups">{query ? "没有找到匹配的群聊" : "让不同的想法，在一个群里碰面。"}</p>}</>}</div>
      <div className="sidebar-footer"><span className={`presence-dot ${desktop ? "connected" : ""}`} /><span>{desktop ? "记录保存在本机" : "浏览器预览"}</span><button className="icon-button" title="数据与连接" aria-label="查看数据与连接" onClick={() => setSettings(true)}><Icon name="shield" size={15} /></button></div>
    </aside>}{inGroup && group ? <SessionSidebar key={group.id} sessions={groupSessions} selected={session} loading={sessionsLoading} saving={sessionSaving} onRename={s => setSessionDialog(s)} onArchive={archiveSession} onDelete={setDeleteSession} onSelect={selectSession} onCreate={() => setSessionDialog("new")} /> : inAgent && <SessionSidebar key={`agent:${selectedAgent}`} sessions={agentSessions} selected={privateSession} loading={privateSessionsLoading} saving={sessionSaving} searchPlaceholder={`搜索 ${agentName(selectedAgent)} 会话`} bottomNote="会话独立 · 上下文隔离" onRename={s => setSessionDialog(s)} onArchive={archiveSession} onDelete={setDeleteSession} onSelect={selectPrivateSession} onCreate={() => setSessionDialog("new")} />}<main className="conversation-workspace">{!desktop && <div className="preview-notice">浏览器预览<span>Agent 执行与项目目录选择请使用桌面版。</span></div>}{error && <div className="global-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError(null)}><Icon name="close" size={16} /></button></div>}{loading || !agentsLoaded || (!selectedExists && (checkingAgents || discovering)) || (inGroup && (sessionsLoading || !session)) || (inAgent && (privateSessionsLoading || !privateSession)) ? <div className="loading-conversation"><span className="small-spinner" />{inGroup || inAgent ? (sessionsLoading || privateSessionsLoading) ? "正在读取会话…" : "请选择或新建会话" : checkingAgents || discovering ? "正在连接本机已安装的 Agent…" : "正在打开你的工作台…"}</div> : !selectedExists ? <div className="conversation-welcome"><div className="welcome-mark"><Icon name="spark" size={37}/></div><h2>还没有已连接的 Agent</h2><p>在设置中自动检测并连接本机已安装的 Agent，或手动添加一位 Agent 后开始对话。</p><div className="pairing-actions"><button className="primary" disabled={discovering} onClick={()=>void discoverLocalAgents(true)}>{discovering ? "正在检测本机 Agent…" : "自动检测并连接"}</button><button className="outline-button" onClick={()=>setPairing(true)}><Icon name="plus" size={16}/>手动添加 Agent</button></div></div> : <ConnectedChat session={activeSession} onGroups={showGroups} onRenameSession={() => activeSession && setSessionDialog(activeSession)} onArchiveSession={() => archiveSession()} onDeleteSession={() => activeSession && setDeleteSession(activeSession)} onGroupUpdated={updated => setGroups(current => current.map(g => g.id === updated.id ? updated : g))} sessionSaving={sessionSaving} key={key} scope={scope} scopeKey={key} title={title} group={group} project={project} agent={selectedAgent} onAgent={agent => { if (group) setGroupTargets(current => ({ ...current, [group.id]: agent })); }} snapshot={visibleSnapshot} legacyProbes={{codex:probe,deepseek:deepseekProbe}} legacyConnecting={{codex:connecting,deepseek:deepseekConnecting}} onConnectLegacy={{codex:connect,deepseek:connectDeepseek}} onInvite={() => group && setGroupDialog(group)} onSettings={() => setSettings(true)} onRefresh={refreshCurrent} notify={notify} />}</main></>}</div>
    {groupDialog && <GroupDialog key={groupDialog === "new" ? "new" : groupDialog.id} existing={groupDialog === "new" ? undefined : groupDialog} projects={projects} onClose={() => setGroupDialog(null)} onDeleted={groupDeleted} onSaved={saved => { setGroups(current => [saved, ...current.filter(g => g.id !== saved.id)]); openGroup(saved.id); setGroupDialog(null); notify(groupDialog === "new" ? `已创建「${saved.name}」` : "群设置已保存"); }} />}
    {sessionDialog && (group || inAgent) && <SessionDialog key={sessionDialog === "new" ? `${group?.id ?? selectedAgent}:new` : sessionDialog.id} groupId={group?.id} agentId={group ? undefined : selectedAgent} existing={sessionDialog === "new" ? undefined : sessionDialog} onClose={() => setSessionDialog(null)} onSaved={saved => { if ("group_id" in saved) { sessionsRevision.current += 1; setSessions(current => [saved, ...current.filter(s => s.id !== saved.id)]); selectSession(saved); setGroups(current => current.map(g => g.id === saved.group_id ? { ...g, session_count: sessionDialog === "new" ? (g.session_count ?? 1) + 1 : g.session_count } : g)); } else { privateSessionsRevision.current += 1; setPrivateSessions(current => [saved, ...current.filter(s => s.id !== saved.id)]); selectPrivateSession(saved); } setSessionDialog(null); notify(sessionDialog === "new" ? "已创建会话" : "会话名称已更新"); }} />}
    {deleteSession && <DeleteSessionDialog session={deleteSession} onClose={() => setDeleteSession(null)} onDeleted={sessionDeleted} />}
    {pairing && <PairingDialog invitation={invitation} onInvitation={setInvitation} onClose={()=>setPairing(false)}/> }
    {settings && <Dialog title="成员接入与设置" onClose={() => setSettings(false)} wide><p className="dialog-intro">自动检测并连接本机已安装的 Agent，连接成功后将自动显示在侧边栏；也可手动添加自定义 Agent。</p><div className="pairing-actions"><button className="primary" disabled={discovering || !desktop} onClick={() => void discoverLocalAgents(true)}>{discovering ? "正在检测本机 Agent…" : "自动检测并连接本机 Agent"}</button><button className="outline-button" onClick={()=>{setSettings(false);setPairing(true);}}>添加 Agent</button><button className="outline-button" onClick={()=>void toggleDispatch()}>{paused?"继续全部派发":"暂停全部派发"}</button></div><div className="field-caption"><span>本机自动检测结果</span></div><div className="discovery-list" role="group" aria-label="本机自动检测结果">{discovering && !discoveryResults.length ? <div className="discovery-empty">正在静默检测本机已安装的 Agent…</div> : discoveryResults.length ? discoveryResults.map(item => <div className="discovery-item" key={item.provider_kind}><Avatar agent={item.provider_kind} /><div><strong>{item.display_name}</strong><p>{item.message}{item.version ? ` · v${item.version}` : ""}</p></div><span className={`discovery-status ${item.status}`}>{item.status === "connected" ? "已自动连接" : item.status === "needs_auth" ? "需原厂登录" : "未检测到"}</span></div>) : <div className="discovery-empty">点击上方「自动检测并连接本机 Agent」扫描本机已安装的组件。</div>}</div><div className="field-caption"><span>Agent 成员表</span></div><div className="settings-registry" role="group" aria-label="Agent 成员表">{agents.filter(a=>a.registration_state!=="revoked").map(a=>{const isCodex=a.id==="codex",isDeepseek=a.id==="deepseek";const detail=a.registration_state!=="active"?(a.registration_state==="validating"?"正在检查":"接入失败"):isCodex?(probe?`${probe.auth ?? "本机配置"} · ${probe.models.length} 个模型`:connecting?"正在检测…":a.description):isDeepseek?(deepseekProbe?`Harness · ${deepseekProbe.models.length} 个模型`:deepseekConnecting?"正在连接…":a.description):a.description;return <div className="connection-card" key={a.id}><Avatar agent={a.id}/><div><strong>{a.name}{a.connected && <span className="sidebar-visible-badge">侧边栏已显示</span>}</strong><p>{detail}</p></div>{a.registration_state==="active" && isCodex && <button className="outline-button" disabled={connecting || !desktop} onClick={connect}>{connecting?"连接中…":probe?"重新检测":"连接"}</button>}{a.registration_state==="active" && isDeepseek && <button className="outline-button" disabled={deepseekConnecting || !desktop} onClick={connectDeepseek}>{deepseekConnecting?"连接中…":deepseekProbe?"重新检测":"连接"}</button>}{a.registration_state==="active" && <button className="outline-button" onClick={()=>void invoke("agent_update",{agent:a.id,enabled:!a.enabled,revoke:false}).then(refreshAgents).catch(cause=>setError(String(cause)))}>{a.enabled?"停用":"启用"}</button>}<button className="outline-button" onClick={()=>void invoke("agent_update",{agent:a.id,enabled:false,revoke:true}).then(refreshAgents).catch(cause=>setError(String(cause)))}>移除</button></div>;})}</div>{(probe || deepseekProbe || info) && <details className="runtime-details"><summary>运行环境与本地数据位置</summary>{probe && <code>Codex ({probe.version}): {probe.executable}</code>}{deepseekProbe && <code>DeepSeek (Harness {deepseekProbe.version}): {deepseekProbe.executable}</code>}{info && <code>DB: {info.database_path}</code>}</details>}<div className="dialog-footer"><span>AgentCrew · v{info?.version ?? "0.1.0"}</span><button className="primary" onClick={() => setSettings(false)}>完成<Icon name="check" size={16} /></button></div></Dialog>}
    {toast && <div className="toast" role="status"><Icon name="check" size={16} />{toast}</div>}
  </div>;
}
