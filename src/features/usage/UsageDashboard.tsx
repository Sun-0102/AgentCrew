import { useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import type { AgentUsageSummary, DailyUsagePoint, RateLimitWindow, UsageDashboardData } from "./api";

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatUsd(amount: number): string {
  if (amount <= 0) return "$0.00";
  if (amount < 0.01) return `$${amount.toFixed(4)}`;
  return `$${amount.toFixed(2)}`;
}

function formatCountdown(resetsAt: number | null): string {
  if (!resetsAt) return "重置时间未知";
  const diffSec = Math.max(0, resetsAt - Math.floor(Date.now() / 1000));
  if (diffSec === 0) return "即将重置";
  const days = Math.floor(diffSec / 86400);
  const hours = Math.floor((diffSec % 86400) / 3600);
  const mins = Math.floor((diffSec % 3600) / 60);
  const dateStr = new Date(resetsAt * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
  if (days > 0) return `${days} 天 ${hours} 小时后重置 (${dateStr})`;
  if (hours > 0) return `${hours} 小时 ${mins} 分钟后重置 (${dateStr})`;
  return `${Math.max(1, mins)} 分钟后重置 (${dateStr})`;
}

function planLabel(planType?: string | null): string {
  if (!planType) return "已连接";
  const map: Record<string, string> = {
    prolite: "ChatGPT Pro Lite",
    pro: "ChatGPT Pro",
    plus: "ChatGPT Plus",
    team: "ChatGPT Team",
    enterprise: "Enterprise",
    free: "Free",
  };
  return map[planType.toLowerCase()] ?? planType;
}

function providerLabel(kind: string): string {
  if (kind === "codex") return "Codex";
  if (kind === "deepseek") return "DeepSeek";
  if (kind === "gemini") return "Gemini";
  if (kind === "claude") return "Claude Code";
  return kind;
}

function providerAccent(kind: string): string {
  if (kind === "codex") return "accent-codex";
  if (kind === "deepseek") return "accent-deepseek";
  if (kind === "gemini") return "accent-gemini";
  if (kind === "claude") return "accent-claude";
  return "";
}

function providerColor(kind: string): string {
  if (kind === "codex") return "var(--accent)";
  if (kind === "deepseek") return "#0891b2";
  if (kind === "gemini") return "#10b981";
  if (kind === "claude") return "#d97706";
  return "#64748b";
}

function WindowCard({
  title,
  subtitle,
  win,
  planType,
  resetCreditsCount,
}: {
  title: string;
  subtitle: string;
  win: RateLimitWindow;
  planType?: string | null;
  resetCreditsCount: number;
}) {
  const remaining = Math.round(win.remainingPercent);
  const used = Math.round(win.usedPercent);
  const barColor = remaining <= 15 ? "#dc2626" : remaining <= 35 ? "#d97706" : "var(--accent)";

  return (
    <div className="usage-card usage-window-card">
      <div className="usage-card-header">
        <div className="usage-card-title">{title}</div>
        <span className="usage-card-sub">{subtitle}</span>
      </div>

      <div className="limit-hero-row">
        <div className="limit-hero-agent">
          <Avatar agent="codex" small />
          <div>
            <div className="limit-item-name">
              <span>Codex</span>
              <span className="usage-tag usage-tag-live">实时</span>
              <span className="usage-plan-badge">{planLabel(planType)}</span>
            </div>
            <div className="limit-hero-reset">{formatCountdown(win.resetsAt)}</div>
          </div>
        </div>
        <div className="limit-hero-score">
          <strong style={{ color: barColor }}>{remaining}%</strong>
          <span>剩余 / 已用 {used}%</span>
        </div>
      </div>

      <div className="progress-track progress-track-lg">
        <div className="progress-fill" style={{ width: `${remaining}%`, background: barColor }} />
      </div>

      {resetCreditsCount > 0 && (
        <div className="limit-credits-bar">
          <span>当前账号有 <strong>{resetCreditsCount} 次</strong> 免费重置额度（Full reset）可用</span>
        </div>
      )}
    </div>
  );
}

const costSeries = [
  { key: "codexCost", turnsKey: "codexTurns", label: "Codex", color: "var(--accent)", dash: undefined },
  { key: "deepseekCost", turnsKey: "deepseekTurns", label: "DeepSeek", color: "#0891b2", dash: "8 4" },
  { key: "geminiCost", turnsKey: "geminiTurns", label: "Gemini", color: "#10b981", dash: "2 4" },
  { key: "claudeCost", turnsKey: "claudeTurns", label: "Claude", color: "#d97706", dash: "5 3 2 3" },
] as const;

function DailyCostChart({ daily }: { daily: DailyUsagePoint[] }) {
  const [activeDate, setActiveDate] = useState<string | null>(null);
  const activeIndex = daily.findIndex((point) => point.date === activeDate);
  const active = daily[activeIndex];
  const left = 72;
  const right = 876;
  const top = 20;
  const bottom = 184;
  const maxCost = Math.max(0, ...daily.flatMap((point) => costSeries.map((series) => point[series.key] ?? 0)));
  const ceiling = maxCost > 0 ? maxCost * 1.15 : 1;
  const x = (index: number) => (daily.length <= 1 ? (left + right) / 2 : left + (index * (right - left)) / (daily.length - 1));
  const y = (cost: number) => bottom - (cost / ceiling) * (bottom - top);
  const labelStep = Math.max(1, Math.ceil((daily.length - 1) / 6));
  const tickDecimals = Math.max(2, Math.min(8, Math.ceil(-Math.log10(ceiling / 4)) + 1));

  if (daily.length === 0) return <div className="usage-empty">当前周期暂无成本数据。</div>;

  return (
    <div className="usage-line-chart" onMouseLeave={() => setActiveDate(null)}>
      <svg className="usage-line-svg" viewBox="0 0 900 222" role="group" aria-label="每日折算成本折线图，单位美元。可聚焦各日期查看明细。">
        {[0, 1, 2, 3, 4].map((tick) => {
          const value = (ceiling * tick) / 4;
          const position = y(value);
          return (
            <g key={tick}>
              <line className="usage-chart-grid" x1={left} x2={right} y1={position} y2={position} />
              <text className="usage-chart-label" x={left - 10} y={position + 4} textAnchor="end">
                ${value.toFixed(tickDecimals)}
              </text>
            </g>
          );
        })}
        {costSeries.map((series) => (
          <g key={series.key}>
            <polyline
              points={daily.map((point, index) => `${x(index)},${y(point[series.key] ?? 0)}`).join(" ")}
              fill="none"
              stroke={series.color}
              strokeWidth="2.5"
              strokeDasharray={series.dash}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
            {daily.map((point, index) => (
              <circle
                key={point.date}
                cx={x(index)}
                cy={y(point[series.key] ?? 0)}
                r={activeDate === point.date ? 4.5 : 3}
                fill="var(--surface)"
                stroke={series.color}
                strokeWidth="2"
              />
            ))}
          </g>
        ))}
        {active && <line className="usage-chart-guide" x1={x(activeIndex)} x2={x(activeIndex)} y1={top} y2={bottom} />}
        {daily.map((point, index) => {
          const total = costSeries.reduce((sum, s) => sum + (point[s.key] ?? 0), 0);
          const totalTurns = costSeries.reduce((sum, s) => sum + (point[s.turnsKey] ?? 0), 0);
          const description = `${point.date}：合计 ${formatUsd(total)}（${totalTurns} 轮）`;
          const hitLeft = index === 0 ? left : (x(index - 1) + x(index)) / 2;
          const hitRight = index === daily.length - 1 ? right : (x(index) + x(index + 1)) / 2;
          return (
            <g key={point.date}>
              {(index % labelStep === 0 || index === daily.length - 1) && (
                <text className="usage-chart-label" x={x(index)} y={210} textAnchor="middle">
                  {point.date}
                </text>
              )}
              <rect
                className="usage-chart-hit"
                x={hitLeft}
                y={top}
                width={hitRight - hitLeft}
                height={bottom - top + 6}
                tabIndex={0}
                role="img"
                aria-label={description}
                onMouseEnter={() => setActiveDate(point.date)}
                onFocus={() => setActiveDate(point.date)}
                onBlur={() => setActiveDate(null)}
              />
            </g>
          );
        })}
      </svg>
      {maxCost === 0 && !active && <div className="usage-chart-zero">当前周期折算成本为 $0.00</div>}
      {active && (
        <div className={`usage-chart-tooltip ${activeIndex > (daily.length - 1) / 2 ? "align-right" : ""}`}>
          <strong>{active.date}</strong>
          {costSeries.map((series) => (
            <div key={series.key}>
              <span>
                <i className="legend-dot" style={{ background: series.color }} />
                {series.label}
              </span>
              <b>{formatUsd(active[series.key] ?? 0)}</b>
            </div>
          ))}
          <div className="usage-chart-total">
            <span>合计 ({costSeries.reduce((sum, s) => sum + (active[s.turnsKey] ?? 0), 0)} 轮)</span>
            <b>{formatUsd(costSeries.reduce((sum, s) => sum + (active[s.key] ?? 0), 0))}</b>
          </div>
        </div>
      )}
    </div>
  );
}

function AgentUsageCard({
  agent,
  data,
}: {
  agent: AgentUsageSummary;
  data: UsageDashboardData;
}) {
  const isCodex = agent.providerKind === "codex";
  const fiveHourWindow = isCodex ? data.codexLimits.windows.find((w) => w.kind === "five_hour") : undefined;
  const weeklyWindow = isCodex ? data.codexLimits.windows.find((w) => w.kind === "weekly" || w.kind === "custom") : undefined;
  const primaryWindow = fiveHourWindow ?? weeklyWindow;
  const totalTokens = agent.inputTokens + agent.outputTokens;
  const cacheRate = agent.inputTokens > 0 ? Math.min(100, (agent.cachedTokens / agent.inputTokens) * 100) : 0;

  return (
    <div className={`usage-provider-card ${providerAccent(agent.providerKind)}`}>
      <div className="usage-provider-head">
        <Avatar agent={agent.id} small />
        <div>
          <h3>{agent.displayName || providerLabel(agent.providerKind)}</h3>
          <small>
            {agent.runtimeKind}
            {isCodex && data.codexLimits.planType ? ` / ${planLabel(data.codexLimits.planType)}` : ""}
          </small>
        </div>
        <span
          className={`usage-status-pill ${
            agent.quotaExhaustedRecent ? "warn" : agent.connected ? "ok" : "muted"
          }`}
        >
          {agent.quotaExhaustedRecent ? "近期限流" : agent.connected ? "已连接" : "未连接"}
        </span>
      </div>

      <div className="usage-highlight-box">
        {isCodex && primaryWindow ? (
          <>
            <span>{fiveHourWindow ? "5 小时剩余" : "周限额剩余"}</span>
            <strong style={{ color: primaryWindow.remainingPercent <= 15 ? "#dc2626" : "var(--accent)" }}>
              {Math.round(primaryWindow.remainingPercent)}%
            </strong>
          </>
        ) : (
          <>
            <span>本周期 Token 消耗</span>
            <strong style={{ color: providerColor(agent.providerKind) }}>{formatTokens(totalTokens)}</strong>
          </>
        )}
      </div>

      <div className="usage-metric-list">
        {isCodex && fiveHourWindow && weeklyWindow && (
          <div className="usage-metric-row">
            <span>周限额剩余</span>
            <strong>{Math.round(weeklyWindow.remainingPercent)}%</strong>
          </div>
        )}
        <div className="usage-metric-row">
          <span>今日 / 周期轮次</span>
          <strong>
            {agent.turnsToday} 轮 / {agent.turnsRange} 轮
          </strong>
        </div>
        {isCodex && (
          <div className="usage-metric-row">
            <span>Token 消耗</span>
            <strong>{formatTokens(totalTokens)}</strong>
          </div>
        )}
        <div className="usage-metric-row">
          <span>缓存命中率</span>
          <strong>{agent.inputTokens > 0 ? `${cacheRate.toFixed(0)}%` : "—"}</strong>
        </div>
        <div className="usage-metric-row">
          <span>折算费用</span>
          <strong>{formatUsd(agent.costUsd)}</strong>
        </div>
      </div>

      {agent.quotaExhaustedRecent && (
        <div className="usage-provider-note usage-provider-note-warn">
          近期检测到 <code>quota_exhausted</code> 限流响应，请检查原厂账号额度或稍后重试。
        </div>
      )}
    </div>
  );
}

export default function UsageDashboard({
  data,
  days,
  loading,
  embedded = false,
  onChangeDays,
  onRefresh,
}: {
  data: UsageDashboardData;
  days: number;
  loading: boolean;
  embedded?: boolean;
  onChangeDays: (days: number) => void;
  onRefresh: () => void;
}) {
  const fiveHourWindow = data.codexLimits.windows.find((w) => w.kind === "five_hour");
  const weeklyWindow = data.codexLimits.windows.find((w) => w.kind === "weekly" || w.kind === "custom");

  const content = (
    <div className={embedded ? "usage-embedded" : "usage-container"}>
      <div className={`usage-header ${embedded ? "usage-header-embedded" : ""}`}>
        {embedded ? (
          <div className="usage-embedded-meta">
            <span>来自本机调用记录与 Codex 实时窗口</span>
            {data.updatedAt > 0 && (
              <span>
                更新于{" "}
                {new Date(data.updatedAt * 1000).toLocaleTimeString("zh-CN", {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
            )}
          </div>
        ) : (
          <div>
            <h1>
              <span className="usage-title-icon">
                <Icon name="chart" size={18} />
              </span>
              使用量与额度看板
              <span className="usage-kbd">⌘U</span>
            </h1>
          </div>
        )}
        <div className="usage-controls">
          <div className="usage-segmented" role="group" aria-label="统计周期">
            {[
              { value: 1, label: "今日" },
              { value: 7, label: "近 7 天" },
              { value: 30, label: "近 30 天" },
            ].map((opt) => (
              <button
                key={opt.value}
                type="button"
                className={days === opt.value ? "active" : ""}
                onClick={() => onChangeDays(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>
          <button type="button" className="usage-btn-outline" onClick={onRefresh} disabled={loading}>
            <span className={loading ? "usage-spin" : ""}>
              <Icon name="reset" size={14} />
            </span>
            {loading ? "刷新中…" : "刷新"}
          </button>
        </div>
      </div>

      {(fiveHourWindow || weeklyWindow) && (
        <div className={`window-grid ${fiveHourWindow && weeklyWindow ? "" : "single"}`}>
          {fiveHourWindow && (
            <WindowCard
              title="5 小时滚动限额"
              subtitle="Codex 实时窗口"
              win={fiveHourWindow}
              planType={data.codexLimits.planType}
              resetCreditsCount={data.codexLimits.resetCreditsCount}
            />
          )}
          {weeklyWindow && (
            <WindowCard
              title="每周限额"
              subtitle="跨会话累计窗口"
              win={weeklyWindow}
              planType={data.codexLimits.planType}
              resetCreditsCount={data.codexLimits.resetCreditsCount}
            />
          )}
        </div>
      )}

      <div className="kpi-grid">
        <div className="kpi-card">
          <div className="kpi-label">API 等价折算费用（近 {days} 天）</div>
          <div className="kpi-value">{formatUsd(data.summary.totalCostUsd)}</div>
          <div className="kpi-sub">基于各模型公开 API 单价估算</div>
        </div>
        <div className="kpi-card">
          <div className="kpi-label">累计 Token 消耗</div>
          <div className="kpi-value">
            {formatTokens(data.summary.totalInputTokens + data.summary.totalOutputTokens)}
          </div>
          <div className="kpi-sub">
            总输入 {formatTokens(data.summary.totalInputTokens)} / 输出{" "}
            {formatTokens(data.summary.totalOutputTokens)}
          </div>
        </div>
        <div className="kpi-card">
          <div className="kpi-label">Prompt 缓存命中率</div>
          <div className="kpi-value">{data.summary.cacheHitRate.toFixed(1)}%</div>
          <div className="kpi-sub">已命中 {formatTokens(data.summary.totalCachedTokens)} 缓存</div>
        </div>
        <div className="kpi-card">
          <div className="kpi-label">累计执行轮次</div>
          <div className="kpi-value">{data.summary.totalTurns} 轮</div>
          <div className="kpi-sub">
            群聊 {data.summary.groupTurns} 轮 / 私聊 {data.summary.privateTurns} 轮
          </div>
        </div>
      </div>

      {data.agents.length > 0 && (
        <>
          <h3 className="usage-section-title">
            <span>各接入 Agent 用量</span>
            <span className="usage-section-sub">
              Codex 支持实时配额窗口；Gemini / DeepSeek / Claude 统计本机调用与限流状态
            </span>
          </h3>
          <div className={`usage-provider-grid ${embedded ? "usage-provider-grid-embedded" : ""}`}>
            {data.agents.map((agent) => (
              <AgentUsageCard key={agent.id} agent={agent} data={data} />
            ))}
          </div>
        </>
      )}

      <div className="usage-card" style={{ marginTop: 16, marginBottom: 16 }}>
        <div className="usage-card-header">
          <div className="usage-card-title">每日折算成本趋势 (USD)</div>
          <div className="chart-legend">
            {costSeries.map((series) => (
              <span key={series.key}>
                <i className="legend-dot" style={{ background: series.color }} />
                {series.label}
              </span>
            ))}
          </div>
        </div>
        <DailyCostChart daily={data.daily} />
      </div>

      <div className="usage-card">
        <div className="usage-card-header">
          <div className="usage-card-title">模型维度明细</div>
          <span className="usage-card-sub">来自本机 SQLite usage_records</span>
        </div>
        {data.models.length === 0 ? (
          <div className="usage-empty">
            当前周期内暂无已记录的模型调用。在私聊或群聊中发送消息后，用量将自动记录在这里。
          </div>
        ) : (
          <table className="usage-table">
            <thead>
              <tr>
                <th>Agent / 模型</th>
                <th className="num">轮次</th>
                <th className="num">总输入 Token</th>
                <th className="num">输出 Token</th>
                <th className="num">缓存命中</th>
                <th className="num">折算费用</th>
              </tr>
            </thead>
            <tbody>
              {data.models.map((row) => (
                <tr key={`${row.agentId}:${row.model}`}>
                  <td>
                    <strong>{providerLabel(row.providerKind)}</strong> / {row.model || "默认模型"}
                  </td>
                  <td className="num">{row.turns}</td>
                  <td className="num">{row.inputTokens.toLocaleString()}</td>
                  <td className="num">{row.outputTokens.toLocaleString()}</td>
                  <td className="num">{row.cacheHitRate.toFixed(0)}%</td>
                  <td className="num">
                    <strong>{formatUsd(row.costUsd)}</strong>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );

  if (embedded) return content;
  return <main className="usage-workspace">{content}</main>;
}
