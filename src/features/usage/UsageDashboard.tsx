import { useMemo, useState } from "react";
import Icon from "../../components/Icon";
import type { ModelUsageRow, UsageDashboardData } from "./api";
import UsageTrendChart from "./UsageTrendChart";
import { AgentUsageCard, WindowCard } from "./UsageCards";
import {
  PROVIDER_SERIES,
  formatTokens,
  formatUsd,
  providerColor,
  providerLabel,
  type ModelSortKey,
} from "./usageFormat";

export { PROVIDER_SERIES, formatTokens, formatUsd, providerColor };

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
