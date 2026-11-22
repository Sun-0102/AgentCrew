import { useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import Dialog from "../../components/Dialog";
import { agents, updateGroupMembers, type Group } from "./api";

export default function GroupMembers({ group, onUpdated, onSettings }: { group: Group; onUpdated: (group: Group) => void; onSettings: () => void }) {
  const [adding, setAdding] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const available = agents.filter(agent => !group.members.includes(agent.id));
  async function update(add: string[], remove: string[]) {
    if (saving) return;
    setSaving(true); setError("");
    try { const updated = await updateGroupMembers(group.id, add, remove); onUpdated(updated); setAdding(false); setSelected([]); }
    catch (cause) { setError(String(cause)); } finally { setSaving(false); }
  }
  return <>
    <p className="panel-caption">{group.members.length} 位 Agent · 1 位成员</p>
    <div className="person-row"><Avatar agent="me" small /><div><strong>我</strong><small>群主 · 本机用户</small></div><span className="availability ready">你</span></div>
    {group.members.map(id => <div className="person-row" key={id}><Avatar agent={id} small /><div><strong>{agents.find(a => a.id === id)?.name ?? id}</strong><small>{agents.find(a => a.id === id)?.description}</small></div><button className="member-remove" disabled={saving || group.members.length <= 1} title={group.members.length <= 1 ? "群内至少保留一位 Agent" : "从整个群移出，历史消息保留"} onClick={() => update([], [id])}>移出</button></div>)}
    {!adding && error && <p className="inline-error" role="alert">{error}</p>}
    <button className="outline-button panel-add" disabled={saving || !available.length} onClick={() => { setSelected([]); setError(""); setAdding(true); }}><Icon name="plus" size={16} />添加成员</button>
    {!available.length && <p className="panel-caption">已加入全部可用 Agent。</p>}
    <button className="outline-button panel-add" onClick={onSettings}><Icon name="settings" size={16} />群设置与项目</button>
    <div className="panel-note"><Icon name="shield" /><p>成员调整作用于整个群。移出后历史消息保留；群内有任务执行或暂停时，先完成或停止任务。</p></div>
    {adding && <Dialog title="添加群成员" onClose={() => { if (!saving) setAdding(false); }}>
      <p className="dialog-intro">选择本机已支持的 Agent，加入后可参与本群各会话。</p>
      <div className="member-options">{available.map(agent => <label className={`member-option ${selected.includes(agent.id) ? "checked" : ""}`} key={agent.id}><Avatar agent={agent.id} /><span className="member-option-text"><strong>{agent.name}</strong><small>{agent.description}</small></span><input type="checkbox" disabled={saving} checked={selected.includes(agent.id)} onChange={event => setSelected(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} /><span className="checkbox-mark"><Icon name="check" size={13} /></span></label>)}</div>
      {error && <p className="inline-error" role="alert">{error}</p>}
      <div className="dialog-footer"><span>保留现有聊天记录</span><button className="primary" disabled={saving || !selected.some(id => available.some(agent => agent.id === id))} onClick={() => update(selected.filter(id => available.some(agent => agent.id === id)), [])}>{saving ? "正在添加…" : "添加成员"}</button></div>
    </Dialog>}
  </>;
}
