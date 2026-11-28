import { agentName, type ContextUsage } from "./api";

export function formatCompactTokens(value: number): string {
  const n = Math.max(0, Math.round(value));
  if (n >= 1_000_000) {
    // 1_048_576 / 1_047_576 display cleanly as 1M
    if (n <= 1_060_000) return "1M";
    const m = n / 1_000_000;
    return `${m >= 10 ? Math.round(m) : Number(m.toFixed(1))}M`;
  }
  if (n >= 1_000) {
    const k = n / 1_000;
    return `${k >= 100 ? Math.round(k) : Number(k.toFixed(1))}k`;
  }
  return String(n);
}

export function formatPercent(percent: number): string {
  if (percent > 0 && percent < 1) return "<1%";
  return `${Math.min(100, Math.max(0, Math.round(percent)))}%`;
}

function defaultWindowForAgent(agentId: string): number {
  const lower = agentId.toLowerCase();
  if (lower.includes("gemini")) return 1_048_576;
  if (lower.includes("claude") || lower.includes("sonnet") || lower.includes("opus") || lower.includes("haiku")) return 200_000;
  if (lower.includes("deepseek")) return 128_000;
  return 272_000;
}

function levelClass(percent: number): "normal" | "warn" | "danger" {
  if (percent >= 90) return "danger";
  if (percent >= 70) return "warn";
  return "normal";
}

function Ring({ percent, size = 14 }: { percent: number; size?: number }) {
  const stroke = 1.9;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const clamped = Math.min(100, Math.max(0, percent));
  const dashOffset = circumference * (1 - clamped / 100);
  return (
    <svg className="context-meter-ring" width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <circle className="context-meter-track" cx={size / 2} cy={size / 2} r={radius} strokeWidth={stroke} />
      <circle
        className="context-meter-progress"
        cx={size / 2}
        cy={size / 2}
        r={radius}
        strokeWidth={stroke}
        strokeDasharray={circumference}
        strokeDashoffset={dashOffset}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
    </svg>
  );
}

export default function ContextMeter({
  agent,
  members,
  isGroup,
  model,
  contextUsage,
  compactingAgent,
  compacting,
  onCompact,
}: {
  agent: string;
  members: string[];
  isGroup: boolean;
  model: string;
  contextUsage?: Record<string, ContextUsage>;
  compactingAgent?: string | null;
  compacting?: boolean;
  onCompact?: () => void;
}) {
  if (!agent) return null;

  const fallback = (id: string, fallbackModel = ""): ContextUsage => ({
    agentId: id,
    providerKind: id,
    model: fallbackModel,
    usedTokens: 0,
    contextWindow: defaultWindowForAgent(id),
    percent: 0,
    inputTokens: 0,
    outputTokens: 0,
    cachedTokens: 0,
    source: "none",
  });

  const current = contextUsage?.[agent] ?? fallback(agent, model);
  const displayModel = current.model || model;
  const level = levelClass(current.percent);
  const memberList = isGroup && members.length > 0 ? members : [agent];
  const memberItems = memberList.map(id => contextUsage?.[id] ?? fallback(id, id === agent ? model : ""));

  const isCompacting = !!compacting || !!compactingAgent;
  const showCompactAction = onCompact && (current.percent >= 70 || isCompacting);

  return (
    <div className="context-meter-wrap">
    <div
      className={`context-meter level-${level}${current.usedTokens === 0 ? " empty" : ""}`}
      tabIndex={0}
      aria-label={`当前上下文已用 ${formatPercent(current.percent)}，${formatCompactTokens(current.usedTokens)} / ${formatCompactTokens(current.contextWindow)}`}
    >
      <Ring percent={current.percent} />
      <span className="context-meter-pct">{formatPercent(current.percent)}</span>
      <span className="context-meter-sep" aria-hidden="true">·</span>
      <span className="context-meter-tokens">
        {formatCompactTokens(current.usedTokens)} / {formatCompactTokens(current.contextWindow)}
      </span>

      <div className="context-meter-popover" role="tooltip">
        <div className="context-meter-popover-head">
          <strong>{isGroup ? "群成员上下文" : "上下文窗口"}</strong>
          <span>{isGroup ? "各成员独立" : displayModel || agentName(agent)}</span>
        </div>

        {isGroup ? (
          <div className="context-meter-member-list">
            {memberItems.map(item => {
              const itemLevel = levelClass(item.percent);
              const isSelected = item.agentId === agent;
              return (
                <div className={`context-meter-member-row${isSelected ? " selected" : ""}`} key={item.agentId}>
                  <div className="context-meter-member-meta">
                    <span className="context-meter-member-name">
                      {isSelected && <i className="context-meter-active-dot" />}
                      {agentName(item.agentId)}
                    </span>
                    <span className={`context-meter-member-val level-${itemLevel}`}>
                      {formatPercent(item.percent)} · {formatCompactTokens(item.usedTokens)} / {formatCompactTokens(item.contextWindow)}
                    </span>
                  </div>
                  <div className="context-meter-bar">
                    <div
                      className={`context-meter-bar-fill level-${itemLevel}`}
                      style={{ width: `${Math.min(100, Math.max(0, item.percent))}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="context-meter-single">
            <div className="context-meter-bar">
              <div
                className={`context-meter-bar-fill level-${level}`}
                style={{ width: `${Math.min(100, Math.max(0, current.percent))}%` }}
              />
            </div>
            <div className="context-meter-numbers">
              <span>{current.usedTokens.toLocaleString()} / {current.contextWindow.toLocaleString()} tokens</span>
              <strong className={`level-${level}`}>{current.percent.toFixed(1)}%</strong>
            </div>
          </div>
        )}

        <div className="context-meter-breakdown">
          {current.usedTokens > 0 ? (
            <>
              <span>{isGroup ? `${agentName(agent)} 最近一轮：` : "最近一轮："}</span>
              <span>输入 {formatCompactTokens(current.inputTokens)}</span>
              {current.cachedTokens > 0 && <span>缓存 {formatCompactTokens(current.cachedTokens)}</span>}
              <span>输出 {formatCompactTokens(current.outputTokens)}</span>
            </>
          ) : (
            <span>本会话尚未产生上下文消耗</span>
          )}
        </div>
      </div>
    </div>
    {showCompactAction && (
      <button
        type="button"
        className={`context-compact-btn${isCompacting ? " is-compacting" : ""}`}
        disabled={isCompacting}
        onClick={onCompact}
        title="生成阶段检查点摘要并轮转底层会话（前端聊天记录完整保留）"
      >
        {isCompacting ? "正在整理上下文…" : "整理上下文"}
      </button>
    )}
    </div>
  );
}
