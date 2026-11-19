import { useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { addProject, getAppInfo, listProjects, type AppInfo, type Project } from "./features/projects/api";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const desktop = isTauri();
  const selected = projects.find((project) => project.id === selectedId);

  useEffect(() => {
    if (!desktop) { setLoading(false); return; }
    let active = true;
    Promise.all([listProjects(), getAppInfo()])
      .then(([items, appInfo]) => {
        if (!active) return;
        setProjects(items);
        setSelectedId(items[0]?.id ?? null);
        setInfo(appInfo);
      })
      .catch((cause: unknown) => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [desktop]);

  async function handleAddProject() {
    setAdding(true);
    setError(null);
    try {
      const path = await open({ directory: true, multiple: false, title: "选择项目目录" });
      if (typeof path !== "string") return;
      const project = await addProject(path);
      const items = await listProjects();
      setProjects(items);
      setSelectedId(project.id);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setAdding(false);
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="项目导航">
        <div className="brand"><span className="brand-mark">A</span><div>AI-gent<small>本地 AI 协作工作台</small></div></div>
        <div className="section-label">工作空间 <span>{projects.length}</span></div>
        <button className="add-project" onClick={handleAddProject} disabled={!desktop || loading || adding}>
          <span aria-hidden="true">＋</span> {adding ? "正在添加…" : "添加本地项目"}
        </button>
        <nav className="project-list" aria-label="项目列表">
          {projects.map((project) => (
            <button key={project.id} className={`project-item ${selectedId === project.id ? "selected" : ""}`} aria-current={selectedId === project.id ? "page" : undefined} onClick={() => setSelectedId(project.id)} title={project.primary_root}>
              <svg className="folder" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10H3Z" /></svg><span>{project.name}</span>
            </button>
          ))}
          {!projects.length && <p className="nav-hint">{loading ? "正在读取项目…" : "添加一个目录，开始整理你的工作。"}</p>}
        </nav>
        <div className="local-note"><span className="status-dot" />本地存储<small>{info ? `v${info.version} · 开发版` : "开发框架"}</small></div>
      </aside>

      <main className="workspace">
        <header className="workspace-header"><div className="breadcrumb">工作空间 <span>/</span> <strong>{selected?.name ?? "开始使用"}</strong></div><span className="badge">框架阶段</span></header>
        {!desktop && <div className="notice">当前为浏览器预览。请运行 npm run desktop，使用目录选择和本地数据库。</div>}
        {error && <div className="error" role="alert">{error}<button aria-label="关闭错误提示" onClick={() => setError(null)}>×</button></div>}
        <div className="workspace-content">
          <div className="eyebrow"><span aria-hidden="true" />项目工作台</div>
          <h1>{selected ? selected.name : "从一个本地项目开始。"}</h1>
          <p className="intro">{selected ? "在这里管理这个项目的 AI 会话、任务与交付。" : "把项目、AI 会话和工作记录放在一起，让每一步都有迹可循。"}</p>
          {selected ? <div className="path-card"><span>项目目录</span><code>{selected.primary_root}</code><small>仅登记目录，源码保留在原位置。</small></div> : <button className="primary" onClick={handleAddProject} disabled={!desktop || loading || adding}>＋ 选择项目目录</button>}

          <div className="section-heading"><h2>项目成员</h2><span>接入后可创建独立会话</span></div>
          <div className="agent-grid">
            <article className="agent-card"><div className="agent-top"><span className="agent-icon codex">C</span><span className="muted-tag">未接入</span></div><h3>Codex</h3><p>实现需求、修改代码、执行验证。</p><button disabled>新建私聊 <span aria-hidden="true">→</span></button></article>
            <article className="agent-card"><div className="agent-top"><span className="agent-icon deepseek">D</span><span className="muted-tag">未接入</span></div><h3>DeepSeek</h3><p>拆解任务、检查交付、提出修改。</p><button disabled>新建私聊 <span aria-hidden="true">→</span></button></article>
          </div>
          <div className="workflow-card"><div className="workflow-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M4 8h15m-4-4 4 4-4 4M20 16H5m4-4-4 4 4 4" /></svg></div><div><h3>让两位成员一起完成任务</h3><p>拆解 → 实现 → 检查 → 修改</p></div><span className="muted-tag">协作群 · 待开放</span></div>
        </div>
        <footer>记录保存在本机 <span>·</span> Agent 尚未连接</footer>
      </main>

      <aside className="inspector" aria-label="执行与证据">
        <h2>执行与证据</h2><p className="inspector-subtitle">每一步工作，都有记录。</p>
        <div className="empty-evidence"><span aria-hidden="true"><svg viewBox="0 0 40 48" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M9 4h15l9 9v30H9Z" /><path d="M24 4v10h9M15 23h12M15 29h12M15 35h7" /></svg></span><h3>等待第一份工作记录</h3><p>任务开始后，在这里查看<br />执行状态、代码差异和交付。</p></div>
        <div className="foundation"><h3>当前框架</h3><div><span className="status-dot" />桌面界面<span>已建立</span></div><div><span className={`status-dot ${info ? "" : "pending"}`} />Rust 通信<span>{info ? "已连接" : "未连接"}</span></div><div><span className={`status-dot ${info ? "" : "pending"}`} />SQLite<span>{info ? "已初始化" : "未连接"}</span></div><div><span className="status-dot pending" />Agent 执行<span>待接入</span></div></div>
        {info && <details className="storage-details"><summary>本地数据位置</summary><code>{info.database_path}</code></details>}
      </aside>
    </div>
  );
}
