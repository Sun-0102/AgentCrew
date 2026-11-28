import { useMemo, useState } from "react";
import type { DailyUsagePoint } from "./api";
import {
  PROVIDER_SERIES,
  formatAxisValue,
  formatTokens,
  formatUsd,
  pointMetricValue,
  providerLabel,
  type ChartMetric,
  type ChartScale,
  type SeriesConfig,
} from "./usageFormat";

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

export default function UsageTrendChart({
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
