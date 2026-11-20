import { useEffect, useState } from "react";
import Icon from "../../components/Icon";
import MessageBody from "./MessageBody";
import { clock } from "./api";
import { elapsed, type ReplyBlock } from "./activity";

const outcomes: Record<string, string> = { cancelled: "已停止", failed: "执行失败", interrupted: "执行中断" };

export default function TaskActivity({ block }: { block: ReplyBlock }) {
  const [open, setOpen] = useState(block.active);
  const [now, setNow] = useState(Date.now);
  useEffect(() => { setOpen(block.active); }, [block.active]);
  useEffect(() => {
    if (!block.active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [block.active]);
  const tools = block.activity.filter(m => m.role === "tool");
  const commands = tools.filter(m => m.text.startsWith("命令：")).length;
  const changes = tools.filter(m => m.text.startsWith("文件变更：")).length;
  const duration = !block.active && !block.endedAt ? "耗时未知" : elapsed(block.startedAt, block.active ? null : block.endedAt, now);
  const durationText = duration === "耗时未知" ? duration : `用时${!block.active && !block.outcome ? "约" : " "}${duration.replace(" 分 ", "分钟").replace(" 秒", "秒")}`;
  const summary = [commands && `${commands} 次命令`, changes && `${changes} 次文件修改`, tools.length > commands + changes && `${tools.length - commands - changes} 条工具记录`].filter(Boolean).join(" · ");
  return <div className={`task-activity ${block.active ? "is-active" : ""}`}>
    <button type="button" className="task-activity-toggle" aria-expanded={open} aria-controls={`activity-${block.id}`} onClick={() => setOpen(!open)}>
      <span>{block.active ? "正在处理 · " : outcomes[block.outcome ?? ""] ? `${outcomes[block.outcome!]} · ` : ""}{durationText}</span><Icon name={open ? "down" : "right"} size={14} />
    </button>
    {open && <div className="task-activity-items" id={`activity-${block.id}`}>
      {summary && <p className="task-activity-summary">{summary}</p>}
      {block.activity.length ? block.activity.map(message => <div className="task-activity-item" key={message.id}>
        <time>{clock(message.created_at)}</time>
        {message.role === "assistant" ? <MessageBody text={message.text} /> : <details><summary>{message.text.split("\n")[0].slice(0, 160)}</summary><pre>{message.text}</pre></details>}
      </div>) : <p className="task-activity-summary">本轮没有工具执行记录。</p>}
    </div>}
  </div>;
}
