import { useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import type { DailyUsagePoint, RateLimitWindow, UsageDashboardData } from "./api";

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
  if (days > 0) return `${days} 天 ${hours} 小时后重置 · ${dateStr}`;
  if (hours > 0) return `${hours} 小时 ${mins} 分钟后重置 · ${dateStr}`;
  return `${Math.max(1, mins)} 分钟后重置 · ${dateStr}`;
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
  return kind;
}

function RingProgress({ percent, color, size = 20 }: { percent: number; color: string; size?: number }) {
  const r = 9;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - Math.max(0, Math.min(100, percent)) / 100);
  return (
    <svg className="quota-ring-svg" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r={r} fill="none" stroke="#e2e8f0" strokeWidth="3" />
      <circle
        cx="12"
        cy="12"
        r={r}
        fill="none"
        stroke={color}
        strokeWidth="3"
        strokeDasharray={c.toFixed(2)}
        strokeDashoffset={offset.toFixed(2)}
        strokeLinecap="round"
      />
    </svg>
  );
}

export function QuotaQuickPopover({
  data,
  onOpenFull,
  onClose,
}: {
  data: UsageDashboardData;
  onOpenFull: () => void;
  onClose: () => void;
}) {
  const codexWindows = data.codexLimits.windows;
  const deepseek = data.agents.find((a) => a.providerKind === "deepseek");
  const gemini = data.agents.find((a) => a.providerKind === "gemini");

  return (
    <div className="quick-popover" role="dialog" aria-label="使用量速览">
      <div className="popover-title">
        <span>使用量速览</span>
        <span style={{ fontWeight: 400 }}>
          {data.updatedAt ? new Date(data.updatedAt * 1000).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) : "本地统计"}
        </span>
      </div>

      {data.codexLimits.available && (
        <div className="popover-group">
          <div className="popover-group-name">
            <span>Codex ({planLabel(data.codexLimits.planType)})</span>
            <span className="usage-tag usage-tag-live">实时</span>
          </div>
          {codexWindows.length > 0 ? (
            codexWindows.map((w) => (
              <div className="popover-row" key={w.kind}>
                <div>
                  <div className="popover-row-label">{w.label}</div>
                  <div className="popover-row-sub">{formatCountdown(w.resetsAt)}</div>
                </div>
                <div className="popover-row-val">
                  <span>{Math.round(w.remainingPercent)}% 剩余</span>
                  <RingProgress percent={w.remainingPercent} color="var(--accent)" />
                </div>
              </div>
            ))
          ) : (
            <div className="popover-row">
              <div className="popover-row-label">计费模式</div>
              <div className="popover-row-val" style={{ color: "var(--muted)" }}>无周期限额</div>
            </div>
          )}
        </div>
      )}

      {deepseek && (
        <div className="popover-group">
          <div className="popover-group-name">
            <span>DeepSeek</span>
            <span className="usage-tag usage-tag-api">本地统计</span>
          </div>
          <div className="popover-row">
            <div>
              <div className="popover-row-label">今日 / 本周期轮次</div>
              <div className="popover-row-sub">
                {deepseek.connected ? "Harness 已就绪" : "未连接"}
              </div>
            </div>
            <div className="popover-row-val">
              {deepseek.turnsToday} / {deepseek.turnsRange} 轮
            </div>
          </div>
        </div>
      )}

      {gemini && (
        <div className="popover-group" style={{ marginBottom: 4 }}>
          <div className="popover-group-name">
            <span>Gemini (ACP)</span>
            <span className="usage-tag usage-tag-local">健康状态</span>
          </div>
          <div className="popover-row">
            <div>
              <div className="popover-row-label">运行状态</div>
              <div className="popover-row-sub">
                今日 {gemini.turnsToday} 轮 · {gemini.quotaExhaustedRecent ? "近期触发过限流" : "未触发限流"}
              </div>
            </div>
            <div
              className="popover-row-val"
              style={{ color: gemini.quotaExhaustedRecent ? "#dc2626" : gemini.connected ? "#047857" : "var(--muted)" }}
            >
              {gemini.quotaExhaustedRecent ? "额度受限" : gemini.connected ? "正常" : "未连接"}
            </div>
          </div>
        </div>
      )}

      <div className="popover-footer">
        <span style={{ fontSize: 11, color: "var(--muted)" }}>快捷键 ⌘U</span>
        <button
          type="button"
          className="popover-link"
          onClick={() => {
            onClose();
            onOpenFull();
          }}
        >
          查看完整看板 →
        </button>
      </div>
    </div>
  );
}

export function SidebarQuotaCard({
  data,
  onClick,
}: {
  data: UsageDashboardData;
  onClick: () => void;
}) {
  const codexPrimary = data.codexLimits.windows[0];
  const deepseek = data.agents.find((a) => a.providerKind === "deepseek");
  const gemini = data.agents.find((a) => a.providerKind === "gemini");

  if (!codexPrimary && !deepseek && !gemini) return null;

  return (
    <button type="button" className="sidebar-quota-card" onClick={onClick}>
      <div className="sq-head">
        <span>额度与用量</span>
        <span style={{ color: "var(--accent)" }}>详情 ↗</span>
      </div>
      {codexPrimary && (
        <div className="sq-row">
          <span className="sq-name">Codex</span>
          <div className="sq-bar">
            <div className="sq-fill" style={{ width: `${codexPrimary.remainingPercent}%`, background: "var(--accent)" }} />
          </div>
          <span className="sq-val">{Math.round(codexPrimary.remainingPercent)}%</span>
        </div>
      )}
      {deepseek && (
        <div className="sq-row">
          <span className="sq-name">DeepSeek</span>
          <div className="sq-bar">
            <div className="sq-fill" style={{ width: deepseek.connected ? "100%" : "0%", background: "#0891b2" }} />
          </div>
          <span className="sq-val">{deepseek.turnsToday} 轮</span>
        </div>
      )}
      {gemini && (
        <div className="sq-row">
          <span className="sq-name">Gemini</span>
          <div className="sq-bar">
            <div
              className="sq-fill"
              style={{
                width: gemini.connected ? "100%" : "0%",
                background: gemini.quotaExhaustedRecent ? "#dc2626" : "#10b981",
              }}
            />
          </div>
          <span
            className="sq-val"
            style={{
              color: gemini.quotaExhaustedRecent
                ? "#dc2626"
                : gemini.connected
                  ? "#047857"
                  : "var(--muted)",
            }}
          >
            {gemini.quotaExhaustedRecent
              ? "受限"
              : gemini.connected
                ? "正常"
                : gemini.availability === "checking"
                  ? "检测中"
                  : "离线"}
          </span>
        </div>
      )}
    </button>
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
              <span className="usage-tag usage-tag-live">app-server 实时</span>
              <span className="usage-plan-badge">{planLabel(planType)}</span>
            </div>
            <div className="limit-hero-reset">{formatCountdown(win.resetsAt)}</div>
          </div>
        </div>
        <div className="limit-hero-score">
          <strong style={{ color: barColor }}>{remaining}%</strong>
          <span>剩余 · 已用 {used}%</span>
        </div>
      </div>

      <div className="progress-track progress-track-lg">
        <div className="progress-fill" style={{ width: `${remaining}%`, background: barColor }} />
      </div>

      {resetCreditsCount > 0 && (
        <div className="limit-credits-bar">
          <span>✨ 当前账号有 <strong>{resetCreditsCount} 次</strong> 免费重置额度（Full reset）可用</span>
        </div>
      )}
    </div>
  );
}

const costSeries = [
  { key: "codexCost", label: "Codex", color: "var(--accent)", dash: undefined },
  { key: "deepseekCost", label: "DeepSeek", color: "#0891b2", dash: "8 4" },
  { key: "geminiCost", label: "Gemini", color: "#10b981", dash: "2 4" },
] as const;

function DailyCostChart({ daily }: { daily: DailyUsagePoint[] }) {
  const [activeDate, setActiveDate] = useState<string | null>(null);
  const activeIndex = daily.findIndex((point) => point.date === activeDate);
  const active = daily[activeIndex];
  const left = 72;
  const right = 876;
  const top = 20;
  const bottom = 184;
  const maxCost = Math.max(0, ...daily.flatMap((point) => costSeries.map((series) => point[series.key])));
  const ceiling = maxCost > 0 ? maxCost * 1.15 : 1;
  const x = (index: number) => daily.length <= 1 ? (left + right) / 2 : left + index * (right - left) / (daily.length - 1);
  const y = (cost: number) => bottom - cost / ceiling * (bottom - top);
  const labelStep = Math.max(1, Math.ceil((daily.length - 1) / 6));
  const tickDecimals = Math.max(2, Math.min(8, Math.ceil(-Math.log10(ceiling / 4)) + 1));

  if (daily.length === 0) return <div className="usage-empty">当前周期暂无成本数据。</div>;

  return (
    <div className="usage-line-chart" onMouseLeave={() => setActiveDate(null)}>
      <svg className="usage-line-svg" viewBox="0 0 900 222" role="group" aria-label="每日折算成本折线图，单位美元。可聚焦各日期查看明细。">
        {[0, 1, 2, 3, 4].map((tick) => {
          const value = ceiling * tick / 4;
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
              points={daily.map((point, index) => `${x(index)},${y(point[series.key])}`).join(" ")}
              fill="none"
              stroke={series.color}
              strokeWidth="2.5"
              strokeDasharray={series.dash}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
            {daily.map((point, index) => (
              <circle key={point.date} cx={x(index)} cy={y(point[series.key])} r={activeDate === point.date ? 4.5 : 3}
                fill="var(--surface)" stroke={series.color} strokeWidth="2" />
            ))}
          </g>
        ))}
        {active && <line className="usage-chart-guide" x1={x(activeIndex)} x2={x(activeIndex)} y1={top} y2={bottom} />}
        {daily.map((point, index) => {
          const total = point.codexCost + point.deepseekCost + point.geminiCost;
          const description = `${point.date}：Codex ${formatUsd(point.codexCost)}，DeepSeek ${formatUsd(point.deepseekCost)}，Gemini ${formatUsd(point.geminiCost)}，合计 ${formatUsd(total)}（${point.codexTurns + point.deepseekTurns + point.geminiTurns} 轮）`;
          const hitLeft = index === 0 ? left : (x(index - 1) + x(index)) / 2;
          const hitRight = index === daily.length - 1 ? right : (x(index) + x(index + 1)) / 2;
          return (
            <g key={point.date}>
              {(index % labelStep === 0 || index === daily.length - 1) && (
                <text className="usage-chart-label" x={x(index)} y={210} textAnchor="middle">{point.date}</text>
              )}
              <rect className="usage-chart-hit" x={hitLeft} y={top} width={hitRight - hitLeft} height={bottom - top + 6}
                tabIndex={0} role="img" aria-label={description}
                onMouseEnter={() => setActiveDate(point.date)} onFocus={() => setActiveDate(point.date)} onBlur={() => setActiveDate(null)} />
            </g>
          );
        })}
      </svg>
      {maxCost === 0 && !active && <div className="usage-chart-zero">当前周期折算成本为 $0.00</div>}
      {active && (
        <div className={`usage-chart-tooltip ${activeIndex > (daily.length - 1) / 2 ? "align-right" : ""}`}>
          <strong>{active.date}</strong>
          {costSeries.map((series) => (
            <div key={series.key}><span><i className="legend-dot" style={{ background: series.color }} />{series.label}</span><b>{formatUsd(active[series.key])}</b></div>
          ))}
          <div className="usage-chart-total"><span>合计 · {active.codexTurns + active.deepseekTurns + active.geminiTurns} 轮</span><b>{formatUsd(active.codexCost + active.deepseekCost + active.geminiCost)}</b></div>
        </div>
      )}
    </div>
  );
}

export default function UsageDashboard({
  data,
  days,
  loading,
  onChangeDays,
  onRefresh,
}: {
  data: UsageDashboardData;
  days: number;
  loading: boolean;
  onChangeDays: (days: number) => void;
  onRefresh: () => void;
}) {
  const [tab, setTab] = useState<"limits" | "overview">("limits");

  const fiveHourWindow = data.codexLimits.windows.find((w) => w.kind === "five_hour");
  const weeklyWindow = data.codexLimits.windows.find((w) => w.kind === "weekly" || w.kind === "custom");
  const codexAgent = data.agents.find((a) => a.providerKind === "codex");
  const deepseekAgent = data.agents.find((a) => a.providerKind === "deepseek");
  const geminiAgent = data.agents.find((a) => a.providerKind === "gemini");

  return (
    <main className="usage-workspace">
      <div className="usage-container">
        <div className="usage-header">
          <div>
            <h1>
              <span className="usage-title-icon">
                <Icon name="chart" size={18} />
              </span>
              使用量与额度看板
              <span className="usage-kbd">⌘U</span>
            </h1>
          </div>
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
              {loading ? "刷新中…" : "刷新额度"}
            </button>
          </div>
        </div>

        <div className="usage-tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "limits"}
            className={tab === "limits" ? "active" : ""}
            onClick={() => setTab("limits")}
          >
            限额窗口 (Limits) <span className="usage-tab-badge">实时</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "overview"}
            className={tab === "overview" ? "active" : ""}
            onClick={() => setTab("overview")}
          >
            用量与折算成本 (Overview)
          </button>
        </div>

        {tab === "limits" ? (
          <section>
            {(fiveHourWindow || weeklyWindow) && (
              <div className={`window-grid ${fiveHourWindow && weeklyWindow ? "" : "single"}`}>
                {fiveHourWindow && (
                  <WindowCard
                    title="5 小时滚动限额"
                    subtitle="按最快恢复时间排序"
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

            <h2 className="usage-section-title">
              <span>各接入 Agent 额度明细</span>
            </h2>

            <div className="usage-provider-grid">
              <div className="usage-provider-card accent-codex">
                <div className="usage-provider-head">
                  <Avatar agent="codex" small />
                  <div>
                    <h3>Codex</h3>
                    <small>codex-app-server · {planLabel(data.codexLimits.planType)}</small>
                  </div>
                  <span className={`usage-status-pill ${codexAgent?.connected ? "ok" : "muted"}`}>
                    {codexAgent?.connected ? "已连接" : "未连接"}
                  </span>
                </div>

                <div className="usage-highlight-box">
                  <span>{fiveHourWindow ? "5 小时剩余" : "周限额剩余"}</span>
                  <strong style={{ color: "var(--accent)" }}>
                    {fiveHourWindow
                      ? `${Math.round(fiveHourWindow.remainingPercent)}%`
                      : weeklyWindow
                        ? `${Math.round(weeklyWindow.remainingPercent)}%`
                        : data.codexLimits.available
                          ? "无周期限额"
                          : "未读取"}
                  </strong>
                </div>

                <div className="usage-metric-list">
                  {fiveHourWindow && weeklyWindow && (
                    <div className="usage-metric-row">
                      <span>周限额剩余</span>
                      <strong>{Math.round(weeklyWindow.remainingPercent)}%</strong>
                    </div>
                  )}
                  <div className="usage-metric-row">
                    <span>今日 / 周期轮次</span>
                    <strong>
                      {codexAgent?.turnsToday ?? 0} 轮 / {codexAgent?.turnsRange ?? 0} 轮
                    </strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>Token 消耗</span>
                    <strong>
                      {formatTokens((codexAgent?.inputTokens ?? 0) + (codexAgent?.outputTokens ?? 0))}
                    </strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>折算费用</span>
                    <strong>{formatUsd(codexAgent?.costUsd ?? 0)}</strong>
                  </div>
                </div>

                <div className="usage-provider-note">
                  {data.codexLimits.available
                    ? data.codexLimits.resetCreditsCount > 0
                      ? `已通过 account/rateLimits/read 实时读取；当前有 ${data.codexLimits.resetCreditsCount} 次免费重置额度可用。`
                      : "已通过 account/rateLimits/read 实时读取当前账号限流窗口。"
                    : data.codexLimits.error || "连接 Codex 后将自动读取实时限流窗口。"}
                </div>
              </div>

              <div className="usage-provider-card accent-deepseek">
                <div className="usage-provider-head">
                  <Avatar agent="deepseek" small />
                  <div>
                    <h3>DeepSeek</h3>
                    <small>deepseek-harness-sdk</small>
                  </div>
                  <span className={`usage-status-pill ${deepseekAgent?.connected ? "ok" : "muted"}`}>
                    {deepseekAgent?.connected ? "已连接" : "未连接"}
                  </span>
                </div>

                <div className="usage-highlight-box">
                  <span>计费与限额模式</span>
                  <strong style={{ color: "#0891b2" }}>Harness 托管</strong>
                </div>

                <div className="usage-metric-list">
                  <div className="usage-metric-row">
                    <span>今日 / 周期轮次</span>
                    <strong>
                      {deepseekAgent?.turnsToday ?? 0} 轮 / {deepseekAgent?.turnsRange ?? 0} 轮
                    </strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>Token 消耗</span>
                    <strong>
                      {formatTokens((deepseekAgent?.inputTokens ?? 0) + (deepseekAgent?.outputTokens ?? 0))}
                    </strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>折算费用</span>
                    <strong>{formatUsd(deepseekAgent?.costUsd ?? 0)}</strong>
                  </div>
                </div>

                <div className="usage-provider-note">
                  统计来自每次 DeepSeek Harness 对话结束时写入本机的实际用量记录。
                </div>
              </div>

              <div className="usage-provider-card accent-gemini">
                <div className="usage-provider-head">
                  <Avatar agent="gemini" small />
                  <div>
                    <h3>Gemini</h3>
                    <small>agy acp / gemini-cli-acp</small>
                  </div>
                  <span
                    className={`usage-status-pill ${
                      geminiAgent?.quotaExhaustedRecent
                        ? "warn"
                        : geminiAgent?.connected
                          ? "ok"
                          : "muted"
                    }`}
                  >
                    {geminiAgent?.quotaExhaustedRecent
                      ? "近期限流"
                      : geminiAgent?.connected
                        ? "状态正常"
                        : "未连接"}
                  </span>
                </div>

                <div className="usage-highlight-box">
                  <span>ACP 限流状态</span>
                  <strong style={{ color: geminiAgent?.quotaExhaustedRecent ? "#d97706" : "#059669" }}>
                    {geminiAgent?.quotaExhaustedRecent ? "额度受限" : "运行正常"}
                  </strong>
                </div>

                <div className="usage-metric-list">
                  <div className="usage-metric-row">
                    <span>剩余百分比</span>
                    <strong style={{ color: "var(--muted)" }}>ACP 暂未开放</strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>今日 / 周期轮次</span>
                    <strong>
                      {geminiAgent?.turnsToday ?? 0} 轮 / {geminiAgent?.turnsRange ?? 0} 轮
                    </strong>
                  </div>
                  <div className="usage-metric-row">
                    <span>Token 消耗</span>
                    <strong>
                      {formatTokens((geminiAgent?.inputTokens ?? 0) + (geminiAgent?.outputTokens ?? 0))}
                    </strong>
                  </div>
                </div>

                <div className="usage-provider-note">
                  ACP 协议暂未公开账号剩余百分比；若触发 <code>quota_exhausted</code> 会在此处自动标黄提醒。
                </div>
              </div>
            </div>
          </section>
        ) : (
          <section>
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
                  输入 {formatTokens(data.summary.totalInputTokens)} · 输出{" "}
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
                  群聊 {data.summary.groupTurns} 轮 · 私聊 {data.summary.privateTurns} 轮
                </div>
              </div>
            </div>

            <div className="usage-card" style={{ marginBottom: 20 }}>
              <div className="usage-card-header">
                <div className="usage-card-title">每日折算成本趋势 (USD)</div>
                <div className="chart-legend">
                  <span><i className="legend-dot" style={{ background: "var(--accent)" }} />Codex</span>
                  <span><i className="legend-dot" style={{ background: "#0891b2" }} />DeepSeek</span>
                  <span><i className="legend-dot" style={{ background: "#10b981" }} />Gemini</span>
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
                      <th className="num">输入 Token</th>
                      <th className="num">输出 Token</th>
                      <th className="num">缓存命中</th>
                      <th className="num">折算费用</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.models.map((row) => (
                      <tr key={`${row.agentId}:${row.model}`}>
                        <td>
                          <strong>{providerLabel(row.providerKind)}</strong> · {row.model || "默认模型"}
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
          </section>
        )}
      </div>
    </main>
  );
}
