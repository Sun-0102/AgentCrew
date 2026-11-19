import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { addProject, getAppInfo, listProjects, type AppInfo, type Project } from "./features/projects/api";
import { agents, clock, desktop, emptySnapshot, getGroups, getSnapshot, probeCodex, probeDeepSeek, running, type Group, type Probe, type Scope, type Snapshot } from "./features/chats/api";
import { selectedSpeed } from "./features/chats/speed";
import { selectedEffort } from "./features/chats/reasoning";
import ChatStage from "./features/chats/ChatStage";
import GroupDialog from "./features/chats/GroupDialog";
import Icon from "./components/Icon";
import Avatar from "./components/Avatar";
import Dialog from "./components/Dialog";

type Selection = { kind: "personal" } | { kind: "deepseek" } | { kind: "group"; id: string } | { kind: "project"; id: string };
export default function Workbench() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [groupTargets, setGroupTargets] = useState<Record<string, "codex" | "deepseek">>({});
  const [selection, setSelection] = useState<Selection>({ kind: "personal" });
  const [mode, setMode] = useState<"chats" | "projects">("chats");
  const [rail, setRail] = useState("all");
  const [tab, setTab] = useState("chats");
  const [query, setQuery] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [deepseekProbe, setDeepseekProbe] = useState<Probe | null>(null);
  const [deepseekModel, setDeepseekModel] = useState("");
  const [deepseekConnecting, setDeepseekConnecting] = useState(desktop);
  const [snapshot, setSnapshot] = useState<Snapshot>(emptySnapshot);
  const [privatePreview, setPrivatePreview] = useState("");
  const [model, setModel] = useState("");
  const [efforts, setEfforts] = useState<Record<string, string>>(() => {
    try { const saved = JSON.parse(localStorage.getItem("ai-gent.reasoning.v1") ?? "{}"); return saved && typeof saved === "object" && !Array.isArray(saved) ? saved : {}; } catch { return {}; }
  });
  function chooseEffort(agent: string, model: string, value: string) {
    const next = { ...efforts, [`${agent}:${model}`]: value };
    setEfforts(next);
    try { localStorage.setItem("ai-gent.reasoning.v1", JSON.stringify(next)); } catch { setError("思考模式未能保存，重启后需要重新选择。"); }
  }
  const [speeds, setSpeeds] = useState<Record<string, string>>(() => {
    try { const saved = JSON.parse(localStorage.getItem("ai-gent.speed.v1") ?? "{}"); return saved && typeof saved === "object" && !Array.isArray(saved) ? saved : {}; } catch { return {}; }
  });
  function chooseSpeed(value: string) {
    const next = { ...speeds, [model]: value };
    setSpeeds(next);
    try { localStorage.setItem("ai-gent.speed.v1", JSON.stringify(next)); } catch { setError("速度选择未能保存，重启后需要重新选择。"); }
  }
  const codexSpeed = selectedSpeed(probe?.models.find(m => m.model === model), speeds[model]);
  const reasoningEfforts = {
    codex: selectedEffort(probe?.models.find(m => m.model === model), efforts[`codex:${model}`]),
    deepseek: selectedEffort(deepseekProbe?.models.find(m => m.model === deepseekModel), efforts[`deepseek:${deepseekModel}`]),
  };
  const [connecting, setConnecting] = useState(desktop);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState("");
  const [groupDialog, setGroupDialog] = useState<"new" | Group | null>(null);
  const [settings, setSettings] = useState(false);
  const group = selection.kind === "group" ? groups.find(g => g.id === selection.id) : undefined;
  const project = selection.kind === "project" ? projects.find(p => p.id === selection.id) : projects.find(p => p.id === group?.project_id);
  const selectedAgent = selection.kind === "deepseek" ? "deepseek" : group ? (group.members.includes(groupTargets[group.id] ?? "codex") ? groupTargets[group.id] ?? "codex" : group.members[0] === "deepseek" ? "deepseek" : "codex") : "codex";
  const scope: Scope = { projectId: selection.kind === "project" ? selection.id : null, groupId: selection.kind === "group" ? selection.id : null, agent: selectedAgent };
  const key = selection.kind === "group" || selection.kind === "project" ? `${selection.kind}:${selection.id}` : selection.kind;
  const currentKey = useRef(key); currentKey.current = key;
  const pollingKey = useRef(key);
  const title = group?.name ?? (selection.kind === "deepseek" ? "DeepSeek" : project ? `${project.name} · Codex` : "Codex");
  function chooseModel(result: Probe, previous?: string) { setModel(current => result.models.find(m => m.model === (current || previous))?.model ?? result.models.find(m => m.isDefault)?.model ?? result.models[0]?.model ?? ""); }
  function chooseDeepseekModel(result: Probe, previous?: string) { setDeepseekModel(current => result.models.find(m => m.model === (current || previous))?.model ?? result.models.find(m => m.isDefault)?.model ?? result.models[0]?.model ?? ""); }
  function notify(message: string) { setToast(message); }
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(""), 3600); return () => clearTimeout(timer); }, [toast]);
  useEffect(() => {
    let active = true;
    void (async () => {
      const results = await Promise.allSettled([desktop ? listProjects() : Promise.resolve([]), desktop ? getAppInfo() : Promise.resolve(null), getGroups(), getSnapshot({ projectId: null, groupId: null }), desktop ? probeCodex() : Promise.resolve(null)]);
      if (!active) return;
      const [p, i, g, s, c] = results;
      if (p.status === "fulfilled") setProjects(p.value);
      if (i.status === "fulfilled") setInfo(i.value);
      if (g.status === "fulfilled") setGroups(g.value);
      if (s.status === "fulfilled" && currentKey.current === "personal") { setSnapshot(s.value); setPrivatePreview(s.value.messages.filter(m => m.role !== "tool").at(-1)?.text ?? ""); }
      if (c.status === "fulfilled" && c.value) { setProbe(c.value); chooseModel(c.value, s.status === "fulfilled" ? s.value.session?.model : undefined); }
      const failure = results.find(result => result.status === "rejected");
      if (failure?.status === "rejected") setError(String(failure.reason));
      setLoading(false); setConnecting(false);
    })();
    return () => { active = false; };
  }, []);
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    void (async () => {
      const [connection, history] = await Promise.allSettled([
        probeDeepSeek(), getSnapshot({ projectId: null, groupId: null, agent: "deepseek" }),
      ]);
      if (!active) return;
      if (connection.status === "fulfilled") {
        setDeepseekProbe(connection.value);
        chooseDeepseekModel(connection.value, history.status === "fulfilled" ? history.value.session?.model : undefined);
      } else {
        setError(`DeepSeek 自动连接失败：${String(connection.reason)}`);
      }
      if (history.status === "rejected") setError(`DeepSeek 历史记录读取失败：${String(history.reason)}`);
      setDeepseekConnecting(false);
    })();
    return () => { active = false; };
  }, []);
  useEffect(() => {
    let active = true; let timer: ReturnType<typeof setTimeout>;
    if (pollingKey.current !== key) { setSnapshot(emptySnapshot); pollingKey.current = key; }
    const refresh = async () => {
      try { const result = await getSnapshot(scope); if (active) { setSnapshot(result); if (key === "personal") setPrivatePreview(result.messages.filter(m => m.role !== "tool").at(-1)?.text ?? ""); } }
      catch (cause) { if (active) setError(String(cause)); }
      if (active) timer = setTimeout(refresh, 900);
    };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, [key, scope.agent]);
  useEffect(() => {
    let active = true; let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => { try { const result = await getGroups(); if (active) setGroups(result); } catch (cause) { if (active) setError(String(cause)); } if (active) timer = setTimeout(refresh, 3500); };
    void refresh(); return () => { active = false; clearTimeout(timer); };
  }, []);
  async function refreshCurrent() {
    const selected = key;
    const [result, list] = await Promise.all([getSnapshot(scope), getGroups()]);
    if (currentKey.current === selected) setSnapshot(result);
    setGroups(list); if (selected === "personal") setPrivatePreview(result.messages.filter(m => m.role !== "tool").at(-1)?.text ?? "");
  }
  async function connect() {
    if (!desktop) { notify("请在桌面版中连接本机 Codex"); return; }
    setConnecting(true); setError(null);
    try { const result = await probeCodex(); setProbe(result); chooseModel(result, snapshot.session?.model); notify(`已连接 Codex · ${result.models.length} 个模型`); }
    catch (cause) { setError(String(cause)); } finally { setConnecting(false); }
  }
  async function connectDeepseek() {
    if (!desktop) { notify("请在桌面版中连接 DeepSeek Harness"); return; }
    setDeepseekConnecting(true); setError(null);
    try {
      const [result, saved] = await Promise.all([probeDeepSeek(), getSnapshot({ projectId: null, groupId: null, agent: "deepseek" })]);
      setDeepseekProbe(result);
      chooseDeepseekModel(result, saved.session?.model);
      notify(`DeepSeek Harness 已就绪 · ${result.models.length} 个内置目录模型`);
    } catch (cause) { setDeepseekProbe(null); setError(String(cause)); } finally { setDeepseekConnecting(false); }
  }
  async function add() {
    if (!desktop) { notify("请在桌面版中选择本地项目目录"); return; }
    setAdding(true); setError(null);
    try { const path = await open({ directory: true, multiple: false, title: "选择项目目录" }); if (typeof path !== "string") return; const p = await addProject(path); setProjects(await listProjects()); setSelection({ kind: "project", id: p.id }); setMode("projects"); }
    catch (cause) { setError(String(cause)); } finally { setAdding(false); }
  }
  function selectAgent(id: string) { setRail(id); setMode("chats"); setTab("chats"); setSelection({ kind: id === "deepseek" ? "deepseek" : "personal" }); }
  const filteredGroups = groups.filter(g => g.name.toLowerCase().includes(query.toLowerCase()) && (rail === "all" || g.members.includes(rail)));
  return <div className="app-shell">
    <header className="app-topbar"><div className="brand"><span className="brand-mark">A</span><span>AI-gent<small>你的 AI，坐在同一张桌前。</small></span></div><nav className="top-nav" aria-label="工作区"><button className={mode === "chats" ? "active" : ""} onClick={() => { setMode("chats"); setTab("chats"); }}>聊天</button><button className={mode === "projects" ? "active" : ""} onClick={() => { setMode("projects"); if (projects[0]) setSelection({ kind: "project", id: projects[0].id }); }}>工作空间</button></nav><div className="topbar-end"><span className="local-badge"><Icon name="shield" size={14} />本机工作台</span><button className="profile-button" aria-label="工作台设置" onClick={() => setSettings(true)}>我</button></div></header>
    <div className="workbench"><aside className="agent-rail" aria-label="成员导航"><button className={`rail-button ${rail === "all" ? "active" : ""}`} onClick={() => { setRail("all"); setMode("chats"); setTab("chats"); }}><span className="rail-icon"><Icon name="chat" size={23} /></span><span>全部</span></button><div className="rail-rule" />{agents.map(agent => <button className={`rail-button ${rail === agent.id ? "active" : ""}`} key={agent.id} onClick={() => selectAgent(agent.id)} title={`${agent.name} 私聊`}><span className="rail-avatar"><Avatar agent={agent.id} /><i className={agent.available ? "ready" : ""} /></span><span>{agent.name}</span></button>)}<button className="rail-button add-agent" onClick={() => setSettings(true)}><span className="rail-icon dashed"><Icon name="plus" size={23} /></span><span>接入</span></button><div className="rail-spacer" /><button className="rail-button" onClick={() => setSettings(true)}><span className="rail-icon"><Icon name="settings" size={21} /></span><span>设置</span></button><span className="rail-version">v{info?.version ?? "0.1.0"}</span></aside>
    <aside className="conversation-sidebar" aria-label="会话导航"><div className="sidebar-profile"><Avatar agent="me" small /><div><strong>我的工作台</strong><small>和自己的 Agent 一起工作</small></div><button className="icon-button" aria-label="工作台连接设置" onClick={() => setSettings(true)}><Icon name="settings" size={17} /></button></div>
      {mode === "chats" && <div className={`sidebar-tabs ${tab === "contacts" ? "contacts-active" : ""}`} role="tablist" aria-label="会话与通讯录"><span className="tab-slider" /><button role="tab" aria-selected={tab === "chats"} onClick={() => setTab("chats")}>聊天</button><button role="tab" aria-selected={tab === "contacts"} onClick={() => setTab("contacts")}>通讯录</button></div>}
      <label className="sidebar-search"><Icon name="search" size={17} /><input aria-label={mode === "projects" ? "搜索项目" : "搜索会话"} placeholder={mode === "projects" ? "搜索项目" : tab === "contacts" ? "搜索成员" : "搜索会话"} value={query} onChange={e => setQuery(e.target.value)} /><kbd>⌕</kbd></label>
      {mode === "chats" && <button className="new-group-button sidebar-create-group" onClick={() => setGroupDialog("new")}><Icon name="plus" size={18} />创建协作群</button>}
      <div className="conversation-list">{mode === "projects" ? <><div className="list-heading"><span>本地项目</span><span>{projects.length}</span></div>{projects.filter(p => p.name.toLowerCase().includes(query.toLowerCase())).map(p => <button className={`conversation-item ${selection.kind === "project" && selection.id === p.id ? "selected" : ""}`} key={p.id} onClick={() => setSelection({ kind: "project", id: p.id })}><span className="folder-avatar"><Icon name="folder" size={21} /></span><span className="conversation-text"><strong>{p.name}</strong><small title={p.primary_root}>{p.primary_root}</small></span></button>)}{!projects.length && <div className="sidebar-empty"><Icon name="folder" size={30} /><strong>给任务一个工作目录。</strong><p>添加项目后，可围绕项目文件继续聊。</p></div>}<button className="new-group-button" onClick={add} disabled={adding}><Icon name="plus" size={18} />{adding ? "正在添加…" : "添加本地项目"}</button></> : tab === "contacts" ? <><div className="list-heading"><span>我的 Agent</span><span>{agents.length}</span></div>{agents.filter(a => a.name.toLowerCase().includes(query.toLowerCase())).map(a => <button className="contact-item" onClick={() => selectAgent(a.id)} key={a.id}><Avatar agent={a.id} /><span><strong>{a.name}</strong><small>{a.available ? "已接入 · 可发起私聊" : "待接入 · 可先加入群"}</small></span><Icon name="right" size={15} /></button>)}<div className="contacts-note"><Icon name="shield" size={16} />这里是你本机的成员。</div></> : <><div className="list-heading"><span>私聊</span><span>{rail === "all" ? 2 : 1}</span></div>{rail !== "deepseek" && "codex".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "personal" ? "selected" : ""}`} onClick={() => setSelection({ kind: "personal" })}><Avatar /><span className="conversation-text"><span className="conversation-name"><strong>Codex</strong>{connecting ? <span className="small-spinner" /> : probe && <span className="connected-dot" />}</span><small>{privatePreview || "独立私聊，无需选择项目"}</small></span></button>}{rail !== "codex" && "deepseek".includes(query.toLowerCase()) && <button className={`conversation-item ${selection.kind === "deepseek" ? "selected" : ""}`} onClick={() => setSelection({ kind: "deepseek" })}><Avatar agent="deepseek" /><span className="conversation-text"><span className="conversation-name"><strong>DeepSeek</strong><span className="pending-label">{deepseekConnecting ? "连接中…" : deepseekProbe ? "已就绪" : "可重试"}</span></span><small>Harness 独立私聊</small></span></button>}
      <div className="list-heading group-list-heading"><span>协作群</span></div>{filteredGroups.map(g => <button className={`conversation-item ${selection.kind === "group" && selection.id === g.id ? "selected" : ""}`} key={g.id} onClick={() => setSelection({ kind: "group", id: g.id })}><Avatar agent="group" /><span className="conversation-text"><span className="conversation-name"><strong title={g.name}>{g.name}</strong><time>{clock(g.updated_at)}</time></span><small>{running(g.status ?? "") ? "Agent 正在处理任务…" : g.preview ?? `${g.members.length} 位 Agent · 你`}</small></span></button>)}{!filteredGroups.length && <p className="no-groups">{query ? "没有找到匹配的群聊" : "让不同的想法，在一个群里碰面。"}</p>}</>}</div>
      <div className="sidebar-footer"><span className={`presence-dot ${desktop ? "connected" : ""}`} /><span>{desktop ? "记录保存在本机" : "浏览器预览"}</span><button className="icon-button" title="数据与连接" aria-label="查看数据与连接" onClick={() => setSettings(true)}><Icon name="shield" size={15} /></button></div>
    </aside><main className="conversation-workspace">{!desktop && <div className="preview-notice">浏览器预览<span>Agent 执行与项目目录选择请使用桌面版。</span></div>}{error && <div className="global-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError(null)}><Icon name="close" size={16} /></button></div>}{loading ? <div className="loading-conversation"><span className="small-spinner" />正在打开你的工作台…</div> : <ChatStage key={key} scope={scope} scopeKey={key} title={title} group={group} project={project} agent={selectedAgent} onAgent={agent => { if (group) setGroupTargets(current => ({ ...current, [group.id]: agent })); }} snapshot={snapshot} codexServiceTier={codexSpeed} onServiceTier={chooseSpeed} reasoningEffort={reasoningEfforts[selectedAgent]} collaborationEfforts={reasoningEfforts} onReasoningEffort={value => chooseEffort(selectedAgent, selectedAgent === "deepseek" ? deepseekModel : model, value)} collaborationModels={{ codex: probe && (!probe.requires_auth || probe.auth) && !connecting ? model : "", deepseek: deepseekProbe && !deepseekConnecting ? deepseekModel : "" }} probe={selectedAgent === "deepseek" ? deepseekProbe : probe} model={selectedAgent === "deepseek" ? deepseekModel : model} connecting={selectedAgent === "deepseek" ? deepseekConnecting : connecting} onModel={selectedAgent === "deepseek" ? setDeepseekModel : setModel} onConnect={selectedAgent === "deepseek" ? connectDeepseek : connect} onInvite={() => group && setGroupDialog(group)} onSettings={() => setSettings(true)} onRefresh={refreshCurrent} notify={notify} />}</main></div>
    {groupDialog && <GroupDialog key={groupDialog === "new" ? "new" : groupDialog.id} existing={groupDialog === "new" ? undefined : groupDialog} projects={projects} onClose={() => setGroupDialog(null)} onSaved={saved => { setGroups(current => [saved, ...current.filter(g => g.id !== saved.id)]); setSelection({ kind: "group", id: saved.id }); setMode("chats"); setTab("chats"); setRail("all"); setGroupDialog(null); notify(groupDialog === "new" ? `已创建「${saved.name}」` : "群设置已保存"); }} />}
    {settings && <Dialog title="成员接入与设置" onClose={() => setSettings(false)} wide><p className="dialog-intro">使用本机的 Agent 运行组件，工作记录留在你的电脑上。</p><div className="connection-card"><Avatar /><div><strong>Codex</strong><p>{probe ? `${probe.auth ?? "本机服务配置"} · ${probe.models.length} 个可选模型` : connecting ? "正在检测本机运行组件…" : "连接本机 Codex，即可开始工作"}</p></div><button className="outline-button" disabled={connecting || !desktop} onClick={connect}>{connecting ? "连接中…" : probe ? "重新检测" : "连接"}</button></div>{probe && <details className="runtime-details"><summary>{probe.version}</summary><code>{probe.executable}</code></details>}<div className="connection-card"><Avatar agent="deepseek" /><div><strong>DeepSeek</strong><p>{deepseekProbe ? `Harness 配置 · ${deepseekProbe.models.length} 个内置目录模型` : deepseekConnecting ? "正在自动连接本机 Harness…" : "自动连接未完成，可重试连接本机 Harness。"}</p></div><button className="outline-button" disabled={deepseekConnecting || !desktop} onClick={connectDeepseek}>{deepseekConnecting ? "连接中…" : deepseekProbe ? "重新检测" : "连接"}</button></div>{deepseekProbe && <details className="runtime-details"><summary>Harness {deepseekProbe.version} · 模型来源：已安装 Harness 内置目录</summary><code>{deepseekProbe.executable}</code></details>}<p className="field-help">DeepSeek 回复按消息显示。当前 Windows 终端执行暂不可用，文件工具可用。需要额外权限的操作会被拒绝；当前 SDK 不提供审批交互。取消会停止本软件启动的 Harness 进程组，已写入的文件保留。群内可 @DeepSeek 执行文件任务；不会自动唤醒其他成员。</p><div className="settings-section"><Icon name="shield" size={20} /><div><strong>本机工作台</strong><p>一次执行一个任务。聊天、群成员和项目记录分别保留，退出软件会停止本软件管理的执行。</p></div></div>{info && <details className="runtime-details"><summary>本地数据位置</summary><code>{info.database_path}</code></details>}<div className="dialog-footer"><span>AI-gent · v{info?.version ?? "0.1.0"}</span><button className="primary" onClick={() => setSettings(false)}>完成<Icon name="check" size={16} /></button></div></Dialog>}
    {toast && <div className="toast" role="status"><Icon name="check" size={16} />{toast}</div>}
  </div>;
}
