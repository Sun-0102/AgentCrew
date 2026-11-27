import { useMemo, useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import type {
  AgentUsageSummary,
  DailyUsagePoint,
  ModelUsageRow,
  RateLimitWindow,
  UsageDashboardData,
} from "./api";

type ChartMetric = "cost" | "turns" | "tokens";
type ChartScale = "sqrt" | "linear";
type ModelSortKey = "cost" | "turns" | "tokens" | "cache";

export const PROVIDER_SERIES = [
  {
    provider: "codex",
    label: "Codex",
    colorVar: "var(--provider-codex)",
    costKey: "codexCost",
    turnsKey: "codexTurns",
    tokensKey: "codexTokens",
    dash: undefined,
    marker: "circle",
  },
  {
    provider: "deepseek",
    label: "DeepSeek",
    colorVar: "var(--provider-deepseek)",
    costKey: "deepseekCost",
    turnsKey: "deepseekTurns",
    tokensKey: "deepseekTokens",
    dash: "7 4",
    marker: "diamond",
  },
  {
    provider: "gemini",
    label: "Gemini",
    colorVar: "var(--provider-gemini)",
    costKey: "geminiCost",
    turnsKey: "geminiTurns",
    tokensKey: "geminiTokens",
    dash: "3 3.5",
    marker: "square",
  },
  {
    provider: "claude",
    label: "Claude",
    colorVar: "var(--provider-claude)",
    costKey: "claudeCost",
    turnsKey: "claudeTurns",
    tokensKey: "claudeTokens",
    dash: "8 3 2 3",
    marker: "triangle",
  },
] as const;

type SeriesConfig = (typeof PROVIDER_SERIES)[number];

export function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(Math.max(0, Math.round(n)));
}

export function formatUsd(amount: number): string {
  if (amount <= 0) return "$0.00";
  if (amount < 0.001) return `$${amount.toFixed(4)}`;
  if (amount < 0.01) return `$${amount.toFixed(4)}`;
  if (amount < 0.1) return `$${amount.toFixed(3)}`;
  return `$${amount.toFixed(2)}`;
}

function formatAxisValue(value: number, metric: ChartMetric, ceiling: number): string {
  if (value <= 0) {
    return metric === "cost" ? "$0.00" : "0";
  }
  if (metric === "turns") {
    return value >= 10 ? `${Math.round(value)}` : `${Number(value.toFixed(1))}`;
  }
  if (metric === "tokens") {
    return formatTokens(value);
  }
  if (ceiling < 0.02 || value < 0.005) return `$${value.toFixed(4)}`;
  if (ceiling < 0.2 || value < 0.05) return `$${value.toFixed(3)}`;
  return `$${value.toFixed(2)}`;
}

function formatRelativeTime(unixSec?: number | null): string {
  if (!unixSec || unixSec <= 0) return "暂无调用";
  const diffSec = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
  if (diffSec < 60) return "刚刚";
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} 天前`;
  return new Date(unixSec * 1000).toLocaleDateString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
  });
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

export function providerColor(kind: string): string {
  if (kind === "codex") return "var(--provider-codex)";
  if (kind === "deepseek") return "var(--provider-deepseek)";
  if (kind === "gemini") return "var(--provider-gemini)";
  if (kind === "claude") return "var(--provider-claude)";
  return "#64748b";
}

function pointMetricValue(point: DailyUsagePoint, series: SeriesConfig, metric: ChartMetric): number {
  if (metric === "turns") return point[series.turnsKey] ?? 0;
  if (metric === "tokens") return point[series.tokensKey] ?? 0;
  return point[series.costKey] ?? 0;
}

function SeriesMarker({
  marker,
  cx,
  cy,
  size,
  color,
  emphasized,
}: {
  marker: SeriesConfig["marker"];
  cx: number;
  cy: number;
  size: number;
  color: string;
  emphasized: boolean;
}) {
  const strokeW = emphasized ? 2.2 : 1.8;
  if (marker === "diamond") {
    const r = size + 0.9;
    return (
      <polygon
        points={`${cx},${cy - r} ${cx + r},${cy} ${cx},${cy + r} ${cx - r},${cy}`}
        fill={emphasized ? color : "var(--surface)"}
        stroke={color}
        strokeWidth={strokeW}
      />
    );
  }
  if (marker === "square") {
    const half = size * 0.92;
    return (
      <rect
        x={cx - half}
        y={cy - half}
        width={half * 2}
        height={half * 2}
        rx={1.6}
        fill={emphasized ? color : "var(--surface)"}
        stroke={color}
        strokeWidth={strokeW}
      />
    );
  }
  if (marker === "triangle") {
    const r = size + 1.1;
    return (
      <polygon
        points={`${cx},${cy - r} ${cx + r * 0.95},${cy + r * 0.8} ${cx - r * 0.95},${cy + r * 0.8}`}
        fill={emphasized ? color : "var(--surface)"}
        stroke={color}
        strokeWidth={strokeW}
        strokeLinejoin="round"
      />
    );
  }
  return (
    <circle
      cx={cx}
      cy={cy}
      r={size}
      fill={emphasized ? color : "var(--surface)"}
      stroke={color}
      strokeWidth={strokeW}
    />
  );
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
  const barColor =
    remaining <= 15 ? "#dc2626" : remaining <= 35 ? "#d97706" : "var(--provider-codex)";

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
          <span>
            当前账号有 <strong>{resetCreditsCount} 次</strong> 免费重置额度（Full reset）可用
          </span>
        </div>
      )}
    </div>
  );
}

function UsageTrendChart({
  daily,
  granularity,
}: {
  daily: DailyUsagePoint[];
  granularity: "hour" | "day";
}) {
  const [metric, setMetric] = useState<ChartMetric>("cost");
  const [scale, setScale] = useState<ChartScale>("sqrt");
  const [focusedProvider, setFocusedProvider] = useState<string | null>(null);
  const [hoveredProvider, setHoveredProvider] = useState<string | null>(null);
  const [activeDate, setActiveDate] = useState<string | null>(null);

  const hasTokenSeries = useMemo(
    () =>
      daily.some(
        (p) =>
          (p.codexTokens ?? 0) > 0 ||
          (p.deepseekTokens ?? 0) > 0 ||
          (p.geminiTokens ?? 0) > 0 ||
          (p.claudeTokens ?? 0) > 0,
      ),
    [daily],
  );

  const effectiveScale: ChartScale = metric === "turns" ? "linear" : scale;
  const activeSeriesList = useMemo<readonly SeriesConfig[]>(
    () =>
      focusedProvider
        ? PROVIDER_SERIES.filter((s) => s.provider === focusedProvider)
        : PROVIDER_SERIES,
    [focusedProvider],
  );

  const seriesTotals = useMemo(
    () =>
      Object.fromEntries(
        PROVIDER_SERIES.map((s) => {
          const cost = daily.reduce((sum, p) => sum + (p[s.costKey] ?? 0), 0);
          const turns = daily.reduce((sum, p) => sum + (p[s.turnsKey] ?? 0), 0);
          const tokens = daily.reduce((sum, p) => sum + (p[s.tokensKey] ?? 0), 0);
          return [s.provider, { cost, turns, tokens }];
        }),
      ) as Record<SeriesConfig["provider"], { cost: number; turns: number; tokens: number }>,
    [daily],
  );

  const activeIndex = daily.findIndex((point) => point.date === activeDate);
  const active = daily[activeIndex];
  const left = 68;
  const right = 880;
  const top = 18;
  const bottom = 186;
  const plotHeight = bottom - top;

  const maxVal = Math.max(
    0,
    ...daily.flatMap((point) => activeSeriesList.map((s) => pointMetricValue(point, s, metric))),
  );
  const totalTurnsAll = daily.reduce(
    (sum, p) => sum + activeSeriesList.reduce((acc, s) => acc + (p[s.turnsKey] ?? 0), 0),
    0,
  );
  const ceiling =
    maxVal > 0
      ? metric === "turns"
        ? Math.max(4, Math.ceil(maxVal * 1.15))
        : maxVal * 1.14
      : metric === "turns"
        ? 4
        : 1;

  const x = (index: number) =>
    daily.length <= 1
      ? (left + right) / 2
      : left + (index * (right - left)) / (daily.length - 1);

  const yFor = (val: number, hasActivity: boolean) => {
    if (val <= 0) {
      return hasActivity ? bottom - 4 : bottom;
    }
    const clamped = Math.min(1, Math.max(0, val / ceiling));
    const ratio = effectiveScale === "sqrt" ? Math.sqrt(clamped) : clamped;
    const rawY = bottom - ratio * plotHeight;
    return hasActivity ? Math.min(bottom - 4, rawY) : rawY;
  };

  const tickValueAt = (ratio: number) => {
    if (ratio <= 0) return 0;
    return effectiveScale === "sqrt" ? ceiling * ratio * ratio : ceiling * ratio;
  };

  const labelStep = Math.max(1, Math.ceil((daily.length - 1) / (granularity === "hour" ? 8 : 7)));

  if (daily.length === 0) {
    return <div className="usage-empty">当前周期暂无趋势数据。</div>;
  }

  const metricTitle =
    metric === "cost"
      ? granularity === "hour"
        ? "今日分时折算成本 (USD)"
        : "每日折算成本趋势 (USD)"
      : metric === "turns"
        ? granularity === "hour"
          ? "今日分时调用轮次"
          : "每日调用轮次趋势"
        : granularity === "hour"
          ? "今日分时 Token 吞吐"
          : "每日 Token 消耗趋势";

  return (
    <div className="usage-trend-block">
      <div className="usage-trend-header">
        <div>
          <div className="usage-card-title">{metricTitle}</div>
          <div className="usage-trend-sub">
            {focusedProvider
              ? `已单独聚焦 ${providerLabel(focusedProvider)}（纵轴已自动适配量级，再次点击图例恢复全部对比）`
              : effectiveScale === "sqrt" && metric !== "turns"
                ? "已启用微额放大比例尺，低单价调用（如几美分）与高开销曲线可同屏清晰辨认；未调用日期自动隐藏节点"
                : "点击任一 Agent 图例可单独放大查看其专属量级走势"}
          </div>
        </div>

        <div className="usage-trend-controls">
          <div className="usage-mini-seg" role="group" aria-label="趋势指标">
            <button
              type="button"
              className={metric === "cost" ? "active" : ""}
              onClick={() => setMetric("cost")}
            >
              成本 ($)
            </button>
            <button
              type="button"
              className={metric === "turns" ? "active" : ""}
              onClick={() => setMetric("turns")}
            >
              轮次
            </button>
            {hasTokenSeries && (
              <button
                type="button"
                className={metric === "tokens" ? "active" : ""}
                onClick={() => setMetric("tokens")}
              >
                Token
              </button>
            )}
          </div>

          {metric !== "turns" && (
            <div className="usage-mini-seg" role="group" aria-label="纵轴比例">
              <button
                type="button"
                className={scale === "sqrt" ? "active" : ""}
                onClick={() => setScale("sqrt")}
                title="拉伸底部微小数值区间，让几美分的低成本调用也清晰可见"
              >
                微额放大
              </button>
              <button
                type="button"
                className={scale === "linear" ? "active" : ""}
                onClick={() => setScale("linear")}
                title="标准等距线性纵轴"
              >
                线性
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="chart-legend-pills" role="group" aria-label="按 Agent 筛选或高亮曲线">
        {PROVIDER_SERIES.map((series) => {
          const totals = seriesTotals[series.provider];
          const isFocused = focusedProvider === series.provider;
          const isDimmed =
            (focusedProvider !== null && !isFocused) ||
            (hoveredProvider !== null && hoveredProvider !== series.provider);
          const summaryBadge =
            totals.turns > 0
              ? metric === "turns"
                ? `${totals.turns} 轮`
                : metric === "tokens"
                  ? `${formatTokens(totals.tokens)}`
                  : `${formatUsd(totals.cost)} / ${totals.turns}轮`
              : "未调用";
          return (
            <button
              key={series.provider}
              type="button"
              className={`legend-pill ${isFocused ? "is-focused" : ""} ${isDimmed ? "is-dimmed" : ""} ${totals.turns === 0 ? "is-idle" : ""}`}
              aria-pressed={isFocused}
              onMouseEnter={() => setHoveredProvider(series.provider)}
              onMouseLeave={() => setHoveredProvider(null)}
              onClick={() =>
                setFocusedProvider((prev) => (prev === series.provider ? null : series.provider))
              }
            >
              <i
                className={`legend-swatch shape-${series.marker}`}
                style={{ background: series.colorVar }}
              />
              <span className="legend-pill-name">{series.label}</span>
              <span className="legend-pill-stat">{summaryBadge}</span>
            </button>
          );
        })}
        {focusedProvider && (
          <button
            type="button"
            className="legend-reset-btn"
            onClick={() => setFocusedProvider(null)}
          >
            显示全部
          </button>
        )}
      </div>

      <div className="usage-line-chart" onMouseLeave={() => setActiveDate(null)}>
        <svg
          className="usage-line-svg"
          viewBox="0 0 900 220"
          role="group"
          aria-label={`${metricTitle}，可聚焦各时间点查看明细。`}
        >
          {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
            const value = tickValueAt(ratio);
            const position = bottom - ratio * plotHeight;
            return (
              <g key={ratio}>
                <line
                  className={`usage-chart-grid ${ratio === 0 ? "is-baseline" : ""}`}
                  x1={left}
                  x2={right}
                  y1={position}
                  y2={position}
                />
                <text
                  className="usage-chart-label"
                  x={left - 10}
                  y={position + 4}
                  textAnchor="end"
                >
                  {formatAxisValue(value, metric, ceiling)}
                </text>
              </g>
            );
          })}

          {activeSeriesList.map((series) => {
            const periodTurns = seriesTotals[series.provider].turns;
            const isHighlighted =
              focusedProvider === series.provider || hoveredProvider === series.provider;
            const isFaded =
              hoveredProvider !== null && hoveredProvider !== series.provider && !focusedProvider;
            const coords = daily.map((point, index) => {
              const turns = point[series.turnsKey] ?? 0;
              const val = pointMetricValue(point, series, metric);
              return {
                point,
                index,
                turns,
                val,
                hasActivity: turns > 0 || val > 0,
                cx: x(index),
                cy: yFor(val, turns > 0),
              };
            });

            // Skip drawing flat 0 baseline clutter for completely idle providers unless hovered/focused.
            const showLine = periodTurns > 0 || maxVal > 0 && isHighlighted;

            return (
              <g
                key={series.provider}
                style={{
                  opacity: isFaded ? 0.22 : periodTurns === 0 && !isHighlighted ? 0.3 : 1,
                  transition: "opacity 150ms ease",
                }}
              >
                {showLine && (
                  <polyline
                    points={coords.map((c) => `${c.cx},${c.cy}`).join(" ")}
                    fill="none"
                    stroke={series.colorVar}
                    strokeWidth={isHighlighted ? "3" : "2.35"}
                    strokeDasharray={series.dash}
                    strokeLinejoin="round"
                    strokeLinecap="round"
                  />
                )}
                {coords.map((c) => {
                  const isPointHovered = activeDate === c.point.date;
                  if (!c.hasActivity && !isPointHovered) return null;
                  if (!c.hasActivity && isPointHovered) {
                    return (
                      <circle
                        key={c.point.date}
                        cx={c.cx}
                        cy={bottom}
                        r={2.2}
                        fill="var(--line)"
                      />
                    );
                  }
                  return (
                    <SeriesMarker
                      key={c.point.date}
                      marker={series.marker}
                      cx={c.cx}
                      cy={c.cy}
                      size={isPointHovered ? 4.6 : 3.4}
                      color={series.colorVar}
                      emphasized={isPointHovered || isHighlighted}
                    />
                  );
                })}
              </g>
            );
          })}

          {active && (
            <line
              className="usage-chart-guide"
              x1={x(activeIndex)}
              x2={x(activeIndex)}
              y1={top}
              y2={bottom}
            />
          )}

          {daily.map((point, index) => {
            const totalCost = activeSeriesList.reduce((sum, s) => sum + (point[s.costKey] ?? 0), 0);
            const totalTurns = activeSeriesList.reduce(
              (sum, s) => sum + (point[s.turnsKey] ?? 0),
              0,
            );
            const description = `${point.date}：合计 ${formatUsd(totalCost)}（${totalTurns} 轮）`;
            const hitLeft = index === 0 ? left : (x(index - 1) + x(index)) / 2;
            const hitRight =
              index === daily.length - 1 ? right : (x(index) + x(index + 1)) / 2;
            const hasAnyTurn = totalTurns > 0;
            return (
              <g key={point.date}>
                {hasAnyTurn && (
                  <circle
                    cx={x(index)}
                    cy={bottom + 8}
                    r={2}
                    fill="var(--accent)"
                    opacity={activeDate === point.date ? 0.95 : 0.45}
                  />
                )}
                {(index % labelStep === 0 || index === daily.length - 1) && (
                  <text
                    className={`usage-chart-label ${hasAnyTurn ? "is-active-day" : ""}`}
                    x={x(index)}
                    y={211}
                    textAnchor="middle"
                  >
                    {point.date}
                  </text>
                )}
                <rect
                  className="usage-chart-hit"
                  x={hitLeft}
                  y={top}
                  width={Math.max(4, hitRight - hitLeft)}
                  height={bottom - top + 12}
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

        {maxVal === 0 && totalTurnsAll === 0 && !active && (
          <div className="usage-chart-zero">当前周期暂无调用记录</div>
        )}

        {active && (
          <div
            className={`usage-chart-tooltip ${
              activeIndex > (daily.length - 1) / 2 ? "align-right" : ""
            }`}
          >
            <div className="usage-tooltip-head">
              <strong>{granularity === "hour" ? `今日 ${active.date}` : active.date}</strong>
              <span>
                {activeSeriesList.reduce((sum, s) => sum + (active[s.turnsKey] ?? 0), 0)} 轮调用
              </span>
            </div>
            {activeSeriesList.map((series) => {
              const cost = active[series.costKey] ?? 0;
              const turns = active[series.turnsKey] ?? 0;
              const tokens = active[series.tokensKey] ?? 0;
              const used = turns > 0 || cost > 0 || tokens > 0;
              return (
                <div key={series.provider} className={`usage-tooltip-row ${used ? "" : "is-zero"}`}>
                  <span className="usage-tooltip-agent">
                    <i
                      className={`legend-swatch shape-${series.marker}`}
                      style={{ background: series.colorVar }}
                    />
                    <span>{series.label}</span>
                  </span>
                  {used ? (
                    <span className="usage-tooltip-vals">
                      <small>
                        {turns} 轮{tokens > 0 ? ` / ${formatTokens(tokens)}` : ""}
                      </small>
                      <b>{formatUsd(cost)}</b>
                    </span>
                  ) : (
                    <span className="usage-tooltip-idle">未调用</span>
                  )}
                </div>
              );
            })}
            <div className="usage-chart-total">
              <span>
                合计
                {(() => {
                  const sumTok = activeSeriesList.reduce(
                    (sum, s) => sum + (active[s.tokensKey] ?? 0),
                    0,
                  );
                  return sumTok > 0 ? ` (${formatTokens(sumTok)})` : "";
                })()}
              </span>
              <b>
                {formatUsd(
                  activeSeriesList.reduce((sum, s) => sum + (active[s.costKey] ?? 0), 0),
                )}
              </b>
            </div>
          </div>
        )}
      </div>
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
  const fiveHourWindow = isCodex
    ? data.codexLimits.windows.find((w) => w.kind === "five_hour")
    : undefined;
  const weeklyWindow = isCodex
    ? data.codexLimits.windows.find((w) => w.kind === "weekly" || w.kind === "custom")
    : undefined;
  const primaryWindow = fiveHourWindow ?? weeklyWindow;
  const totalTokens = agent.inputTokens + agent.outputTokens;
  const cacheRate =
    agent.inputTokens > 0 ? Math.min(100, (agent.cachedTokens / agent.inputTokens) * 100) : 0;
  const costShare =
    data.summary.totalCostUsd > 0
      ? Math.min(100, (agent.costUsd / data.summary.totalCostUsd) * 100)
      : 0;
  const avgCostPerTurn = agent.turnsRange > 0 ? agent.costUsd / agent.turnsRange : 0;
  const accentColor = providerColor(agent.providerKind);

  return (
    <div className={`usage-provider-card ${providerAccent(agent.providerKind)}`}>
      <div className="usage-provider-head">
        <Avatar agent={agent.id} small />
        <div>
          <div className="usage-provider-title-row">
            <i className="provider-color-dot" style={{ background: accentColor }} />
            <h3>{agent.displayName || providerLabel(agent.providerKind)}</h3>
          </div>
          <small>
            {agent.runtimeKind}
            {isCodex && data.codexLimits.planType ? ` / ${planLabel(data.codexLimits.planType)}` : ""}
            {agent.lastUsedAt ? ` / 最近 ${formatRelativeTime(agent.lastUsedAt)}` : ""}
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
            <strong
              style={{
                color: primaryWindow.remainingPercent <= 15 ? "#dc2626" : accentColor,
              }}
            >
              {Math.round(primaryWindow.remainingPercent)}%
            </strong>
          </>
        ) : (
          <>
            <span>本周期 Token 消耗</span>
            <strong style={{ color: accentColor }}>{formatTokens(totalTokens)}</strong>
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
        <div className="usage-metric-row">
          <span>输入 / 输出 Token</span>
          <strong>
            {formatTokens(agent.inputTokens)} / {formatTokens(agent.outputTokens)}
          </strong>
        </div>
        <div className="usage-metric-row">
          <span>缓存命中率</span>
          <strong>{agent.inputTokens > 0 ? `${cacheRate.toFixed(0)}%` : "—"}</strong>
        </div>
        <div className="usage-metric-row">
          <span>折算费用{agent.turnsRange > 0 ? " (单轮均价)" : ""}</span>
          <strong>
            {formatUsd(agent.costUsd)}
            {agent.turnsRange > 0 && (
              <small className="usage-inline-sub"> / {formatUsd(avgCostPerTurn)}</small>
            )}
          </strong>
        </div>
        {data.summary.totalCostUsd > 0 && (
          <div className="usage-share-bar-wrap" title={`占当前周期总折算费用的 ${costShare.toFixed(1)}%`}>
            <div className="usage-share-bar-meta">
              <span>费用占比</span>
              <span>{costShare < 0.1 && agent.costUsd > 0 ? "<0.1%" : `${costShare.toFixed(1)}%`}</span>
            </div>
            <div className="usage-share-bar-track">
              <div
                className="usage-share-bar-fill"
                style={{
                  width: `${agent.costUsd > 0 ? Math.max(2, costShare) : 0}%`,
                  background: accentColor,
                }}
              />
            </div>
          </div>
        )}
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
  const [modelSort, setModelSort] = useState<ModelSortKey>("cost");
  const fiveHourWindow = data.codexLimits.windows.find((w) => w.kind === "five_hour");
  const weeklyWindow = data.codexLimits.windows.find(
    (w) => w.kind === "weekly" || w.kind === "custom",
  );

  const totalTokens = data.summary.totalInputTokens + data.summary.totalOutputTokens;
  const freshInputTokens = Math.max(0, data.summary.totalInputTokens - data.summary.totalCachedTokens);
  const savedCost = data.summary.savedCostUsd ?? 0;
  const activeDays = data.summary.activeDays ?? 0;
  const failedTurns = data.summary.failedTurns ?? 0;
  const granularity: "hour" | "day" = data.granularity ?? (days === 1 ? "hour" : "day");

  const providerCostSegments = useMemo(() => {
    const total = data.summary.totalCostUsd;
    if (total <= 0) return [];
    return PROVIDER_SERIES.map((s) => {
      const sum = data.models
        .filter((m) => m.providerKind === s.provider)
        .reduce((acc, m) => acc + m.costUsd, 0);
      return {
        provider: s.provider,
        label: s.label,
        colorVar: s.colorVar,
        cost: sum,
        pct: (sum / total) * 100,
      };
    }).filter((seg) => seg.cost > 0);
  }, [data.models, data.summary.totalCostUsd]);

  const sortedModels = useMemo(() => {
    const copy: ModelUsageRow[] = [...data.models];
    copy.sort((a, b) => {
      if (modelSort === "turns") return b.turns - a.turns || b.costUsd - a.costUsd;
      if (modelSort === "tokens") {
        return (
          b.inputTokens + b.outputTokens - (a.inputTokens + a.outputTokens) ||
          b.costUsd - a.costUsd
        );
      }
      if (modelSort === "cache") return b.cacheHitRate - a.cacheHitRate || b.turns - a.turns;
      return b.costUsd - a.costUsd || b.turns - a.turns;
    });
    return copy;
  }, [data.models, modelSort]);

  const maxModelCost = useMemo(
    () => Math.max(0, ...data.models.map((m) => m.costUsd)),
    [data.models],
  );

  const rangeLabel = days === 1 ? "今日" : `近 ${days} 天`;

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
              { value: 14, label: "近 14 天" },
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
          <div className="kpi-label">API 等价折算费用（{rangeLabel}）</div>
          <div className="kpi-value">{formatUsd(data.summary.totalCostUsd)}</div>
          {providerCostSegments.length > 0 && (
            <div className="kpi-mini-bar" aria-label="各厂商成本构成">
              {providerCostSegments.map((seg) => (
                <span
                  key={seg.provider}
                  style={{
                    width: `${Math.max(3, seg.pct)}%`,
                    background: seg.colorVar,
                  }}
                  title={`${seg.label}: ${formatUsd(seg.cost)} (${seg.pct.toFixed(1)}%)`}
                />
              ))}
            </div>
          )}
          <div className="kpi-sub">
            {activeDays > 1
              ? `活跃 ${activeDays} 天 / 活跃日均 ${formatUsd(data.summary.totalCostUsd / activeDays)}`
              : "基于各模型公开 API 单价估算"}
          </div>
        </div>

        <div className="kpi-card">
          <div className="kpi-label">累计 Token 消耗</div>
          <div className="kpi-value">{formatTokens(totalTokens)}</div>
          <div className="kpi-sub">
            新输入 {formatTokens(freshInputTokens)} / 缓存 {formatTokens(data.summary.totalCachedTokens)} / 输出{" "}
            {formatTokens(data.summary.totalOutputTokens)}
          </div>
        </div>

        <div className="kpi-card">
          <div className="kpi-label">Prompt 缓存命中率</div>
          <div className="kpi-value-row">
            <span className="kpi-value">{data.summary.cacheHitRate.toFixed(1)}%</span>
            {savedCost > 0.001 && (
              <span className="kpi-saved-pill" title="相比全部按未缓存输入单价计费估算节省的金额">
                约省 {formatUsd(savedCost)}
              </span>
            )}
          </div>
          <div className="kpi-sub">已复用 {formatTokens(data.summary.totalCachedTokens)} 缓存上下文</div>
        </div>

        <div className="kpi-card">
          <div className="kpi-label">累计执行轮次</div>
          <div className="kpi-value">{data.summary.totalTurns} 轮</div>
          <div className="kpi-sub">
            群聊 {data.summary.groupTurns} 轮 / 私聊 {data.summary.privateTurns} 轮
            {failedTurns > 0 ? ` / 异常 ${failedTurns} 轮` : ""}
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
        <UsageTrendChart daily={data.daily} granularity={granularity} />
      </div>

      <div className="usage-card">
        <div className="usage-card-header usage-table-header">
          <div>
            <div className="usage-card-title">模型维度明细</div>
            <span className="usage-card-sub">来自本机 SQLite usage_records，点击表头可切换排序</span>
          </div>
          {data.models.length > 1 && (
            <div className="usage-mini-seg" role="group" aria-label="模型排序方式">
              {[
                { key: "cost" as const, label: "按费用" },
                { key: "turns" as const, label: "按轮次" },
                { key: "tokens" as const, label: "按 Token" },
                { key: "cache" as const, label: "按缓存率" },
              ].map((item) => (
                <button
                  key={item.key}
                  type="button"
                  className={modelSort === item.key ? "active" : ""}
                  onClick={() => setModelSort(item.key)}
                >
                  {item.label}
                </button>
              ))}
            </div>
          )}
        </div>
        {sortedModels.length === 0 ? (
          <div className="usage-empty">
            当前周期内暂无已记录的模型调用。在私聊或群聊中发送消息后，用量将自动记录在这里。
          </div>
        ) : (
          <div className="usage-table-wrap">
            <table className="usage-table">
              <thead>
                <tr>
                  <th>Agent / 模型</th>
                  <th className="num">轮次</th>
                  <th className="num">总输入 Token</th>
                  <th className="num">输出 Token</th>
                  <th className="num">缓存命中</th>
                  <th className="num">单轮均价</th>
                  <th className="num">折算费用与占比</th>
                </tr>
              </thead>
              <tbody>
                {sortedModels.map((row) => {
                  const color = providerColor(row.providerKind);
                  const pctOfTotal =
                    data.summary.totalCostUsd > 0
                      ? (row.costUsd / data.summary.totalCostUsd) * 100
                      : 0;
                  const barPct = maxModelCost > 0 ? (row.costUsd / maxModelCost) * 100 : 0;
                  const avgTurn = row.turns > 0 ? row.costUsd / row.turns : 0;
                  return (
                    <tr key={`${row.agentId}:${row.model}`}>
                      <td>
                        <div className="usage-model-cell">
                          <i className="provider-color-dot" style={{ background: color }} />
                          <div>
                            <strong>{providerLabel(row.providerKind)}</strong>
                            <span className="usage-model-sep">/</span>
                            <span className="usage-model-id">{row.model || "默认模型"}</span>
                          </div>
                        </div>
                      </td>
                      <td className="num">{row.turns}</td>
                      <td className="num">{row.inputTokens.toLocaleString()}</td>
                      <td className="num">{row.outputTokens.toLocaleString()}</td>
                      <td className="num">{row.cacheHitRate.toFixed(0)}%</td>
                      <td className="num usage-muted-num">{formatUsd(avgTurn)}</td>
                      <td className="num">
                        <div className="usage-cost-cell">
                          <div className="usage-cost-cell-top">
                            <strong>{formatUsd(row.costUsd)}</strong>
                            <small>
                              {row.costUsd > 0 && pctOfTotal < 0.1
                                ? "<0.1%"
                                : `${pctOfTotal.toFixed(1)}%`}
                            </small>
                          </div>
                          <div className="usage-cost-cell-bar">
                            <span
                              style={{
                                width: `${row.costUsd > 0 ? Math.max(3, barPct) : 0}%`,
                                background: color,
                              }}
                            />
                          </div>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
              {sortedModels.length > 1 && (
                <tfoot>
                  <tr>
                    <td>
                      <strong>合计 ({sortedModels.length} 个模型)</strong>
                    </td>
                    <td className="num">
                      <strong>{data.summary.totalTurns}</strong>
                    </td>
                    <td className="num">{data.summary.totalInputTokens.toLocaleString()}</td>
                    <td className="num">{data.summary.totalOutputTokens.toLocaleString()}</td>
                    <td className="num">{data.summary.cacheHitRate.toFixed(1)}%</td>
                    <td className="num usage-muted-num">
                      {data.summary.totalTurns > 0
                        ? formatUsd(data.summary.totalCostUsd / data.summary.totalTurns)
                        : "$0.00"}
                    </td>
                    <td className="num">
                      <strong>{formatUsd(data.summary.totalCostUsd)}</strong>
                    </td>
                  </tr>
                </tfoot>
              )}
            </table>
          </div>
        )}
      </div>
    </div>
  );

  if (embedded) return content;
  return <main className="usage-workspace">{content}</main>;
}
