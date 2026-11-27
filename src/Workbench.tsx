import { invoke } from "@tauri-apps/api/core";
import ConnectedChat from "./features/chats/ConnectedChat";
import PairingDialog, { type Invitation } from "./features/chats/PairingDialog";
import SettingsDialog, { type SettingsTab } from "./features/chats/SettingsDialog";
import { useCallback, useEffect, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { addProject, getAppInfo, listProjects, type AppInfo, type Project } from "./features/projects/api";
import { useAgents, loadAgents, refreshAgents, autoDiscoverAgents, agentName, clock, desktop, emptySnapshot, getGroups, getGroupSessions, getPrivateSessions, getProjectSessions, getSnapshot, probeAgent, running, saveGroupSession, savePrivateSession, saveProjectSession, type Agent, type DiscoveryItem, type Group, type GroupSession, type PrivateSession, type ProjectSession, type Probe, type Scope, type Snapshot } from "./features/chats/api";
import { applyStreamDeltas, subscribeRealtimeEvents, type CoalescedSync, type StreamDelta } from "./features/chats/events";
import { discardSessionDraft } from "./features/chats/ChatStage";
import GroupDialog from "./features/chats/GroupDialog";
import { GroupSidebarList, GroupHub, SessionSidebar, SessionDialog, DeleteSessionDialog, type ChatSession } from "./features/chats/GroupSessions";
import Icon from "./components/Icon";
import Dialog from "./components/Dialog";
import Avatar from "./components/Avatar";
import appIcon from "./assets/app-icon.png";
import { emptyUsageDashboard, getUsageDashboard, type UsageDashboardData } from "./features/usage/api";
import GitPanel from "./features/git/GitPanel";
import ProjectList from "./features/projects/ProjectList";
import SchedulesPanel from "./features/schedules/SchedulesPanel";
import { listScheduleRuns, listSchedules, type ScheduledTask, type ScheduledTaskRun } from "./features/schedules/api";

type Selection = { kind: "agent"; id: string } | { kind: "personal" } | { kind: "deepseek" } | { kind: "group"; id: string } | { kind: "project"; id: string };
type DraftTarget = { kind: "group"; groupId: string } | { kind: "project"; projectId: string } | { kind: "agent"; agentId: string };
export default function Workbench() {
  const agents = useAgents();
  const [agentsLoaded, setAgentsLoaded] = useState(!desktop);
  const connectedAgents = agents.filter(agent => agent.connected);
  const checkingAgents = agents.some(agent => agent.available && agent.availability === "checking");
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
    let active = true;
    void invoke<boolean>("dispatch_snapshot")
      .then(value => { if (active) setPaused(value); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
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
  const [projectSessions, setProjectSessions] = useState<ProjectSession[]>([]);
  const [projectSessionsOwner, setProjectSessionsOwner] = useState("");
  const [projectSessionsLoading, setProjectSessionsLoading] = useState(false);
  const projectSessionsRevision = useRef(0);
  const [sessionDialog, setSessionDialog] = useState<ChatSession | null>(null);
  const [draftTarget, setDraftTarget] = useState<DraftTarget | null>(null);
  const [draftProjectAgents, setDraftProjectAgents] = useState<Record<string, string>>({});
  const [pendingProjectPath, setPendingProjectPath] = useState<string | null>(null);
  const [pendingProjectName, setPendingProjectName] = useState("");
  const [pendingProjectError, setPendingProjectError] = useState<string | null>(null);
  const [deleteSession, setDeleteSession] = useState<ChatSession | null>(null);
  const [sessionSaving, setSessionSaving] = useState(false);
  const [sessionSelection, setSessionSelection] = useState<Record<string, string>>(() => {
    try { const value = JSON.parse(localStorage.getItem("ai-gent.selected-sessions.v1") ?? "{}"); return value && typeof value === "object" && !Array.isArray(value) ? value : {}; } catch { return {}; }
  });
  const [privateSessionSelection, setPrivateSessionSelection] = useState<Record<string, string>>(() => {
    try { const value = JSON.parse(localStorage.getItem("ai-gent.selected-private-sessions.v1") ?? "{}"); return value && typeof value === "object" && !Array.isArray(value) ? value : {}; } catch { return {}; }
  });
  const [projectSessionSelection, setProjectSessionSelection] = useState<Record<string, string>>(() => {
    try { const value = JSON.parse(localStorage.getItem("ai-gent.selected-project-sessions.v1") ?? "{}"); return value && typeof value === "object" && !Array.isArray(value) ? value : {}; } catch { return {}; }
  });
  const [groupTargets, setGroupTargets] = useState<Record<string, string>>({});
  const [projectAgents, setProjectAgents] = useState<Record<string, string>>(() => {
    try {
      const value = JSON.parse(localStorage.getItem("ai-gent.project-agents.v1") ?? "{}");
      return value && typeof value === "object" && !Array.isArray(value) ? value : {};
    } catch { return {}; }
  });
  const [selection, setSelection] = useState<Selection>({ kind: "personal" });
  const lastProjectIdRef = useRef<string | null>((() => {
    try { return localStorage.getItem("ai-gent.last-project-id.v1"); } catch { return null; }
  })());
  useEffect(() => {
    if (selection.kind === "project") {
      lastProjectIdRef.current = selection.id;
      try { localStorage.setItem("ai-gent.last-project-id.v1", selection.id); } catch {}
    }
  }, [selection]);
  const [mode, setMode] = useState<"chats" | "projects">("chats");
  const [rail, setRail] = useState("all");
  const [tab, setTab] = useState("chats");
  const [query, setQuery] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [probesByAgent, setProbesByAgent] = useState<Record<string, Probe>>(() => {
    try {
      const raw = JSON.parse(localStorage.getItem("ai-gent.probe-cache.v1") ?? "{}");
      return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
    } catch {
      return {};
    }
  });
  const [probingByAgent, setProbingByAgent] = useState<Record<string, boolean>>({});
  const autoProbedRef = useRef(new Set<string>());
  const connecting = !!probingByAgent.codex;
  const deepseekConnecting = !!probingByAgent.deepseek;
  const [snapshot, setSnapshot] = useState<Snapshot>(emptySnapshot);
  const [privatePreview, setPrivatePreview] = useState("");
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState("");
  const [groupDialog, setGroupDialog] = useState<"new" | Group | null>(null);
  const [groupDialogDelete, setGroupDialogDelete] = useState(false);
  function openGroupDialog(target: Group, deleting = false) { setGroupDialogDelete(deleting); setGroupDialog(target); }
  const [settings, setSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("agents");
  function openSettings(tab: SettingsTab = "agents") {
    setSettingsTab(tab);
    setSettings(true);
    if (tab === "usage") void refreshUsage();
  }
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    try {
      const saved = localStorage.getItem("ai-gent.theme.v1");
      if (saved === "dark" || saved === "light") return saved;
    } catch {}
    return "light";
  });
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem("ai-gent.theme.v1", theme); } catch {}
  }, [theme]);
  const [gitOpen, setGitOpen] = useState(false);
  const [schedulesOpen, setSchedulesOpen] = useState(false);
  const [scheduledTasks, setScheduledTasks] = useState<ScheduledTask[]>([]);
  const [scheduledRuns, setScheduledRuns] = useState<ScheduledTaskRun[]>([]);
  const [schedulesLoading, setSchedulesLoading] = useState(false);
  const [gitPath, setGitPath] = useState<string | undefined>(undefined);
  const [usageDays, setUsageDays] = useState(7);
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageData, setUsageData] = useState<UsageDashboardData>(emptyUsageDashboard);
  const [sidebarLayout, setSidebarLayout] = useState<{ workspaceCollapsed: boolean; sessionsCollapsed: boolean }>(() => {
    try {
      const raw = localStorage.getItem("ai-gent.sidebar-layout.v1");
      if (raw) {
        const saved = JSON.parse(raw);
        return {
          workspaceCollapsed: Boolean(saved?.workspaceCollapsed),
          sessionsCollapsed: Boolean(saved?.sessionsCollapsed),
        };
      }
    } catch {}
    return {
      workspaceCollapsed: typeof window !== "undefined" && window.innerWidth < 980,
      sessionsCollapsed: typeof window !== "undefined" && window.innerWidth < 720,
    };
  });
  const savedBeforeCollapseRef = useRef<{ workspaceCollapsed: boolean; sessionsCollapsed: boolean } | null>(null);
  function updateSidebarLayout(updater: (prev: { workspaceCollapsed: boolean; sessionsCollapsed: boolean }) => { workspaceCollapsed: boolean; sessionsCollapsed: boolean }, persist = true) {
    setSidebarLayout(prev => {
      const next = updater(prev);
      if (persist) {
        try { localStorage.setItem("ai-gent.sidebar-layout.v1", JSON.stringify(next)); } catch {}
      }
      return next;
    });
  }
  const toggleWorkspaceSidebar = (collapsed?: boolean) =>
    updateSidebarLayout(prev => ({ ...prev, workspaceCollapsed: collapsed ?? !prev.workspaceCollapsed }));
  const toggleSessionsSidebar = (collapsed?: boolean) =>
    updateSidebarLayout(prev => ({ ...prev, sessionsCollapsed: collapsed ?? !prev.sessionsCollapsed }));
  const [sidebarWidths, setSidebarWidths] = useState<{ workspace: number | null; session: number | null }>(() => {
    try {
      const raw = localStorage.getItem("ai-gent.sidebar-widths.v1");
      if (raw) {
        const saved = JSON.parse(raw);
        const w = typeof saved?.workspace === "number" && Number.isFinite(saved.workspace) ? Math.round(saved.workspace) : null;
        const s = typeof saved?.session === "number" && Number.isFinite(saved.session) ? Math.round(saved.session) : null;
        return {
          workspace: w ? Math.max(176, Math.min(420, w)) : null,
          session: s ? Math.max(196, Math.min(440, s)) : null,
        };
      }
    } catch {}
    return { workspace: null, session: null };
  });
  const [resizingSidebar, setResizingSidebar] = useState<"workspace" | "session" | null>(null);
  const persistSidebarWidths = useCallback((next: { workspace: number | null; session: number | null }) => {
    try { localStorage.setItem("ai-gent.sidebar-widths.v1", JSON.stringify(next)); } catch {}
  }, []);
  const startSidebarResize = useCallback((which: "workspace" | "session", event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const sidebarEl = event.currentTarget.parentElement;
    if (!sidebarEl) return;
    const startX = event.clientX;
    const startWidth = sidebarEl.getBoundingClientRect().width;
    const minWidth = which === "workspace" ? 176 : 196;
    const baseMax = which === "workspace" ? 420 : 440;
    const otherWidth = which === "workspace"
      ? (document.querySelector(".group-session-sidebar")?.getBoundingClientRect().width ?? 0)
      : (document.querySelector(".conversation-sidebar")?.getBoundingClientRect().width ?? 0);
    const railWidth = document.querySelector(".agent-rail")?.getBoundingClientRect().width ?? 64;
    const dynamicMax = Math.max(minWidth, Math.min(baseMax, window.innerWidth - railWidth - otherWidth - 340));

    setResizingSidebar(which);
    const prevCursor = document.body.style.cursor;
    const prevSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    let latestWidth = Math.round(startWidth);
    const onMove = (moveEvent: MouseEvent) => {
      const nextWidth = Math.round(Math.max(minWidth, Math.min(dynamicMax, startWidth + (moveEvent.clientX - startX))));
      latestWidth = nextWidth;
      setSidebarWidths(prev => ({ ...prev, [which]: nextWidth }));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.cursor = prevCursor;
      document.body.style.userSelect = prevSelect;
      setResizingSidebar(null);
      setSidebarWidths(prev => {
        const next = { ...prev, [which]: latestWidth };
        persistSidebarWidths(next);
        return next;
      });
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, [persistSidebarWidths]);
  const resetSidebarWidth = useCallback((which: "workspace" | "session") => {
    setSidebarWidths(prev => {
      const next = { ...prev, [which]: null };
      persistSidebarWidths(next);
      return next;
    });
  }, [persistSidebarWidths]);
  async function refreshUsage(days = usageDays) {
    if (!desktop) return;
    setUsageLoading(true);
    try { setUsageData(await getUsageDashboard(days)); }
    catch (cause) { setError(`读取额度与用量失败：${String(cause)}`); }
    finally { setUsageLoading(false); }
  }
  async function refreshSchedules() {
    if (!desktop) return;
    setSchedulesLoading(true);
    try {
      const [taskList, runList] = await Promise.all([listSchedules(), listScheduleRuns(undefined, 80)]);
      setScheduledTasks(taskList);
      setScheduledRuns(runList);
    } catch (cause) {
      setError(`读取定时任务失败：${String(cause)}`);
    } finally {
      setSchedulesLoading(false);
    }
  }
  useEffect(() => {
    if (!desktop) return;
    void refreshSchedules();
  }, []);
  useEffect(() => {
    if (!desktop) return;
    void refreshUsage(usageDays);
    const timer = setInterval(() => void refreshUsage(usageDays), 60_000);
    return () => clearInterval(timer);
  }, [usageDays]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "u") {
        e.preventDefault();
        setSettings(prev => {
          if (prev && settingsTab === "usage") return false;
          setSettingsTab("usage");
          void refreshUsage();
          return true;
        });
      } else if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "g") {
        e.preventDefault();
        setSchedulesOpen(false);
        setGitOpen(prev => !prev);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsTab]);
  const group = selection.kind === "group" ? groups.find(g => g.id === selection.id) : undefined;
  const groupSessions = sessionsOwner === group?.id ? sessions : [];
  const session = groupSessions.find(s => s.id === sessionSelection[group?.id ?? ""]) ?? groupSessions.find(s => !s.archived && !s.is_scheduled) ?? groupSessions.find(s => !s.archived) ?? groupSessions[0];
  const inGroup = mode === "chats" && !!group && !groupHub;
  const project = selection.kind === "project" ? projects.find(p => p.id === selection.id) : projects.find(p => p.id === group?.project_id);
  const inProject = mode === "projects" && selection.kind === "project" && !!project;
  const isGroupDraft = inGroup && !!group && draftTarget?.kind === "group" && draftTarget.groupId === group.id;
  const isProjectDraft = inProject && !!project && draftTarget?.kind === "project" && draftTarget.projectId === project.id;
  const currentProjectSessions = projectSessionsOwner === project?.id ? projectSessions : [];
  const projectSession = currentProjectSessions.find(s => s.id === projectSessionSelection[project?.id ?? ""]) ?? currentProjectSessions.find(s => !s.archived && !s.is_scheduled) ?? currentProjectSessions.find(s => !s.archived) ?? currentProjectSessions[0];
  const projectAgentKey = selection.kind === "project" && projectSession ? `${selection.id}:${projectSession.id}` : "";
  const savedProjectAgent = isProjectDraft && project ? draftProjectAgents[project.id] : projectAgents[projectAgentKey];
  const defaultAgent = hasCodexConnected ? "codex" : connectedAgents[0]?.id ?? "";
  const selectedAgent = selection.kind === "agent" ? selection.id : selection.kind === "deepseek" ? "deepseek" : group ? (group.members.includes(groupTargets[group.id] ?? "codex") ? groupTargets[group.id] ?? "codex" : group.members[0]) : selection.kind === "project" && agents.some(a => a.id === savedProjectAgent && a.available) ? savedProjectAgent : defaultAgent;
  const selectedExists = agents.some(agent => agent.id === selectedAgent);
  const inAgent = mode === "chats" && !groupHub && !group && selection.kind !== "project" && selectedExists;
  const isAgentDraft = inAgent && !!selectedAgent && draftTarget?.kind === "agent" && draftTarget.agentId === selectedAgent;
  const isDraft = isGroupDraft || isProjectDraft || isAgentDraft;
  const hasSessions = !gitOpen && !schedulesOpen && !(groupHub && mode === "chats") && (inGroup || inAgent || inProject);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b") {
        if (gitOpen || schedulesOpen || (groupHub && mode === "chats")) return;
        e.preventDefault();
        updateSidebarLayout(prev => {
          const allCollapsed = prev.workspaceCollapsed && (!hasSessions || prev.sessionsCollapsed);
          if (allCollapsed) {
            const restored = savedBeforeCollapseRef.current;
            savedBeforeCollapseRef.current = null;
            if (restored && (!restored.workspaceCollapsed || (hasSessions && !restored.sessionsCollapsed))) {
              return restored;
            }
            return { workspaceCollapsed: false, sessionsCollapsed: false };
          }
          savedBeforeCollapseRef.current = prev;
          return { workspaceCollapsed: true, sessionsCollapsed: hasSessions ? true : prev.sessionsCollapsed };
        });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [gitOpen, schedulesOpen, groupHub, mode, hasSessions]);
  useEffect(() => {
    let lastWidth = window.innerWidth;
    const onResize = () => {
      const width = window.innerWidth;
      if (lastWidth >= 980 && width < 980) {
        updateSidebarLayout(prev => (prev.workspaceCollapsed ? prev : { ...prev, workspaceCollapsed: true }), false);
      }
      if (lastWidth >= 720 && width < 720) {
        updateSidebarLayout(prev => (prev.sessionsCollapsed ? prev : { ...prev, sessionsCollapsed: true }), false);
      }
      lastWidth = width;
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  const agentSessions = privateSessionsOwner === selectedAgent ? privateSessions : [];
  const privateSession = agentSessions.find(s => s.id === privateSessionSelection[selectedAgent]) ?? agentSessions.find(s => !s.archived && !s.is_scheduled) ?? agentSessions.find(s => !s.archived) ?? agentSessions[0];
  const activeSession: ChatSession | undefined = isDraft ? undefined : (inGroup ? session : inAgent ? privateSession : inProject ? projectSession : undefined);
  useEffect(() => {
    if (agentsLoaded && selection.kind === "personal" && !hasCodexConnected && selectedAgent) setSelection(selectedAgent === "deepseek" ? {kind:"deepseek"} : {kind:"agent",id:selectedAgent});
  }, [agentsLoaded, hasCodexConnected, selectedAgent, selection.kind]);
  const scope: Scope = { projectId: selection.kind === "project" ? selection.id : null, groupId: selection.kind === "group" ? (isGroupDraft ? selection.id : session?.id ?? selection.id) : null, sessionId: isDraft ? null : (inAgent ? privateSession?.id ?? null : inProject ? projectSession?.id ?? null : null), agent: selectedAgent };
  const key = isGroupDraft && group ? `group:${group.id}:draft` : isProjectDraft && project ? `project:${project.id}:draft` : isAgentDraft ? `private:${selectedAgent}:draft` : selection.kind === "group" ? session?.id === selection.id ? `group:${selection.id}` : `group:${selection.id}:${session?.id ?? "loading"}` : selection.kind === "project" ? (projectSession ? (projectSession.is_default ? `project:${selection.id}` : `project:${selection.id}:${projectSession.id}`) : `project:${selection.id}:loading`) : inAgent ? (privateSession ? (privateSession.is_default ? `private:${selectedAgent}` : `private:${selectedAgent}:${privateSession.id}`) : `private:${selectedAgent}:loading`) : selection.kind === "agent" ? `agent:${selection.id}` : selection.kind;
  const snapshotKey = `${key}:${selectedAgent}`;
  const currentKey = useRef(snapshotKey); currentKey.current = snapshotKey;
  const pollingKey = useRef(snapshotKey);
  const visibleSnapshot = isDraft ? emptySnapshot : (pollingKey.current === snapshotKey ? snapshot : emptySnapshot);
  const title = isDraft ? "新会话" : (activeSession?.name ?? group?.name ?? (project ? `${project.name} · ${agentName(selectedAgent)}` : agentName(selectedAgent)));
  function selectExecutionAgent(agent: string) {
    if (group) {
      setGroupTargets(current => ({ ...current, [group.id]: agent }));
      return;
    }
    if (isProjectDraft && project) {
      if (agent === selectedAgent || !agents.some(item => item.id === agent && item.connected)) return;
      setDraftProjectAgents(current => ({ ...current, [project.id]: agent }));
      notify(`已切换到 ${agentName(agent)}`);
      return;
    }
    if (!inProject || !projectAgentKey || agent === selectedAgent ||
        !agents.some(item => item.id === agent && item.connected) ||
        activeSession?.archived || running(visibleSnapshot.session?.status)) return;
    const next = { ...projectAgents, [projectAgentKey]: agent };
    setProjectAgents(next);
    try { localStorage.setItem("ai-gent.project-agents.v1", JSON.stringify(next)); }
    catch { setError("Agent 选择未能保存，重启后需要重新选择。"); }
    notify(`已切换到 ${agentName(agent)}`);
  }
  function openGroup(id: string) { setGitOpen(false); setSchedulesOpen(false); setSelection({ kind: "group", id }); setGroupHub(false); setMode("chats"); setTab("chats"); setRail("groups"); }
  function showGroups() { setGitOpen(false); setSchedulesOpen(false); setGroupHub(true); setMode("chats"); setRail("groups"); }
  function openGitPanel(path?: string) {
    setSchedulesOpen(false);
    setGitPath(path ?? project?.primary_root ?? visibleSnapshot.session?.workspace_path ?? projects[0]?.primary_root);
    setGitOpen(true);
  }
  function openSchedulesPanel() {
    setGitOpen(false);
    setSchedulesOpen(true);
    void refreshSchedules();
  }
  async function openScheduleTarget(task: ScheduledTask, conversationId?: string | null) {
    if (task.target_type === "group" && task.group_id) {
      if (conversationId) {
        const next = { ...sessionSelection, [task.group_id]: conversationId };
        setSessionSelection(next);
        try { localStorage.setItem("ai-gent.selected-sessions.v1", JSON.stringify(next)); } catch { /* ignore */ }
        try {
          sessionsRevision.current += 1;
          const list = await getGroupSessions(task.group_id);
          setSessionsOwner(task.group_id);
          setSessions(list);
        } catch { /* ignore */ }
      }
      openGroup(task.group_id);
      return;
    }
    if (task.target_type === "project" && task.project_id) {
      if (conversationId) {
        const next = { ...projectSessionSelection, [task.project_id]: conversationId };
        setProjectSessionSelection(next);
        try { localStorage.setItem("ai-gent.selected-project-sessions.v1", JSON.stringify(next)); } catch { /* ignore */ }
        if (task.agent_id) {
          const nextAgents = { ...projectAgents, [`${task.project_id}:${conversationId}`]: task.agent_id };
          setProjectAgents(nextAgents);
          try { localStorage.setItem("ai-gent.project-agents.v1", JSON.stringify(nextAgents)); } catch { /* ignore */ }
        }
        try {
          projectSessionsRevision.current += 1;
          const list = await getProjectSessions(task.project_id);
          setProjectSessionsOwner(task.project_id);
          setProjectSessions(list);
        } catch { /* ignore */ }
      }
      setSchedulesOpen(false);
      setGitOpen(false);
      setGroupHub(false);
      setMode("projects");
      setSelection({ kind: "project", id: task.project_id });
      return;
    }
    if (task.agent_id) {
      if (conversationId) {
        const next = { ...privateSessionSelection, [task.agent_id]: conversationId };
        setPrivateSessionSelection(next);
        try { localStorage.setItem("ai-gent.selected-private-sessions.v1", JSON.stringify(next)); } catch { /* ignore */ }
        try {
          privateSessionsRevision.current += 1;
          const list = await getPrivateSessions(task.agent_id);
          setPrivateSessionsOwner(task.agent_id);
          setPrivateSessions(list);
        } catch { /* ignore */ }
      }
      setSchedulesOpen(false);
      setGitOpen(false);
      selectAgent(task.agent_id);
      return;
    }
    setSchedulesOpen(false);
    setGitOpen(false);
    showAllChats();
  }
  async function addProjectPath(path: string): Promise<Project | null> {
    if (!desktop) return null;
    try {
      const existing = projects.find(p => p.primary_root === path);
      if (existing) return existing;
      const p = await addProject(path);
      setProjects(await listProjects());
      return p;
    } catch {
      return null;
    }
  }
  function startNewSession() {
    if (inGroup && group) setDraftTarget({ kind: "group", groupId: group.id });
    else if (inProject && project) setDraftTarget({ kind: "project", projectId: project.id });
    else if (inAgent && selectedAgent) setDraftTarget({ kind: "agent", agentId: selectedAgent });
  }
  async function createDraftSession(seedTitle: string): Promise<{ scope: Scope; session: ChatSession } | null> {
    if (isGroupDraft && group) {
      const saved = await saveGroupSession(group.id, seedTitle);
      return {
        session: saved,
        scope: { projectId: null, groupId: saved.id, sessionId: null, agent: selectedAgent },
      };
    }
    if (isProjectDraft && project) {
      const saved = await saveProjectSession(project.id, seedTitle);
      const chosenAgent = draftProjectAgents[project.id] ?? selectedAgent;
      if (chosenAgent) {
        const nextAgents = { ...projectAgents, [`${project.id}:${saved.id}`]: chosenAgent };
        setProjectAgents(nextAgents);
        try { localStorage.setItem("ai-gent.project-agents.v1", JSON.stringify(nextAgents)); } catch {}
      }
      return {
        session: saved,
        scope: { projectId: project.id, groupId: null, sessionId: saved.id, agent: chosenAgent },
      };
    }
    if (isAgentDraft && selectedAgent) {
      const saved = await savePrivateSession(selectedAgent, seedTitle);
      return {
        session: saved,
        scope: { projectId: null, groupId: null, sessionId: saved.id, agent: selectedAgent },
      };
    }
    return null;
  }
  function commitDraftSession(saved: ChatSession) {
    if ("group_id" in saved) {
      sessionsRevision.current += 1;
      setSessions(current => [saved, ...current.filter(s => s.id !== saved.id)]);
      selectSession(saved);
      setGroups(current => current.map(g => g.id === saved.group_id ? { ...g, session_count: (g.session_count ?? 1) + 1 } : g));
    } else if ("project_id" in saved) {
      projectSessionsRevision.current += 1;
      setProjectSessions(current => [saved, ...current.filter(s => s.id !== saved.id)]);
      selectProjectSession(saved);
    } else {
      privateSessionsRevision.current += 1;
      setPrivateSessions(current => [saved, ...current.filter(s => s.id !== saved.id)]);
      selectPrivateSession(saved);
    }
    setDraftTarget(null);
  }
  async function renameSessionInline(target: ChatSession, nextName: string) {
    const trimmed = nextName.trim();
    if (!trimmed || trimmed === target.name) return;
    setSessionSaving(true);
    try {
      if ("group_id" in target) {
        const saved = await saveGroupSession(target.group_id, trimmed, target);
        sessionsRevision.current += 1;
        setSessions(current => current.map(s => s.id === saved.id ? saved : s));
      } else if ("project_id" in target) {
        const saved = await saveProjectSession(target.project_id, trimmed, target);
        projectSessionsRevision.current += 1;
        setProjectSessions(current => current.map(s => s.id === saved.id ? saved : s));
      } else {
        const saved = await savePrivateSession(target.agent_id, trimmed, target);
        privateSessionsRevision.current += 1;
        setPrivateSessions(current => current.map(s => s.id === saved.id ? saved : s));
      }
      notify("会话名称已更新");
    } catch (cause) {
      setError(String(cause));
      throw cause;
    } finally {
      setSessionSaving(false);
    }
  }
  function selectSession(value: GroupSession) {
    setDraftTarget(null);
    const next = { ...sessionSelection, [value.group_id]: value.id };
    setSessionSelection(next);
    try { localStorage.setItem("ai-gent.selected-sessions.v1", JSON.stringify(next)); } catch { setError("会话选择未能保存，重启后需要重新选择。"); }
  }
  function selectPrivateSession(value: PrivateSession) {
    setDraftTarget(null);
    const next = { ...privateSessionSelection, [value.agent_id]: value.id };
    setPrivateSessionSelection(next);
    try { localStorage.setItem("ai-gent.selected-private-sessions.v1", JSON.stringify(next)); } catch { setError("会话选择未能保存，重启后需要重新选择。"); }
  }
  function selectProjectSession(value: ProjectSession) {
    setDraftTarget(null);
    const next = { ...projectSessionSelection, [value.project_id]: value.id };
    setProjectSessionSelection(next);
    try { localStorage.setItem("ai-gent.selected-project-sessions.v1", JSON.stringify(next)); } catch { setError("会话选择未能保存，重启后需要重新选择。"); }
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
      } else if ("project_id" in target) {
        const updated = await saveProjectSession(target.project_id, target.name, target, !target.archived);
        projectSessionsRevision.current += 1;
        setProjectSessions(current => current.map(s => s.id === updated.id ? updated : s));
        notify(updated.archived ? "会话已归档，记录仍保留" : "会话已恢复");
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
    } else if ("project_id" in deleted) {
      projectSessionsRevision.current += 1;
      setProjectSessions(current => deleted.is_default ? current.map(s => s.id === deleted.id ? { ...s, preview: null, updated_at: null, status: null } : s) : current.filter(s => s.id !== deleted.id));
      if (projectSession?.id === deleted.id) setSnapshot(emptySnapshot);
      const next = { ...projectSessionSelection };
      if (!deleted.is_default && next[deleted.project_id] === deleted.id) delete next[deleted.project_id];
      setProjectSessionSelection(next);
      try { discardSessionDraft(deleted); localStorage.setItem("ai-gent.selected-project-sessions.v1", JSON.stringify(next)); }
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
  const notify = useCallback((message: string) => { setToast(message); }, []);
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
      setLoading(false);
    })();
    return () => { active = false; };
  }, []);
  function saveProbeResult(agentId: string, result: Probe | null) {
    setProbesByAgent(prev => {
      const next = { ...prev };
      if (result) next[agentId] = result;
      else delete next[agentId];
      try { localStorage.setItem("ai-gent.probe-cache.v1", JSON.stringify(next)); } catch {}
      return next;
    });
  }
  async function probeById(agentId: string, silent = false) {
    if (!desktop) return;
    setProbingByAgent(prev => ({ ...prev, [agentId]: true }));
    if (!silent) setError(null);
    try {
      const result = await probeAgent(agentId);
      saveProbeResult(agentId, result);
      await refreshAgents();
      void refreshUsage();
      if (!silent) notify(`${agentName(agentId)} 检测完成 · ${result.models.length} 个模型`);
    } catch (cause) {
      if (!silent) setError(`${agentName(agentId)} 连接失败：${String(cause)}`);
    } finally {
      setProbingByAgent(prev => ({ ...prev, [agentId]: false }));
    }
  }
  useEffect(() => {
    if (!desktop) return;
    const availableIds = agents.filter(a => a.available).map(a => a.id);
    const availableSet = new Set(availableIds);
    for (const id of [...autoProbedRef.current]) {
      if (!availableSet.has(id)) {
        autoProbedRef.current.delete(id);
        saveProbeResult(id, null);
      }
    }
    const pending = availableIds.filter(id => !autoProbedRef.current.has(id));
    if (!pending.length) return;
    pending.forEach(id => autoProbedRef.current.add(id));
    void Promise.allSettled(pending.map(id => probeById(id, true)));
  }, [agents.map(a => `${a.id}:${a.available}`).join(",")]);
  useEffect(() => {
    if (!isGroupDraft && group && session && sessionSelection[group.id] !== session.id) selectSession(session);
  }, [group?.id, session?.id, isGroupDraft]);
  useEffect(() => {
    if (!isAgentDraft && inAgent && privateSession && privateSessionSelection[selectedAgent] !== privateSession.id) selectPrivateSession(privateSession);
  }, [inAgent, selectedAgent, privateSession?.id, isAgentDraft]);
  const snapshotConversationId = useRef<string | undefined>(undefined);
  snapshotConversationId.current = visibleSnapshot.conversation_id;

  useEffect(() => {
    if (!group?.id) { setSessionsOwner(""); setSessions([]); setSessionsLoading(false); return; }
    const groupId = group.id;
    let active = true;
    setSessionsOwner(groupId); setSessions([]); setSessionsLoading(true);
    const revision = sessionsRevision.current;
    void getGroupSessions(groupId)
      .then(result => { if (active && revision === sessionsRevision.current) setSessions(result); })
      .catch(cause => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setSessionsLoading(false); });
    return () => { active = false; };
  }, [group?.id]);
  useEffect(() => {
    if (!inAgent || !selectedAgent) { setPrivateSessionsOwner(""); setPrivateSessions([]); setPrivateSessionsLoading(false); return; }
    const agentId = selectedAgent;
    let active = true;
    setPrivateSessionsOwner(agentId); setPrivateSessions([]); setPrivateSessionsLoading(true);
    const revision = privateSessionsRevision.current;
    void getPrivateSessions(agentId)
      .then(result => { if (active && revision === privateSessionsRevision.current) setPrivateSessions(result); })
      .catch(cause => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setPrivateSessionsLoading(false); });
    return () => { active = false; };
  }, [inAgent, selectedAgent]);
  useEffect(() => {
    if (!inProject || !project?.id) { setProjectSessionsOwner(""); setProjectSessions([]); setProjectSessionsLoading(false); return; }
    const projectId = project.id;
    let active = true;
    setProjectSessionsOwner(projectId); setProjectSessions([]); setProjectSessionsLoading(true);
    const revision = projectSessionsRevision.current;
    void getProjectSessions(projectId)
      .then(result => { if (active && revision === projectSessionsRevision.current) setProjectSessions(result); })
      .catch(cause => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setProjectSessionsLoading(false); });
    return () => { active = false; };
  }, [inProject, project?.id]);
  useEffect(() => {
    if (!isProjectDraft && inProject && project && projectSession && projectSessionSelection[project.id] !== projectSession.id) selectProjectSession(projectSession);
  }, [inProject, project?.id, projectSession?.id, isProjectDraft]);
  useEffect(() => {
    if (isDraft || !agentsLoaded || !selectedExists || (inAgent && !privateSession) || (inGroup && !session) || (inProject && !projectSession)) { setSnapshot(emptySnapshot); return; }
    let active = true;
    if (pollingKey.current !== snapshotKey) { setSnapshot(emptySnapshot); pollingKey.current = snapshotKey; }
    void getSnapshot(scope)
      .then(result => {
        if (active && currentKey.current === snapshotKey) {
          setSnapshot(result);
          if (selection.kind === "personal") setPrivatePreview(result.messages.filter(m => m.role === "user" || m.role === "assistant").at(-1)?.text ?? "");
        }
      })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, [key, scope.agent, isDraft, agentsLoaded, selectedExists, inAgent, privateSession?.id, inGroup, session?.id, inProject, projectSession?.id]);

  const realtimeRef = useRef<{
    onStream: (deltas: StreamDelta[]) => void;
    onSync: (batch: CoalescedSync) => void;
  }>({ onStream: () => {}, onSync: () => {} });
  realtimeRef.current = {
    onStream: deltas => {
      setSnapshot(prev => {
        if (pollingKey.current !== currentKey.current) return prev;
        const next = applyStreamDeltas(prev, deltas);
        if (next !== prev && selection.kind === "personal") {
          setPrivatePreview(next.messages.filter(m => m.role === "user" || m.role === "assistant").at(-1)?.text ?? "");
        }
        return next;
      });
    },
    onSync: batch => {
      const canFetchSnapshot = !isDraft && agentsLoaded && selectedExists && (!inAgent || !!privateSession) && (!inGroup || !!session) && (!inProject || !!projectSession);
      if (canFetchSnapshot && (batch.resyncAll || batch.scopes.has("snapshot"))) {
        const activeConvId = snapshotConversationId.current;
        const matchesCurrent = batch.resyncAll || batch.conversationIds.size === 0 || !activeConvId || batch.conversationIds.has(activeConvId);
        if (matchesCurrent) {
          const targetKey = snapshotKey;
          void getSnapshot(scope)
            .then(result => {
              if (currentKey.current === targetKey) {
                setSnapshot(result);
                if (selection.kind === "personal") setPrivatePreview(result.messages.filter(m => m.role === "user" || m.role === "assistant").at(-1)?.text ?? "");
              }
            })
            .catch(cause => setError(String(cause)));
        }
      }
      if (batch.resyncAll || batch.scopes.has("groups")) {
        void getGroups().then(setGroups).catch(cause => setError(String(cause)));
      }
      if (group?.id && (batch.resyncAll || batch.scopes.has("group_sessions"))) {
        const groupId = group.id;
        if (batch.resyncAll || batch.groupIds.size === 0 || batch.groupIds.has(groupId)) {
          const revision = sessionsRevision.current;
          void getGroupSessions(groupId)
            .then(result => { if (revision === sessionsRevision.current) setSessions(result); })
            .catch(cause => setError(String(cause)));
        }
      }
      if (inAgent && selectedAgent && (batch.resyncAll || batch.scopes.has("private_sessions"))) {
        const agentId = selectedAgent;
        if (batch.resyncAll || batch.agentIds.size === 0 || batch.agentIds.has(agentId)) {
          const revision = privateSessionsRevision.current;
          void getPrivateSessions(agentId)
            .then(result => { if (revision === privateSessionsRevision.current) setPrivateSessions(result); })
            .catch(cause => setError(String(cause)));
        }
      }
      if (desktop && (batch.resyncAll || batch.scopes.has("agents") || batch.scopes.has("dispatch"))) {
        void Promise.all([
          batch.resyncAll || batch.scopes.has("agents") ? refreshAgents() : Promise.resolve(),
          batch.resyncAll || batch.scopes.has("dispatch") ? invoke<boolean>("dispatch_snapshot").then(setPaused) : Promise.resolve(),
        ]).catch(cause => setError(String(cause)));
        if (batch.resyncAll || batch.scopes.has("agents")) {
          void refreshUsage();
        }
      }
      if (desktop && (batch.resyncAll || batch.scopes.has("schedules"))) {
        void refreshSchedules();
      }
    },
  };
  useEffect(() => {
    if (!desktop) return;
    return subscribeRealtimeEvents({
      onStream: deltas => realtimeRef.current.onStream(deltas),
      onSync: batch => realtimeRef.current.onSync(batch),
    });
  }, []);
  async function refreshCurrent() {
    const selected = snapshotKey;
    const [result, list, conversations, privateList, projectList] = await Promise.all([getSnapshot(scope), getGroups(), group ? getGroupSessions(group.id) : Promise.resolve(null), inAgent ? getPrivateSessions(selectedAgent) : Promise.resolve(null), inProject && project ? getProjectSessions(project.id) : Promise.resolve(null)]);
    if (currentKey.current === selected) setSnapshot(result);
    if (currentKey.current === selected && conversations) setSessions(conversations);
    if (currentKey.current === selected && privateList) setPrivateSessions(privateList);
    if (currentKey.current === selected && projectList) setProjectSessions(projectList);
    setGroups(list); if (selection.kind === "personal") setPrivatePreview(result.messages.filter(m => m.role === "user" || m.role === "assistant").at(-1)?.text ?? "");
  }
  async function probeSingleAgent(target: Agent) {
    if (!desktop) { notify(`请在桌面版中检测 ${target.name}`); return; }
    await probeById(target.id, false);
  }
  async function discoverLocalAgents(autoEnable = true, silent = false) {
    if (discovering) return;
    setDiscovering(true);
    setError(null);
    try {
      const results = await autoDiscoverAgents(autoEnable);
      setDiscoveryResults(results);
      if (desktop) {
        const connectedKinds = results.filter(item => item.status === "connected").map(item => item.provider_kind);
        await Promise.allSettled(connectedKinds.map(kind => probeById(kind, true)));
        for (const item of results) {
          if (item.status === "unavailable") saveProbeResult(item.provider_kind, null);
        }
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
  function projectFolderName(path: string): string {
    const trimmed = path.replace(/[\\/]+$/, "");
    const parts = trimmed.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }
  async function add() {
    if (!desktop) { notify("请在桌面版中选择本地项目目录"); return; }
    setError(null);
    try {
      const path = await open({ directory: true, multiple: false, title: "选择项目目录" });
      if (typeof path !== "string") return;
      setPendingProjectPath(path);
      setPendingProjectName("");
      setPendingProjectError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function confirmAddProject() {
    if (!pendingProjectPath || adding) return;
    setAdding(true);
    setPendingProjectError(null);
    try {
      const p = await addProject(pendingProjectPath, pendingProjectName);
      setProjects(await listProjects());
      setGroupHub(false);
      setSelection({ kind: "project", id: p.id });
      setMode("projects");
      setPendingProjectPath(null);
      setPendingProjectName("");
      notify(`已添加项目「${p.name}」`);
    } catch (cause) {
      setPendingProjectError(String(cause));
    } finally {
      setAdding(false);
    }
  }
  function defaultPrivateSelection(): Selection {
    if (hasCodexConnected) return { kind: "personal" };
    const fallback = connectedAgents[0]?.id;
    return fallback === "deepseek" ? { kind: "deepseek" } : fallback ? { kind: "agent", id: fallback } : { kind: "personal" };
  }
  function showAllChats() {
    setGitOpen(false); setSchedulesOpen(false); setGroupHub(false); setRail("all"); setMode("chats"); setTab("chats");
    if (selection.kind === "group" || selection.kind === "project") setSelection(defaultPrivateSelection());
  }
  function selectAgent(id: string) { setGitOpen(false); setSchedulesOpen(false); setRail(id); setMode("chats"); setTab("chats"); setSelection(id === "codex" ? {kind:"personal"} : id === "deepseek" ? {kind:"deepseek"} : {kind:"agent",id}); }
  const filteredGroups = groups.filter(g => g.name.toLowerCase().includes(query.toLowerCase()));
  const codexPrimaryWindow = usageData.codexLimits.windows[0];
  const quotaAlertAgent = usageData.agents.find(a => a.quotaExhaustedRecent);
  const quotaAlertText = quotaAlertAgent
    ? `${quotaAlertAgent.displayName} 限流`
    : codexPrimaryWindow && codexPrimaryWindow.remainingPercent <= 20
      ? `Codex ${Math.round(codexPrimaryWindow.remainingPercent)}%`
      : null;
  const fullStage = gitOpen || schedulesOpen;
  const isHubMode = !fullStage && groupHub && mode === "chats";
  const showWorkspaceSidebar = !fullStage && !isHubMode && !sidebarLayout.workspaceCollapsed;
  const showSessionSidebar = hasSessions && !sidebarLayout.sessionsCollapsed;
  const sidebarControls = (sidebarLayout.workspaceCollapsed || (hasSessions && sidebarLayout.sessionsCollapsed)) ? (
    <div className="sidebar-expand-controls">
      {sidebarLayout.workspaceCollapsed && (
        <button type="button" className="sidebar-expand-btn" title="展开我的工作台 (⌘B)" aria-label="展开我的工作台" onClick={() => toggleWorkspaceSidebar(false)}>
          <Icon name="chevronsRight" size={14} />
          <span>工作台</span>
        </button>
      )}
      {hasSessions && sidebarLayout.sessionsCollapsed && (
        <button type="button" className="sidebar-expand-btn" title="展开会话列表 (⌘B)" aria-label="展开会话列表" onClick={() => toggleSessionsSidebar(false)}>
          <Icon name="chevronsRight" size={14} />
          <span>会话</span>
        </button>
      )}
    </div>
  ) : null;
  const workbenchClass = [
    "workbench",
    fullStage ? "full-stage" : isHubMode ? "hub-mode" : hasSessions ? "session-mode" : "",
    !fullStage && !isHubMode && sidebarLayout.workspaceCollapsed ? "hide-workspace" : "",
    hasSessions && sidebarLayout.sessionsCollapsed ? "hide-sessions" : "",
  ].filter(Boolean).join(" ");
  const workbenchStyle = {
    ...(sidebarWidths.workspace ? { "--workspace-w": `${sidebarWidths.workspace}px` } : {}),
    ...(sidebarWidths.session ? { "--session-w": `${sidebarWidths.session}px` } : {}),
  } as CSSProperties;
  const isWorkspaceWaiting = loading || !agentsLoaded || (!selectedExists && (checkingAgents || discovering)) || (!isDraft && ((inGroup && (sessionsLoading || !session)) || (inAgent && (privateSessionsLoading || !privateSession)) || (inProject && (projectSessionsLoading || !projectSession))));
  const showingChat = !isWorkspaceWaiting && selectedExists;
  return <div className={`app-shell ${gitOpen ? "git-mode" : schedulesOpen ? "schedules-mode" : ""}`}>
    <header className="app-topbar"><div className="brand"><img className="brand-mark" src={appIcon} alt="" /><span>AgentCrew<span className="brand-edition">桌面版</span><small>多端协同 · 智能体工作台</small></span></div><nav className="top-nav" aria-label="工作区"><button className={!fullStage && mode === "chats" ? "active" : ""} onClick={() => { setGitOpen(false); setSchedulesOpen(false); setGroupHub(false); setMode("chats"); setTab("chats"); if (selection.kind === "project") setSelection(defaultPrivateSelection()); }}>对话</button><button className={!fullStage && mode === "projects" ? "active" : ""} onClick={() => { setGitOpen(false); setSchedulesOpen(false); setGroupHub(false); setMode("projects"); const targetProject = (selection.kind === "project" && projects.find(p => p.id === selection.id)) || (lastProjectIdRef.current && projects.find(p => p.id === lastProjectIdRef.current)) || projects[0]; if (targetProject) setSelection({ kind: "project", id: targetProject.id }); }}>项目空间</button></nav><div className="topbar-end"><button type="button" className="icon-button" onClick={() => setTheme(t => t === "dark" ? "light" : "dark")} title={theme === "dark" ? "切换为浅色主题" : "切换为暗黑主题"} aria-label={theme === "dark" ? "切换为浅色主题" : "切换为暗黑主题"}><Icon name={theme === "dark" ? "sun" : "moon"} size={17} /></button>{quotaAlertText && <button type="button" className="quota-alert-btn" onClick={() => openSettings("usage")} title="查看用量与额度 (⌘U)"><span className="quota-alert-dot" /><span>{quotaAlertText}</span></button>}<span className="local-badge"><Icon name="shield" size={14} />本机工作台</span><button className="profile-button" aria-label="工作台设置" onClick={() => openSettings("agents")}>我</button></div></header>
    <div className={workbenchClass} style={workbenchStyle}><aside className="agent-rail" aria-label="成员导航"><button className={`rail-button ${!fullStage && !groupHub && rail === "all" ? "active" : ""}`} onClick={showAllChats}><span className="rail-icon"><Icon name="chat" size={23} /></span><span>对话</span></button><button className={`rail-button ${!fullStage && rail === "groups" ? "active" : ""}`} onClick={showGroups} aria-label="查看所有群聊"><span className="rail-icon"><Icon name="group" size={23} /></span><span>群聊</span></button><div className="rail-rule" />{connectedAgents.map(agent => <button className={`rail-button ${!fullStage && rail === agent.id ? "active" : ""}`} key={agent.id} onClick={() => selectAgent(agent.id)} title={`${agent.name} 私聊`}><span className="rail-avatar"><Avatar agent={agent.id} /><i className={agent.availability === "ready" ? "ready" : ""} /></span><span>{agent.name}</span></button>)}<button className="rail-button add-agent" onClick={() => setPairing(true)}><span className="rail-icon dashed"><Icon name="plus" size={23} /></span><span>接入</span></button><div className="rail-spacer" /><button className={`rail-button ${schedulesOpen ? "active" : ""}`} onClick={() => openSchedulesPanel()} title="定时任务中心"><span className="rail-icon"><Icon name="clock" size={21} /></span><span>定时</span></button><button className={`rail-button ${gitOpen ? "active" : ""}`} onClick={() => openGitPanel()} title="代码变更与 Git (⌘G)"><span className="rail-icon"><Icon name="diff" size={21} /></span><span>变更</span></button><button className="rail-button" onClick={() => openSettings("agents")}><span className="rail-icon"><Icon name="settings" size={21} /></span><span>设置</span></button><span className="rail-version">v{info?.version ?? "0.1.3"}</span></aside>
    {gitOpen ? <GitPanel projects={projects} initialPath={gitPath} onAddProjectPath={addProjectPath} onNotify={notify} onClose={() => setGitOpen(false)} /> : schedulesOpen ? <SchedulesPanel tasks={scheduledTasks} runs={scheduledRuns} loading={schedulesLoading} paused={paused} agents={agents} projects={projects} groups={groups} probes={probesByAgent} onRefresh={refreshSchedules} onToggleDispatch={toggleDispatch} onOpenTarget={openScheduleTarget} onNotify={notify} onError={msg => setError(msg)} /> : isHubMode ? <main className="hub-workspace">{error && <div className="global-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError(null)}><Icon name="close" size={16} /></button></div>}{loading ? <div className="loading-conversation">正在读取群聊…</div> : <GroupHub groups={groups} projects={projects} onSelect={g => openGroup(g.id)} onCreate={() => setGroupDialog("new")} />}</main> : <>{showWorkspaceSidebar && (inGroup ? <aside className="conversation-sidebar group-sidebar" aria-label="群聊"><div className="sidebar-profile"><div><strong>我的工作台</strong><small>和自己的 Agent 一起工作</small></div><button type="button" className="sidebar-collapse-btn" title="收起我的工作台 (⌘B)" aria-label="收起我的工作台" onClick={() => toggleWorkspaceSidebar(true)}><Icon name="chevronsLeft" size={15} /></button></div><div className="group-sidebar-heading"><span>协作群</span><button onClick={showGroups} title="查看所有群聊" aria-label="查看所有群聊">{groups.length}<Icon name="right" size={12} /></button></div><GroupSidebarList groups={groups} projects={projects} selectedId={group?.id} onSelect={openGroup} onSettings={openGroupDialog} onDelete={target => openGroupDialog(target, true)} /><div className="group-sidebar-heading"><span>群成员</span></div><div className="group-sidebar-members">{group?.members.map(id => <Avatar agent={id} small key={id} />)}<Avatar agent="me" small /></div><div className="sidebar-footer"><span className="presence-dot connected" /><span>{desktop ? "记录保存在本机" : "浏览器预览"}</span></div><div className={`sidebar-resize-handle ${resizingSidebar === "workspace" ? "is-resizing" : ""}`} role="separator" aria-orientation="vertical" aria-label="调整工作台侧栏宽度" title="拖动调整宽度，双击恢复默认" onMouseDown={event => startSidebarResize("workspace", event)} onDoubleClick={() => resetSidebarWidth("workspace")} /></aside> : <aside className="conversation-sidebar" aria-label="会话导航"><div className="sidebar-profile"><Avatar agent="me" small /><div><strong>我的工作台</strong><small>和自己的 Agent 一起工作</small></div><button type="button" className="sidebar-collapse-btn" title="收起我的工作台 (⌘B)" aria-label="收起我的工作台" onClick={() => toggleWorkspaceSidebar(true)}><Icon name="chevronsLeft" size={15} /></button></div>
      <label className="sidebar-search"><Icon name="search" size={17} /><input aria-label={mode === "projects" ? "搜索项目" : "搜索会话"} placeholder={mode === "projects" ? "搜索项目" : tab === "contacts" ? "搜索成员" : "搜索会话或 Agent…"} value={query} onChange={e => setQuery(e.target.value)} /><kbd>⌕</kbd></label>
      <div className="conversation-list">{mode === "projects" ? <><div className="list-heading"><span>本地项目</span><span>{projects.length}</span></div><ProjectList projects={projects.filter(p => p.name.toLowerCase().includes(query.toLowerCase()))} selectedId={selection.kind === "project" ? selection.id : undefined} onSelect={id => setSelection({ kind: "project", id })} onError={setError} onRenamed={updated => { setProjects(current => current.map(p => p.id === updated.id ? updated : p)); notify(`项目名称已更新为「${updated.name}」`); }} onRemoved={removed => { const remaining = projects.filter(p => p.id !== removed.id); setProjects(remaining); if (lastProjectIdRef.current === removed.id) { lastProjectIdRef.current = remaining[0]?.id ?? null; try { if (lastProjectIdRef.current) localStorage.setItem("ai-gent.last-project-id.v1", lastProjectIdRef.current); else localStorage.removeItem("ai-gent.last-project-id.v1"); } catch {} } setSelection(current => current.kind === "project" && current.id === removed.id ? (remaining[0] ? { kind: "project", id: remaining[0].id } : defaultPrivateSelection()) : current); notify(`已移除「${removed.name}」`); }} />{!projects.length && <div className="sidebar-empty"><Icon name="folder" size={30} /><strong>给任务一个工作目录。</strong><p>添加项目后，可围绕项目文件继续聊。</p></div>}<button className="new-group-button" onClick={add} disabled={adding}><Icon name="plus" size={18} />{adding ? "正在添加…" : "添加本地项目"}</button></> : tab === "contacts" ? <><div className="list-heading"><span className="heading-tabs" role="tablist" aria-label="会话与通讯录"><button role="tab" aria-selected={tab !== "contacts"} onClick={() => setTab("chats")}>私聊</button><button role="tab" aria-selected={tab === "contacts"} onClick={() => setTab("contacts")}>通讯录</button></span><span>{agents.length}</span></div>{agents.filter(a => a.name.toLowerCase().includes(query.toLowerCase())).map(a => <button className="contact-item" onClick={() => selectAgent(a.id)} key={a.id}><Avatar agent={a.id} /><span><strong>{a.name}</strong><small>{a.connected ? "已连接 · 已显示在侧边栏" : a.available ? "已接入 · 等待连接就绪" : a.registration_state === "revoked" ? "已移除 · 历史记录保留" : a.registration_state === "active" ? "已停用 · 历史记录保留" : "尚未接入 · 请完成配对检查"}</small></span><Icon name="right" size={15} /></button>)}<div className="contacts-note"><Icon name="shield" size={16} />这里是你本机的成员。</div></> : <><div className="list-heading"><span className="heading-tabs" role="tablist" aria-label="会话与通讯录"><button role="tab" aria-selected={tab !== "contacts"} onClick={() => setTab("chats")}>私聊</button><button role="tab" aria-selected={tab === "contacts"} onClick={() => setTab("contacts")}>通讯录</button></span><span>{connectedAgents.filter(agent => rail === "all" || agent.id === rail).length}</span></div>{hasCodexConnected && (rail === "all" || rail === "codex") && "codex".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "personal" ? "selected" : ""}`} onClick={() => setSelection({ kind: "personal" })}><Avatar /><span className="conversation-text"><span className="conversation-name"><strong>Codex</strong>{connecting ? <span className="small-spinner" /> : <span className="connected-dot" />}</span><small>{privatePreview || "独立私聊，无需选择项目"}</small></span></button>}{hasDeepseekConnected && (rail === "all" || rail === "deepseek") && "deepseek".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "deepseek" ? "selected" : ""}`} onClick={() => setSelection({ kind: "deepseek" })}><Avatar agent="deepseek" /><span className="conversation-text"><span className="conversation-name"><strong>DeepSeek</strong>{deepseekConnecting ? <span className="small-spinner" /> : <span className="connected-dot" />}</span><small>Harness</small></span></button>}
      {connectedAgents.filter(a => a.id !== "codex" && a.id !== "deepseek" && (rail === "all" || rail === a.id) && a.name.toLowerCase().includes(query.toLowerCase())).map(a => <button className={`conversation-item ${selection.kind === "agent" && selection.id === a.id ? "selected" : ""}`} key={a.id} onClick={() => setSelection({ kind: "agent", id: a.id })}><Avatar agent={a.id}/><span className="conversation-text"><span className="conversation-name"><strong>{a.name}</strong><span className="connected-dot" /></span><small>{a.error ?? a.provider_kind}</small></span></button>)}{rail === "all" && <><div className="list-heading group-list-heading"><span>协作群</span><span>{filteredGroups.length}</span></div>{filteredGroups.map(g => <button className={`conversation-item ${selection.kind === "group" && selection.id === g.id ? "selected" : ""}`} key={g.id} onClick={() => openGroup(g.id)}><Avatar agent="group" /><span className="conversation-text"><span className="conversation-name"><strong title={g.name}>{g.name}</strong><time>{clock(g.updated_at)}</time></span><small>{running(g.status ?? "") ? "Agent 正在处理任务…" : g.preview ?? `${g.members.length} 位 Agent · 你`}</small></span></button>)}{!filteredGroups.length && <p className="no-groups">{query ? "没有找到匹配的群聊" : "让不同的想法，在一个群里碰面。"}</p>}</>}</>}</div>
      <div className="sidebar-footer"><span className={`presence-dot ${desktop ? "connected" : ""}`} /><span>{desktop ? "记录保存在本机" : "浏览器预览"}</span><button className="icon-button" title="设置与用量" aria-label="打开设置与用量" onClick={() => openSettings("agents")}><Icon name="shield" size={15} /></button></div>
      <div className={`sidebar-resize-handle ${resizingSidebar === "workspace" ? "is-resizing" : ""}`} role="separator" aria-orientation="vertical" aria-label="调整工作台侧栏宽度" title="拖动调整宽度，双击恢复默认" onMouseDown={event => startSidebarResize("workspace", event)} onDoubleClick={() => resetSidebarWidth("workspace")} />
    </aside>)}{showSessionSidebar && (inGroup && group ? <SessionSidebar key={group.id} sessions={groupSessions} selected={isGroupDraft ? undefined : session} isDraft={isGroupDraft} loading={sessionsLoading} saving={sessionSaving} onCollapse={() => toggleSessionsSidebar(true)} onRename={renameSessionInline} onArchive={archiveSession} onDelete={setDeleteSession} onSelect={selectSession} onCreate={startNewSession} resizing={resizingSidebar === "session"} onResizeStart={event => startSidebarResize("session", event)} onResizeReset={() => resetSidebarWidth("session")} /> : inProject && project ? <SessionSidebar key={`project:${project.id}`} sessions={currentProjectSessions} selected={isProjectDraft ? undefined : projectSession} isDraft={isProjectDraft} loading={projectSessionsLoading} saving={sessionSaving} searchPlaceholder={`搜索 ${project.name} 会话`} bottomNote="会话独立 · 项目目录共享" onCollapse={() => toggleSessionsSidebar(true)} onRename={renameSessionInline} onArchive={archiveSession} onDelete={setDeleteSession} onSelect={selectProjectSession} onCreate={startNewSession} resizing={resizingSidebar === "session"} onResizeStart={event => startSidebarResize("session", event)} onResizeReset={() => resetSidebarWidth("session")} /> : inAgent && <SessionSidebar key={`agent:${selectedAgent}`} sessions={agentSessions} selected={isAgentDraft ? undefined : privateSession} isDraft={isAgentDraft} loading={privateSessionsLoading} saving={sessionSaving} searchPlaceholder={`搜索 ${agentName(selectedAgent)} 会话`} bottomNote="会话独立 · 上下文隔离" onCollapse={() => toggleSessionsSidebar(true)} onRename={renameSessionInline} onArchive={archiveSession} onDelete={setDeleteSession} onSelect={selectPrivateSession} onCreate={startNewSession} resizing={resizingSidebar === "session"} onResizeStart={event => startSidebarResize("session", event)} onResizeReset={() => resetSidebarWidth("session")} />)}<main className="conversation-workspace">{!showingChat && sidebarControls && <div className="workspace-collapsed-bar">{sidebarControls}</div>}{!desktop && <div className="preview-notice">浏览器预览<span>Agent 执行与项目目录选择请使用桌面版。</span></div>}{error && <div className="global-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError(null)}><Icon name="close" size={16} /></button></div>}{isWorkspaceWaiting ? <div className="loading-conversation"><span className="small-spinner" />{inGroup || inAgent || inProject ? (sessionsLoading || privateSessionsLoading || projectSessionsLoading) ? "正在读取会话…" : "请选择或新建会话" : checkingAgents || discovering ? "正在连接本机已安装的 Agent…" : "正在打开你的工作台…"}</div> : !selectedExists ? <div className="conversation-welcome"><div className="welcome-mark"><Icon name="spark" size={37}/></div><h2>还没有已连接的 Agent</h2><p>在设置中自动检测并连接本机已安装的 Agent，或手动添加一位 Agent 后开始对话。</p><div className="pairing-actions"><button className="primary" disabled={discovering} onClick={()=>void discoverLocalAgents(true)}>{discovering ? "正在检测本机 Agent…" : "自动检测并连接"}</button><button className="outline-button" onClick={()=>setPairing(true)}><Icon name="plus" size={16}/>手动添加 Agent</button></div></div> : <ConnectedChat sidebarControls={sidebarControls} onOpenGit={openGitPanel} session={activeSession} isDraft={isDraft} onCreateDraftSession={isDraft ? createDraftSession : undefined} onCommitDraftSession={isDraft ? commitDraftSession : undefined} onGroups={showGroups} onRenameSession={() => activeSession && setSessionDialog(activeSession)} onArchiveSession={() => archiveSession()} onDeleteSession={() => activeSession && setDeleteSession(activeSession)} onGroupUpdated={updated => setGroups(current => current.map(g => g.id === updated.id ? updated : g))} sessionSaving={sessionSaving} key={key} scope={scope} scopeKey={key} title={title} group={group} project={project} agent={selectedAgent} onAgent={selectExecutionAgent} snapshot={visibleSnapshot} probes={probesByAgent} connectingByAgent={probingByAgent} onConnectAgent={id => void probeById(id, false)} onInvite={() => group && openGroupDialog(group)} onSettings={() => setSettings(true)} onRefresh={refreshCurrent} notify={notify} />}</main></>}</div>
    {groupDialog && <GroupDialog initialDelete={groupDialog !== "new" && groupDialogDelete} key={groupDialog === "new" ? "new" : groupDialog.id} existing={groupDialog === "new" ? undefined : groupDialog} projects={projects} onClose={() => setGroupDialog(null)} onDeleted={groupDeleted} onSaved={saved => { setGroups(current => [saved, ...current.filter(g => g.id !== saved.id)]); openGroup(saved.id); setGroupDialog(null); notify(groupDialog === "new" ? `已创建「${saved.name}」` : "群设置已保存"); }} />}
    {sessionDialog && (group || inAgent || (inProject && project)) && <SessionDialog key={sessionDialog.id} groupId={group?.id} projectId={inProject ? project?.id : undefined} agentId={group || inProject ? undefined : selectedAgent} existing={sessionDialog} onClose={() => setSessionDialog(null)} onSaved={saved => { if ("group_id" in saved) { sessionsRevision.current += 1; setSessions(current => current.map(s => s.id === saved.id ? saved : s)); } else if ("project_id" in saved) { projectSessionsRevision.current += 1; setProjectSessions(current => current.map(s => s.id === saved.id ? saved : s)); } else { privateSessionsRevision.current += 1; setPrivateSessions(current => current.map(s => s.id === saved.id ? saved : s)); } setSessionDialog(null); notify("会话名称已更新"); }} />}
    {pendingProjectPath && <Dialog title="添加本地项目" onClose={() => { if (!adding) { setPendingProjectPath(null); setPendingProjectName(""); setPendingProjectError(null); } }}>
      <form onSubmit={event => { event.preventDefault(); void confirmAddProject(); }}>
        <p className="project-path-preview" title={pendingProjectPath}>{pendingProjectPath}</p>
        <label className="field-label" htmlFor="add-project-name">项目名称 <span>选填</span></label>
        <input id="add-project-name" className="text-input" value={pendingProjectName} onChange={e => setPendingProjectName(e.target.value)} maxLength={60} placeholder={projectFolderName(pendingProjectPath)} autoFocus />
        <p className="field-help">留空则默认显示文件夹名「{projectFolderName(pendingProjectPath)}」。</p>
        {pendingProjectError && <p className="inline-error" role="alert">{pendingProjectError}</p>}
        <div className="dialog-footer"><span /><div className="session-dialog-actions"><button type="button" className="outline-button" disabled={adding} onClick={() => { setPendingProjectPath(null); setPendingProjectName(""); setPendingProjectError(null); }}>取消</button><button type="submit" className="primary" disabled={adding}>{adding ? "正在添加…" : "确认添加"}</button></div></div>
      </form>
    </Dialog>}
    {deleteSession && <DeleteSessionDialog session={deleteSession} onClose={() => setDeleteSession(null)} onDeleted={sessionDeleted} />}
    {pairing && <PairingDialog invitation={invitation} onInvitation={setInvitation} onClose={()=>setPairing(false)}/> }
    {settings && (
      <SettingsDialog
        initialTab={settingsTab}
        agents={agents}
        discoveryResults={discoveryResults}
        discovering={discovering}
        probes={probesByAgent}
        probing={probingByAgent}
        paused={paused}
        info={info}
        usageData={usageData}
        usageDays={usageDays}
        usageLoading={usageLoading}
        onChangeUsageDays={d => setUsageDays(d)}
        onRefreshUsage={() => void refreshUsage()}
        onDiscover={() => void discoverLocalAgents(true)}
        onProbeAgent={agent => void probeSingleAgent(agent)}
        onOpenPairing={() => { setSettings(false); setPairing(true); }}
        onToggleDispatch={() => void toggleDispatch()}
        onError={setError}
        onClose={() => setSettings(false)}
      />
    )}
    {toast && <div className="toast" role="status"><Icon name="check" size={16} />{toast}</div>}
  </div>;
}
