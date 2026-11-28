import Avatar from "../../components/Avatar";
import type { AgentUsageSummary, RateLimitWindow, UsageDashboardData } from "./api";
import {
  formatCountdown,
  formatRelativeTime,
  formatTokens,
  formatUsd,
  planLabel,
  providerAccent,
  providerColor,
  providerLabel,
} from "./usageFormat";

export function WindowCard({
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

export function AgentUsageCard({
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
