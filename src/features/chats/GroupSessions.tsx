import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import Dialog from "../../components/Dialog";
import { clock, saveGroupSession, taskLabels, type Group, type GroupSession } from "./api";
import type { Project } from "../projects/api";

function time(value: string | null) {
  if (!value) return "";
  const date = new Date(value);
  return date.toDateString() === new Date().toDateString() ? clock(value) : date.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" });
}

function preview(session: GroupSession) {
  const key = session.id === session.group_id ? `group:${session.group_id}` : `group:${session.group_id}:${session.id}`;
  try { const draft = localStorage.getItem(`ai-gent.draft.${key}`); if (draft) return `草稿 · ${draft}`; } catch { /* The composer reports storage failures. */ }
  return session.status && ["running", "paused", "cancelling"].includes(session.status) ? taskLabels[session.status] : session.preview ?? "等待第一条消息";
}

export function GroupHub({ groups, projects, onSelect, onCreate }: { groups: Group[]; projects: Project[]; onSelect: (group: Group) => void; onCreate: () => void }) {
  const [query, setQuery] = useState("");
  const projectName = (group: Group) => projects.find(p => p.id === group.project_id)?.name ?? (group.project_id ? "关联项目" : "未关联项目");
  const filtered = groups.filter(g => `${g.name} ${projectName(g)}`.toLowerCase().includes(query.toLowerCase().trim()));
  return <section className="group-hub" aria-label="所有群聊"><div className="hub-inner">
    <div className="hub-eyebrow">我的工作台 / 群聊</div><div className="hub-header"><h1>协作群<span className="hub-count">{groups.length}</span></h1><button className="primary" onClick={onCreate}><Icon name="plus" size={15} />创建协作群</button></div>
    <p className="hub-subtitle">选择一个群，与成员围绕不同需求展开会话。</p>
    <label className="hub-search"><Icon name="search" /><input aria-label="搜索群聊" placeholder="搜索群名称或关联项目" value={query} onChange={e => setQuery(e.target.value)} /></label>
    <div className="hub-table-heading"><span>群聊</span><span>成员</span><span>进行中的会话</span><span /></div>
    <div>{filtered.map(group => <button className="hub-group" key={group.id} onClick={() => onSelect(group)} aria-label={`进入${group.name}`}><span className="hub-name"><Avatar agent="group" /><span><strong>{group.name}</strong><small>{group.project_id ? `关联项目 · ${projectName(group)}` : "未关联项目 · 群专用目录"}</small></span></span><span className="hub-members">{group.members.map(id => <Avatar agent={id} key={id} small />)}<Avatar agent="me" small /></span><span className="hub-number">{group.session_count ?? 1} 个会话</span><span className="hub-enter">进入群聊<Icon name="right" size={15} /></span></button>)}</div>
    {!filtered.length && <div className="hub-empty">{query ? "没有找到相关群聊" : "还没有协作群，创建一个开始讨论吧。"}</div>}
    <div className="hub-footer"><Icon name="shield" size={16} />同一个群共享成员与项目，每个会话保留独立上下文。</div>
  </div></section>;
}

export function SessionSidebar({ sessions, selected, loading, saving, onSelect, onCreate, onRename, onArchive }: { sessions: GroupSession[]; selected?: GroupSession; loading: boolean; saving: boolean; onSelect: (session: GroupSession) => void; onCreate: () => void; onRename: (session: GroupSession) => void; onArchive: (session: GroupSession) => void }) {
  const [query, setQuery] = useState("");
  const [archived, setArchived] = useState(selected?.archived ?? false);
  const [menu, setMenu] = useState<{ id: string; top: number; left: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuTrigger = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const click = (e: MouseEvent) => { if (!menuRef.current?.contains(e.target as Node) && !(e.target as Element).closest?.(".session-row-menu")) close(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { close(); menuTrigger.current?.focus(); } };
    menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    document.addEventListener("click", click); document.addEventListener("keydown", key); window.addEventListener("resize", close); window.addEventListener("scroll", close, true);
    return () => { document.removeEventListener("click", click); document.removeEventListener("keydown", key); window.removeEventListener("resize", close); window.removeEventListener("scroll", close, true); };
  }, [menu]);
  useEffect(() => { setArchived(selected?.archived ?? false); }, [selected?.id, selected?.archived]);
  const filtered = sessions.filter(s => s.archived === archived && s.name.toLowerCase().includes(query.trim().toLowerCase()));
  const menuSession = filtered.find(s => s.id === menu?.id);
  const menuBusy = !!menuSession?.status && ["running", "paused", "cancelling"].includes(menuSession.status);
  return <aside className="group-session-sidebar" aria-label="群内会话">
    <div className="session-heading"><h2>会话 <span className="session-count">{sessions.filter(s => !s.archived).length}</span></h2><Icon name="chat" size={18} /></div>
    <button className="session-new" onClick={onCreate}><Icon name="plus" size={18} />新建会话</button>
    <label className="session-search"><Icon name="search" size={15} /><input value={query} onChange={e => setQuery(e.target.value)} placeholder="搜索本群会话" aria-label="搜索本群会话" /></label>
    <div className="session-filters"><button className={!archived ? "active" : ""} aria-pressed={!archived} onClick={() => setArchived(false)}>进行中</button><button className={archived ? "active" : ""} aria-pressed={archived} onClick={() => setArchived(true)}>已归档</button></div>
    <div className="session-list">{filtered.map(session => <div className={`session-row ${selected?.id === session.id ? "active" : ""}`} key={session.id}><button className="session-select" aria-pressed={selected?.id === session.id} onClick={() => { setMenu(null); onSelect(session); }}><span className="session-icon"><Icon name="chat" size={15} /></span><span className="session-copy"><strong title={session.name}>{session.name}</strong><small>{preview(session)}</small></span><time>{time(session.updated_at ?? session.created_at)}</time></button><button className="session-row-menu" aria-label={`${session.name}的操作`} aria-expanded={menu?.id === session.id} disabled={saving} onClick={e => { menuTrigger.current = e.currentTarget; const rect = e.currentTarget.getBoundingClientRect(); setMenu(menu?.id === session.id ? null : { id: session.id, top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - 160)), left: Math.max(8, Math.min(rect.right - 155, window.innerWidth - 163)) }); }}><Icon name="more" size={16} /></button></div>)}
      {loading ? <p className="session-empty" role="status">正在读取会话…</p> : !filtered.length && <p className="session-empty">{query ? "没有找到相关会话" : archived ? "还没有归档会话" : "还没有会话，创建一个吧"}</p>}
    </div><div className="session-bottom"><Icon name="shield" size={15} />会话独立 · 项目共享</div>
    {menu && menuSession && createPortal(<div className="session-actions session-row-actions" ref={menuRef} style={{ top: menu.top, left: menu.left }} aria-label="会话操作"><button disabled={saving} onClick={() => { setMenu(null); onRename(menuSession); }}>重命名</button><button disabled={saving || menuBusy} title={menuBusy ? "请先完成或停止任务" : undefined} onClick={() => { setMenu(null); onArchive(menuSession); }}>{menuSession.archived ? "恢复会话" : "归档会话"}</button></div>, document.body)}
  </aside>;
}

export function SessionDialog({ groupId, existing, onClose, onSaved }: { groupId: string; existing?: GroupSession; onClose: () => void; onSaved: (session: GroupSession) => void }) {
  const [name, setName] = useState(existing?.name ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  return <Dialog title={existing ? "重命名会话" : "新建会话"} onClose={onClose}>
    <p className="session-dialog-intro">{existing ? "换一个更容易找到的名称，聊天记录保持不变。" : "给这个需求起个名字。新会话从空上下文开始。"}</p>
    <form onSubmit={async e => { e.preventDefault(); if (saving || !name.trim()) return; setSaving(true); setError(""); try { const saved = await saveGroupSession(groupId, name, existing); if (mounted.current) onSaved(saved); } catch (cause) { if (mounted.current) setError(String(cause)); } finally { if (mounted.current) setSaving(false); } }}>
      <label className="field-label" htmlFor="session-name">会话名称</label><input id="session-name" className="text-input" autoFocus required maxLength={40} placeholder="例如：登录功能、首页改版" value={name} disabled={saving} onChange={e => setName(e.target.value)} />
      {error && <p className="inline-error" role="alert">{error}</p>}
      <div className="dialog-footer"><span /><div className="session-dialog-actions"><button type="button" className="outline-button" disabled={saving} onClick={onClose}>取消</button><button className="primary" disabled={saving || !name.trim()} type="submit">{saving ? "正在保存…" : existing ? "保存名称" : "创建会话"}</button></div></div>
    </form>
  </Dialog>;
}
