import { useState } from "react";
import Dialog from "../../components/Dialog";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import { useAgents, saveGroup, deleteGroup, getGroupSessions, type Group, type GroupSession } from "./api";
import type { Project } from "../projects/api";
export default function GroupDialog({ existing, projects, onSaved, onDeleted, onClose }: { existing?: Group; projects: Project[]; onSaved: (group: Group) => void; onDeleted: (group: Group, sessions: GroupSession[]) => void; onClose: () => void }) {
  const agents = useAgents().filter(agent => agent.available);
  const [name, setName] = useState(existing?.name ?? "");
  const [members, setMembers] = useState(existing?.members ?? (agents[0] ? [agents[0].id] : []));
  const [projectId, setProjectId] = useState(existing?.project_id ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const busy = saving || deleting;
  const close = () => { if (!busy) onClose(); };
  async function save() {
    if (busy || confirmDelete || !name.trim() || !members.length) return;
    setSaving(true); setError(null);
    try { onSaved(await saveGroup(name, members, projectId || null, existing?.id)); }
    catch (cause) { setError(String(cause)); }
    finally { setSaving(false); }
  }
  async function remove() {
    if (!existing || busy) return;
    setDeleting(true); setError(null);
    try {
      const sessions = await getGroupSessions(existing.id);
      await deleteGroup(existing.id);
      onDeleted(existing, sessions);
    } catch (cause) { setError(String(cause)); setDeleting(false); }
  }
  if (existing && confirmDelete) return <Dialog title="删除群" onClose={close}>
    <p className="dialog-intro">确认删除「{existing.name}」？此操作无法撤销。</p>
    <p className="field-help">将删除群内全部会话、消息、摘要和任务记录。保留项目文件、群工作目录、图片缓存及原厂会话文件。</p>
    <p className="field-help">请先完成或取消群内所有任务，包括暂停或停止中的任务。</p>
    {error && <div className="inline-error" role="alert">{error}</div>}
    <div className="dialog-footer"><span /><div className="session-dialog-actions"><button type="button" className="outline-button" disabled={busy} onClick={() => { setConfirmDelete(false); setError(null); }}>返回</button><button type="button" className="primary danger-button" disabled={busy} onClick={() => void remove()}>{deleting ? "正在删除…" : "确认删除群"}</button></div></div>
  </Dialog>;
  return <Dialog title={existing ? "群聊设置" : "创建协作群"} onClose={close}>
    <div className="invite-type"><Icon name="shield" size={18} /><span>你自己的 Agent</span><span className="label-chip">本机</span></div>
    <form onSubmit={event => { event.preventDefault(); void save(); }}>
      <label className="field-label" htmlFor="group-name">群名称</label><input id="group-name" className="text-input" value={name} onChange={e => setName(e.target.value)} maxLength={40} placeholder="例如：产品协作群" autoFocus required />
      <div className="field-caption"><span>选择 Agent</span><span>已选 {members.length} 位</span></div>
      <div className="member-options">{agents.map(agent => <label className={`member-option ${members.includes(agent.id) ? "checked" : ""}`} key={agent.id}><Avatar agent={agent.id} /><span className="member-option-text"><strong>{agent.name}</strong><small>{agent.description}</small></span><span className={`availability ${agent.available ? "ready" : ""}`}>{agent.available ? "可连接" : "待接入"}</span><input type="checkbox" checked={members.includes(agent.id)} onChange={event => setMembers(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} /><span className="checkbox-mark"><Icon name="check" size={13} /></span></label>)}</div>
      <p className="field-help">不写 @ 时群内成员都能收到消息；收到具体任务后协商分工。@ 谁就只交给谁。</p>
      <label className="field-label" htmlFor="group-project">关联项目 <span>可选</span></label><select id="group-project" className="text-input" value={projectId} onChange={e => setProjectId(e.target.value)}><option value="">不关联项目，使用群专用目录</option>{existing?.project_id && !projects.some(p => p.id === existing.project_id) && <option value={existing.project_id}>原关联项目（请检查项目列表）</option>}{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select>
      {!projects.length && <p className="field-help">先在“工作空间”添加本地项目，再回来选择。</p>}
      {existing && projectId !== (existing.project_id ?? "") && <p className="field-help">保存后保留本群全部聊天记录，后续任务改在新目录执行。两位 Agent 会使用新的执行会话；旧文件留在原目录，需要旧任务内容时可引用群消息。运行中或暂停的协作请先结束或取消。</p>}
      {error && <div className="inline-error" role="alert">{error}</div>}
      <div className="dialog-footer">{existing ? <button type="button" className="outline-button danger-button" disabled={busy} onClick={() => { setConfirmDelete(true); setError(null); }}>删除群</button> : <span>你也会自动加入群聊</span>}<button className="primary" disabled={busy || !name.trim() || !members.length} type="submit">{saving ? "正在保存…" : existing ? "保存" : "创建群聊"}<Icon name="right" size={16} /></button></div>
    </form>
  </Dialog>;
}
