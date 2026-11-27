import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import Dialog from "../../components/Dialog";
import { clock, deleteGroupSession, deletePrivateSession, deleteProjectSession, saveGroupSession, savePrivateSession, saveProjectSession, taskLabels, type Group, type GroupSession, type PrivateSession, type ProjectSession } from "./api";
import type { Project } from "../projects/api";

export type ChatSession = GroupSession | PrivateSession | ProjectSession;

function time(value: string | null) {
  if (!value) return "";
  const date = new Date(value);
  return date.toDateString() === new Date().toDateString() ? clock(value) : date.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" });
}

function draftKey(session: ChatSession) {
  if ("group_id" in session) {
    return session.id === session.group_id ? `group:${session.group_id}` : `group:${session.group_id}:${session.id}`;
  }
  if ("project_id" in session) {
    return session.is_default ? `project:${session.project_id}` : `project:${session.project_id}:${session.id}`;
  }
  return session.is_default ? `private:${session.agent_id}` : `private:${session.agent_id}:${session.id}`;
}

function preview(session: ChatSession) {
  const key = draftKey(session);
  try { const draft = localStorage.getItem(`ai-gent.draft.${key}`); if (draft) return `草稿 · ${draft}`; } catch { /* The composer reports storage failures. */ }
  return session.status && ["running", "paused", "cancelling"].includes(session.status) ? taskLabels[session.status] : session.preview ?? "等待第一条消息";
}

export function GroupSidebarList({ groups, projects, selectedId, onSelect, onSettings, onDelete }: { groups: Group[]; projects: Project[]; selectedId?: string; onSelect: (id: string) => void; onSettings: (group: Group) => void; onDelete: (group: Group) => void }) {
  const [menu, setMenu] = useState<{ id: string; top: number; left: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const click = (e: MouseEvent) => { if (!menuRef.current?.contains(e.target as Node) && !(e.target as Element).closest?.(".group-row-menu")) close(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { close(); triggerRef.current?.focus(); } };
    menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    document.addEventListener("click", click); document.addEventListener("keydown", key); window.addEventListener("resize", close); window.addEventListener("scroll", close, true);
    return () => { document.removeEventListener("click", click); document.removeEventListener("keydown", key); window.removeEventListener("resize", close); window.removeEventListener("scroll", close, true); };
  }, [menu]);
  const target = groups.find(g => g.id === menu?.id);
  return <div className="group-sidebar-list">{groups.map(g => <div className={`group-sidebar-row ${g.id === selectedId ? "active" : ""}`} key={g.id}>
    <button className="group-sidebar-select" aria-pressed={g.id === selectedId} onClick={() => { setMenu(null); onSelect(g.id); }}><Avatar agent="group" /><span className="group-sidebar-copy"><strong title={g.name}>{g.name}</strong><small>{g.project_id ? `关联项目 · ${projects.find(p => p.id === g.project_id)?.name ?? "本地项目"}` : "未关联项目 · 群专用目录"}</small></span><span className="group-sidebar-count">{g.session_count ?? 1}</span></button>
    <button className="group-row-menu" aria-label={`${g.name}的群操作`} title="群操作" aria-expanded={menu?.id === g.id} aria-controls={menu?.id === g.id ? "group-row-actions" : undefined} onClick={e => { triggerRef.current = e.currentTarget; const rect = e.currentTarget.getBoundingClientRect(); setMenu(menu?.id === g.id ? null : { id: g.id, top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - 110)), left: Math.max(8, Math.min(rect.right - 155, window.innerWidth - 163)) }); }}><Icon name="more" size={16} /></button>
  </div>)}{menu && target && createPortal(<div id="group-row-actions" className="session-actions session-row-actions" ref={menuRef} style={{ top: menu.top, left: menu.left }} aria-label="群操作"><button onClick={() => { setMenu(null); onSettings(target); }}>群设置</button><hr /><button className="destructive-action" onClick={() => { setMenu(null); onDelete(target); }}>删除群</button></div>, document.body)}</div>;
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

export function SessionSidebar<T extends ChatSession>({ sessions, selected, isDraft = false, loading, saving, searchPlaceholder = "搜索本群会话", bottomNote = "会话独立 · 项目共享", onSelect, onCreate, onRename, onArchive, onDelete, onCollapse, resizing, onResizeStart, onResizeReset }: { sessions: T[]; selected?: T; isDraft?: boolean; loading: boolean; saving: boolean; searchPlaceholder?: string; bottomNote?: string; onSelect: (session: T) => void; onCreate: () => void; onRename: (session: T, nextName: string) => Promise<void>; onArchive: (session: T) => void; onDelete: (session: T) => void; onCollapse?: () => void; resizing?: boolean; onResizeStart?: (event: ReactMouseEvent<HTMLDivElement>) => void; onResizeReset?: () => void }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"active" | "scheduled" | "archived">(
    selected?.archived ? "archived" : selected?.is_scheduled ? "scheduled" : "active",
  );
  const [menu, setMenu] = useState<{ id: string; top: number; left: number } | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [editingSaving, setEditingSaving] = useState(false);
  const editingInputRef = useRef<HTMLInputElement>(null);
  const committingRenameRef = useRef(false);
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
  useEffect(() => {
    if (isDraft) {
      setFilter("active");
      return;
    }
    setFilter(selected?.archived ? "archived" : selected?.is_scheduled ? "scheduled" : "active");
  }, [isDraft, selected?.id, selected?.archived, selected?.is_scheduled]);
  useEffect(() => {
    if (!editingId) return;
    const frame = requestAnimationFrame(() => {
      editingInputRef.current?.focus();
      editingInputRef.current?.select();
    });
    return () => cancelAnimationFrame(frame);
  }, [editingId]);
  function startInlineRename(session: T) {
    setMenu(null);
    committingRenameRef.current = false;
    setEditingId(session.id);
    setEditingName(session.name);
  }
  function cancelInlineRename() {
    committingRenameRef.current = false;
    setEditingId(null);
    setEditingName("");
  }
  async function commitInlineRename(session: T) {
    if (committingRenameRef.current) return;
    const trimmed = editingName.trim();
    if (!trimmed || trimmed === session.name) {
      cancelInlineRename();
      return;
    }
    committingRenameRef.current = true;
    setEditingSaving(true);
    try {
      await onRename(session, trimmed);
      setEditingId(null);
      setEditingName("");
    } catch {
      // Error is reported by parent; keep inline editor open so user can adjust.
      requestAnimationFrame(() => editingInputRef.current?.focus());
    } finally {
      committingRenameRef.current = false;
      setEditingSaving(false);
    }
  }
  const activeCount = sessions.filter(s => !s.archived && !s.is_scheduled).length;
  const scheduledCount = sessions.filter(s => !s.archived && !!s.is_scheduled).length;
  const filtered = sessions.filter(s => {
    if (filter === "archived" ? !s.archived : filter === "scheduled" ? (s.archived || !s.is_scheduled) : (s.archived || !!s.is_scheduled)) {
      return false;
    }
    return s.name.toLowerCase().includes(query.trim().toLowerCase());
  });
  const showDraftRow = isDraft && filter === "active" && !query.trim();
  const menuSession = filtered.find(s => s.id === menu?.id);
  const menuBusy = !!menuSession?.status && ["running", "paused", "cancelling"].includes(menuSession.status);
  return <aside className="group-session-sidebar" aria-label="会话列表">
    <div className="session-heading"><h2>会话 <span className="session-count">{activeCount}</span></h2>{onCollapse ? <button type="button" className="sidebar-collapse-btn" title="收起会话列表" aria-label="收起会话列表" onClick={onCollapse}><Icon name="chevronsLeft" size={15} /></button> : <Icon name="chat" size={18} />}</div>
    <button className="session-new" onClick={onCreate}><Icon name="plus" size={18} />新建会话</button>
    <label className="session-search"><Icon name="search" size={15} /><input value={query} onChange={e => setQuery(e.target.value)} placeholder={searchPlaceholder} aria-label={searchPlaceholder} /></label>
    <div className="session-filters">
      <button className={filter === "active" ? "active" : ""} aria-pressed={filter === "active"} onClick={() => setFilter("active")}>进行中</button>
      {(scheduledCount > 0 || filter === "scheduled") && (
        <button className={filter === "scheduled" ? "active" : ""} aria-pressed={filter === "scheduled"} onClick={() => setFilter("scheduled")}>定时{scheduledCount > 0 ? ` ${scheduledCount}` : ""}</button>
      )}
      <button className={filter === "archived" ? "active" : ""} aria-pressed={filter === "archived"} onClick={() => setFilter("archived")}>已归档</button>
    </div>
    <div className="session-list">
      {showDraftRow && (
        <div className="session-row active is-draft" aria-current="page">
          <div className="session-select">
            <span className="session-icon"><Icon name="plus" size={15} /></span>
            <span className="session-copy">
              <strong>新会话</strong>
              <small>发送首条消息后自动命名</small>
            </span>
          </div>
        </div>
      )}
      {filtered.map(session => {
        const isEditing = editingId === session.id;
        return <div className={`session-row ${selected?.id === session.id ? "active" : ""}`} key={session.id}>
          {isEditing ? (
            <div className="session-select is-editing">
              <span className="session-icon"><Icon name={session.is_scheduled ? "clock" : "chat"} size={15} /></span>
              <span className="session-copy">
                <input
                  ref={editingInputRef}
                  className="session-inline-input"
                  aria-label="重命名会话"
                  maxLength={40}
                  value={editingName}
                  disabled={editingSaving}
                  onChange={e => setEditingName(e.target.value)}
                  onBlur={() => { if (!editingSaving) void commitInlineRename(session); }}
                  onKeyDown={e => {
                    if (e.nativeEvent.isComposing) return;
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void commitInlineRename(session);
                    } else if (e.key === "Escape") {
                      e.preventDefault();
                      cancelInlineRename();
                    }
                  }}
                />
                <small>{preview(session)}</small>
              </span>
            </div>
          ) : (
            <button className="session-select" aria-pressed={selected?.id === session.id} onClick={() => { setMenu(null); onSelect(session); }} onDoubleClick={() => { if (!saving) startInlineRename(session); }}>
              <span className="session-icon"><Icon name={session.is_scheduled ? "clock" : "chat"} size={15} /></span>
              <span className="session-copy"><strong title={`${session.name}（双击可重命名）`}>{session.name}</strong><small>{preview(session)}</small></span>
              <time>{time(session.updated_at ?? session.created_at)}</time>
            </button>
          )}
          <button className="session-row-menu" aria-label={`${session.name}的操作`} aria-expanded={menu?.id === session.id} disabled={saving || isEditing} onClick={e => { menuTrigger.current = e.currentTarget; const rect = e.currentTarget.getBoundingClientRect(); setMenu(menu?.id === session.id ? null : { id: session.id, top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - 160)), left: Math.max(8, Math.min(rect.right - 155, window.innerWidth - 163)) }); }}><Icon name="more" size={16} /></button>
        </div>;
      })}
      {loading ? <p className="session-empty" role="status">正在读取会话…</p> : !filtered.length && !showDraftRow && <p className="session-empty">{query ? "没有找到相关会话" : filter === "archived" ? "还没有归档会话" : filter === "scheduled" ? "还没有定时任务会话" : "还没有会话，创建一个吧"}</p>}
    </div><div className="session-bottom"><Icon name="shield" size={15} />{bottomNote}</div>
    {menu && menuSession && createPortal(<div className="session-actions session-row-actions" ref={menuRef} style={{ top: menu.top, left: menu.left }} aria-label="会话操作"><button disabled={saving} onClick={() => startInlineRename(menuSession)}>重命名</button><button disabled={saving || menuBusy} title={menuBusy ? "请先完成或停止任务" : undefined} onClick={() => { setMenu(null); onArchive(menuSession); }}>{menuSession.archived ? "恢复会话" : "归档会话"}</button><hr /><button className="destructive-action" disabled={saving || menuBusy} title={menuBusy ? "请先完成或停止任务" : undefined} onClick={() => { setMenu(null); onDelete(menuSession); }}>删除会话</button></div>, document.body)}
    {onResizeStart && <div className={`sidebar-resize-handle ${resizing ? "is-resizing" : ""}`} role="separator" aria-orientation="vertical" aria-label="调整会话侧栏宽度" title="拖动调整宽度，双击恢复默认" onMouseDown={onResizeStart} onDoubleClick={onResizeReset} />}
  </aside>;
}

export function DeleteSessionDialog({ session, onClose, onDeleted }: { session: ChatSession; onClose: () => void; onDeleted: (session: ChatSession) => void }) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  return <Dialog title="删除会话" onClose={() => { if (!saving) onClose(); }}>
    <p className="session-dialog-intro">确定删除「{session.name}」吗？</p>
    <p className="delete-session-description">将永久删除此会话在本应用中的消息、摘要、任务结果和草稿，无法撤销。其他会话保留。项目文件、图片缓存和原厂会话文件不会删除。</p>
    {error && <p className="inline-error" role="alert">{error}</p>}
    <div className="dialog-footer"><span /><div className="session-dialog-actions"><button className="outline-button" disabled={saving} onClick={onClose}>取消</button><button className="primary danger-button" disabled={saving} onClick={async () => { if (saving) return; setSaving(true); setError(""); try { if ("group_id" in session) await deleteGroupSession(session.group_id, session.id); else if ("project_id" in session) await deleteProjectSession(session.project_id, session.id); else await deletePrivateSession(session.agent_id, session.id); onDeleted(session); } catch (cause) { setError(String(cause)); setSaving(false); } }}>{saving ? "正在删除…" : "确认删除"}</button></div></div>
  </Dialog>;
}

export function SessionDialog({ groupId, agentId, projectId, existing, onClose, onSaved }: { groupId?: string; agentId?: string; projectId?: string; existing?: ChatSession; onClose: () => void; onSaved: (session: ChatSession) => void }) {
  const [name, setName] = useState(existing?.name ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  return <Dialog title={existing ? "重命名会话" : "新建会话"} onClose={onClose}>
    <p className="session-dialog-intro">{existing ? "换一个更容易找到的名称，聊天记录保持不变。" : "给这个需求起个名字。新会话从空上下文开始。"}</p>
    <form onSubmit={async e => { e.preventDefault(); if (saving || !name.trim()) return; setSaving(true); setError(""); try { const saved = groupId ? await saveGroupSession(groupId, name, existing as GroupSession | undefined) : projectId ? await saveProjectSession(projectId, name, existing as ProjectSession | undefined) : await savePrivateSession(agentId!, name, existing as PrivateSession | undefined); if (mounted.current) onSaved(saved); } catch (cause) { if (mounted.current) setError(String(cause)); } finally { if (mounted.current) setSaving(false); } }}>
      <label className="field-label" htmlFor="session-name">会话名称</label><input id="session-name" className="text-input" autoFocus required maxLength={40} placeholder="例如：登录功能、首页改版" value={name} disabled={saving} onChange={e => setName(e.target.value)} />
      {error && <p className="inline-error" role="alert">{error}</p>}
      <div className="dialog-footer"><span /><div className="session-dialog-actions"><button type="button" className="outline-button" disabled={saving} onClick={onClose}>取消</button><button className="primary" disabled={saving || !name.trim()} type="submit">{saving ? "正在保存…" : existing ? "保存名称" : "创建会话"}</button></div></div>
    </form>
  </Dialog>;
}
