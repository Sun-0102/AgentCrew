import { useEffect, useId, useRef, useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import { agentName, useAgents } from "./api";

interface Props {
  agent: string;
  disabled: boolean;
  onAgent: (agent: string) => void;
  onSettings: () => void;
}

export default function ProjectAgentPicker({ agent, disabled, onAgent, onSettings }: Props) {
  const agents = useAgents().filter(item => item.enabled && item.registration_state === "active");
  const id = useId();
  const panel = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 12, bottom: 72, width: 320 });

  function updatePosition() {
    const rect = trigger.current?.getBoundingClientRect();
    if (!rect) return;
    const width = Math.min(320, window.innerWidth - 24);
    setPosition({
      left: Math.max(12, Math.min(rect.left, window.innerWidth - width - 12)),
      bottom: Math.max(12, window.innerHeight - rect.top + 10),
      width,
    });
  }

  useEffect(() => {
    const close = () => panel.current?.hidePopover();
    window.addEventListener("resize", close);
    return () => window.removeEventListener("resize", close);
  }, []);
  useEffect(() => { if (disabled) panel.current?.hidePopover(); }, [disabled]);

  return <>
    <button
      ref={trigger}
      type="button"
      className={`model-config-trigger project-agent-trigger ${open ? "is-open" : ""}`}
      disabled={disabled}
      popoverTarget={id}
      aria-label={`选择执行 Agent：${agentName(agent)}`}
      aria-expanded={open}
      aria-controls={id}
      aria-haspopup="dialog"
      title={disabled ? "任务结束后可切换 Agent" : "选择处理当前项目的 Agent"}
      onClick={updatePosition}
    >
      <Avatar agent={agent} small />
      <span className="model-config-name">{agentName(agent)}</span>
      <span className="model-config-chevron"><Icon name="down" size={13} /></span>
    </button>
    <div
      ref={panel}
      id={id}
      popover="auto"
      role="dialog"
      aria-label="选择项目 Agent"
      className="model-config-popover project-agent-popover"
      style={position}
      onToggle={event => { setOpen(event.newState === "open"); }}
    >
      <div className="project-agent-heading">
        <span className="project-agent-eyebrow">当前项目</span>
        <strong>选择执行 Agent</strong>
        <p>用你熟悉的 Agent，继续处理这个项目。</p>
      </div>
      <div className="project-agent-list" role="listbox" aria-label="项目可用 Agent">
        {agents.map(item => {
          const selected = item.id === agent;
          const status = item.connected ? "已连接" : item.availability === "needs_auth" ? "需要登录" : "未连接";
          return <button
            key={item.id}
            type="button"
            role="option"
            aria-selected={selected}
            disabled={disabled || !item.connected}
            className={`project-agent-option ${selected ? "selected" : ""}`}
            onClick={() => { onAgent(item.id); panel.current?.hidePopover(); }}
          >
            <Avatar agent={item.id} />
            <span className="project-agent-info"><strong>{item.name}</strong><small><span className={`presence-dot ${item.connected ? "connected" : ""}`} />{status}{item.runtime_label ? ` · ${item.runtime_label}` : ""}</small></span>
            {selected && <span className="project-agent-check"><Icon name="check" size={15} /></span>}
          </button>;
        })}
        {!agents.length && <p className="project-agent-empty">还没有接入 Agent。</p>}
      </div>
      <div className="project-agent-footer">
        <p><Icon name="folder" size={14} /><span>项目文件共享，各 Agent 的执行上下文独立。</span></p>
        <button type="button" onClick={() => { panel.current?.hidePopover(); onSettings(); }}>管理 Agent 连接<Icon name="right" size={13} /></button>
      </div>
    </div>
  </>;
}
