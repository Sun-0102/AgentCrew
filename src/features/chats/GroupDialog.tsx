import { useState } from "react";
import Dialog from "../../components/Dialog";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import { agents, saveGroup, type Group } from "./api";
import type { Project } from "../projects/api";
export default function GroupDialog({ existing, projects, onSaved, onClose }: { existing?: Group; projects: Project[]; onSaved: (group: Group) => void; onClose: () => void }) {
  const [name, setName] = useState(existing?.name ?? "");
  const [members, setMembers] = useState(existing?.members ?? ["codex"]);
  const [projectId, setProjectId] = useState(existing?.project_id ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  async function save() {
    if (!name.trim() || !members.length) return;
    setSaving(true); setError(null);
    try { onSaved(await saveGroup(name, members, projectId || null, existing?.id)); }
    catch (cause) { setError(String(cause)); }
    finally { setSaving(false); }
  }
  return <Dialog title={existing ? "邀请本机 Agent" : "创建协作群"} onClose={onClose}>
    <div className="invite-type"><Icon name="shield" size={18} /><span>你自己的 Agent</span><span className="label-chip">本机</span></div>
    <form onSubmit={event => { event.preventDefault(); void save(); }}>
      <label className="field-label" htmlFor="group-name">群名称</label><input id="group-name" className="text-input" value={name} onChange={e => setName(e.target.value)} maxLength={40} placeholder="例如：产品协作群" autoFocus required />
      <div className="field-caption"><span>选择 Agent</span><span>已选 {members.length} 位</span></div>
      <div className="member-options">{agents.map(agent => <label className={`member-option ${members.includes(agent.id) ? "checked" : ""}`} key={agent.id}><Avatar agent={agent.id} /><span className="member-option-text"><strong>{agent.name}</strong><small>{agent.description}</small></span><span className={`availability ${agent.available ? "ready" : ""}`}>{agent.available ? "可连接" : "待接入"}</span><input type="checkbox" checked={members.includes(agent.id)} onChange={event => setMembers(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} /><span className="checkbox-mark"><Icon name="check" size={13} /></span></label>)}</div>
      <p className="field-help">DeepSeek 目前仅支持独立私聊；群聊执行暂未开放。</p>
      {!existing && <><label className="field-label" htmlFor="group-project">关联项目 <span>可选</span></label><select id="group-project" className="text-input" value={projectId} onChange={e => setProjectId(e.target.value)}><option value="">不关联项目，先聊聊</option>{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></>}
      {error && <div className="inline-error" role="alert">{error}</div>}
      <div className="dialog-footer"><span>你也会自动加入群聊</span><button className="primary" disabled={saving || !name.trim() || !members.length} type="submit">{saving ? "正在保存…" : existing ? "保存" : "创建群聊"}<Icon name="right" size={16} /></button></div>
    </form>
  </Dialog>;
}
