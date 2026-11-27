import { invoke } from "@tauri-apps/api/core";
import { desktop } from "../chats/api";

export interface RateLimitWindow {
  kind: "five_hour" | "weekly" | "custom";
  label: string;
  usedPercent: number;
  remainingPercent: number;
  windowDurationMins: number;
  resetsAt: number | null;
}

export interface CodexLimits {
  available: boolean;
  error?: string;
  planType?: string | null;
  windows: RateLimitWindow[];
  resetCreditsCount: number;
}

export interface UsageSummary {
  totalCostUsd: number;
  savedCostUsd?: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCachedTokens: number;
  cacheHitRate: number;
  totalTurns: number;
  groupTurns: number;
  privateTurns: number;
  completedTurns?: number;
  failedTurns?: number;
  activeDays?: number;
}

export interface DailyUsagePoint {
  date: string;
  codexCost: number;
  deepseekCost: number;
  geminiCost: number;
  claudeCost: number;
  codexTurns: number;
  deepseekTurns: number;
  geminiTurns: number;
  claudeTurns: number;
  codexTokens?: number;
  deepseekTokens?: number;
  geminiTokens?: number;
  claudeTokens?: number;
}

export interface ModelUsageRow {
  providerKind: string;
  agentId: string;
  model: string;
  turns: number;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  cacheHitRate: number;
  costUsd: number;
}

export interface AgentUsageSummary {
  id: string;
  displayName: string;
  providerKind: string;
  runtimeKind: string;
  connected: boolean;
  availability: string;
  turnsToday: number;
  turnsRange: number;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  costUsd: number;
  quotaExhaustedRecent: boolean;
  lastQuotaErrorAt: number | null;
  lastUsedAt?: number | null;
}

export interface UsageDashboardData {
  updatedAt: number;
  rangeDays: number;
  granularity?: "hour" | "day";
  codexLimits: CodexLimits;
  summary: UsageSummary;
  daily: DailyUsagePoint[];
  models: ModelUsageRow[];
  agents: AgentUsageSummary[];
}

export const emptyUsageDashboard: UsageDashboardData = {
  updatedAt: 0,
  rangeDays: 7,
  granularity: "day",
  codexLimits: { available: false, windows: [], resetCreditsCount: 0 },
  summary: {
    totalCostUsd: 0,
    savedCostUsd: 0,
    totalInputTokens: 0,
    totalOutputTokens: 0,
    totalCachedTokens: 0,
    cacheHitRate: 0,
    totalTurns: 0,
    groupTurns: 0,
    privateTurns: 0,
    completedTurns: 0,
    failedTurns: 0,
    activeDays: 0,
  },
  daily: [],
  models: [],
  agents: [],
};

export async function getUsageDashboard(days = 7): Promise<UsageDashboardData> {
  if (!desktop) return emptyUsageDashboard;
  return invoke<UsageDashboardData>("usage_dashboard", { days });
}
