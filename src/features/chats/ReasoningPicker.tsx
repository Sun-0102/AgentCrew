import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
import Icon from "../../components/Icon";
import type { Model } from "./api";
import { effortLabels, selectedEffort } from "./reasoning";

interface Props {
  model?: Model;
  value?: string;
  disabled: boolean;
  onChange?: (value: string) => void;
}

export default function ReasoningPicker({ model, value, disabled, onChange }: Props) {
  const id = useId();
  const panel = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 12, top: 12, width: 320 });
  const choices = model?.supportedReasoningEfforts ?? [];
  const index = Math.max(0, choices.findIndex(c => c.reasoningEffort === value));
  const label = effortLabels[choices[index]?.reasoningEffort] ?? choices[index]?.reasoningEffort ?? "思考程度";
  const unavailable = disabled || !choices.length;
  useEffect(() => {
    if (unavailable) panel.current?.hidePopover();
  }, [unavailable]);
  useEffect(() => {
    const close = () => panel.current?.hidePopover();
    window.addEventListener("resize", close);
    return () => window.removeEventListener("resize", close);
  }, []);

  function positionPanel() {
    const bounds = trigger.current!.getBoundingClientRect();
    const width = Math.min(320, window.innerWidth - 24);
    setPosition({ width, left: Math.max(12, Math.min(bounds.left + bounds.width / 2 - width / 2, window.innerWidth - width - 12)), top: Math.max(12, bounds.top - 150) });
  }

  return <>
    <button ref={trigger} type="button" className="reasoning-trigger" aria-label={`选择思考程度：${label}`} aria-expanded={open} aria-controls={id} aria-haspopup="dialog" popoverTarget={id} disabled={unavailable} onClick={positionPanel}>
      <Icon name="spark" size={15} /><span>{label}</span><Icon name="down" size={13} />
    </button>
    <div ref={panel} id={id} popover="auto" className="reasoning-popover" role="dialog" aria-label="思考程度" style={position} onToggle={event => setOpen(event.newState === "open")}>
      <div className="reasoning-popover-heading">
        <Icon name="spark" size={18} />
        <div><strong aria-live="polite">{label}</strong><p title={model?.displayName}>{model?.displayName}</p></div>
        <button type="button" className="icon-button" aria-label="恢复推荐思考程度" title="恢复当前模型推荐档位" disabled={unavailable} onClick={() => onChange?.(selectedEffort(model))}><Icon name="reset" size={18} /></button>
      </div>
      <div className="reasoning-slider" style={{ "--effort-progress": `${choices.length > 1 ? index / (choices.length - 1) * 100 : 0}%` } as CSSProperties}>
        <input type="range" min={0} max={Math.max(1, choices.length - 1)} step={1} value={index} aria-label="拖动选择思考程度" aria-valuetext={label} disabled={unavailable || choices.length < 2} onChange={event => { const choice = choices[Number(event.currentTarget.value)]; if (choice) onChange?.(choice.reasoningEffort); }} />
        <div className="reasoning-slider-stops" aria-hidden="true">{choices.map((choice, i) => <span key={choice.reasoningEffort} className={`${i < index ? "passed" : ""} ${i === index ? "current" : ""}`} style={{ left: `${choices.length > 1 ? i / (choices.length - 1) * 100 : 0}%` }} />)}</div>
      </div>
    </div>
  </>;
}
