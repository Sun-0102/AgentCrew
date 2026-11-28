import type { DailyUsagePoint } from "./api";

export type ChartMetric = "cost" | "turns" | "tokens";
export type ChartScale = "sqrt" | "linear";
export type ModelSortKey = "cost" | "turns" | "tokens" | "cache";

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

export type SeriesConfig = (typeof PROVIDER_SERIES)[number];

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

export function formatAxisValue(value: number, metric: ChartMetric, ceiling: number): string {
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

export function formatRelativeTime(unixSec?: number | null): string {
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

export function formatCountdown(resetsAt: number | null): string {
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

export function planLabel(planType?: string | null): string {
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

export function providerLabel(kind: string): string {
  if (kind === "codex") return "Codex";
  if (kind === "deepseek") return "DeepSeek";
  if (kind === "gemini") return "Gemini";
  if (kind === "claude") return "Claude Code";
  return kind;
}

export function providerAccent(kind: string): string {
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

export function pointMetricValue(point: DailyUsagePoint, series: SeriesConfig, metric: ChartMetric): number {
  if (metric === "turns") return point[series.turnsKey] ?? 0;
  if (metric === "tokens") return point[series.tokensKey] ?? 0;
  return point[series.costKey] ?? 0;
}
