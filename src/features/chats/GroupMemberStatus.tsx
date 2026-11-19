import Avatar from "../../components/Avatar";
import { running, statusLabels, type Snapshot } from "./api";

export default function GroupMemberStatus({ snapshot, members }: { snapshot: Snapshot; members: string[] }) {
  const task = snapshot.task;
  if (!task?.members?.length) return null;
  return <div className="group-member-statuses" aria-label="成员执行状态">
    {task.members.filter(member => members.includes(member.agent)).map(member => {
      const name = member.agent === "codex" ? "Codex" : "DeepSeek";
      const session = snapshot.sessions?.[member.agent];
      const status = member.status === "running" ? session?.status ?? member.status : member.status;
      const active = running(status);
      const label = task.status === "cancelling" && active ? "正在停止" : task.status === "paused" && !active && member.queued_count ? "已暂停" : status === "blocked" ? "执行未完成" : statusLabels[status] ?? status;
      const error = member.error ?? (member.status === "running" ? session?.error : null);
      return <div key={member.agent} className={`member-execution ${active ? "active" : ""} ${error ? "has-error" : ""}`} role="status" aria-label={`${name} 执行状态`}>
        <Avatar agent={member.agent} small /><strong>{name}</strong><span>{label}</span>
        {member.queued_count > 0 && <small>{member.queued_count} 项待处理</small>}
        {error && <span className="member-execution-error">{error}</span>}
      </div>;
    })}
  </div>;
}
