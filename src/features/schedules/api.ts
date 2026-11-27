import { invoke } from "@tauri-apps/api/core";
import { desktop } from "../chats/api";

export type ScheduleKind = "interval" | "daily" | "weekly" | "cron";
export type TargetType = "agent" | "project" | "group";

export interface ScheduledTask {
  id: string;
  name: string;
  prompt: string;
  enabled: boolean;
  schedule_kind: ScheduleKind;
  interval_minutes: number | null;
  time_of_day: string | null;
  weekdays: string | null;
  cron_expr: string | null;
  target_type: TargetType;
  agent_id: string | null;
  project_id: string | null;
  group_id: string | null;
  model: string;
  reasoning_effort: string | null;
  service_tier: string | null;
  source: string;
  source_external_id: string | null;
  next_run_at: number | null;
  last_run_at: number | null;
  last_status: string | null;
  last_error: string | null;
  run_count: number;
  created_at: string;
  updated_at: string;
}

export interface ScheduledTaskRun {
  id: string;
  task_id: string;
  trigger_kind: "scheduled" | "manual";
  status: "dispatched" | "queued" | "failed" | "skipped";
  conversation_id: string | null;
  target_summary: string;
  error: string | null;
  started_at: number;
  finished_at: number | null;
}

export interface ScheduledTaskInput {
  id?: string;
  name: string;
  prompt: string;
  enabled: boolean;
  schedule_kind: ScheduleKind;
  interval_minutes?: number | null;
  time_of_day?: string | null;
  weekdays?: string | null;
  cron_expr?: string | null;
  target_type: TargetType;
  agent_id?: string | null;
  project_id?: string | null;
  group_id?: string | null;
  model?: string | null;
  reasoning_effort?: string | null;
  service_tier?: string | null;
}

export interface CodexAutomationCandidate {
  id: string;
  name: string;
  prompt: string;
  status: string;
  rrule: string;
  cwds: string[];
  model: string | null;
  reasoning_effort: string | null;
  last_run_at: number | null;
  next_run_at: number | null;
  schedule_kind: ScheduleKind;
  interval_minutes: number | null;
  time_of_day: string | null;
  weekdays: string | null;
  cron_expr: string | null;
  already_imported: boolean;
}

export interface CodexScanResult {
  found: boolean;
  db_path: string | null;
  items: CodexAutomationCandidate[];
}

export async function listSchedules(): Promise<ScheduledTask[]> {
  if (!desktop) return [];
  return invoke<ScheduledTask[]>("schedule_list");
}

export async function saveSchedule(input: ScheduledTaskInput): Promise<ScheduledTask> {
  return invoke<ScheduledTask>("schedule_save", { input });
}

export async function toggleSchedule(id: string, enabled: boolean): Promise<ScheduledTask> {
  return invoke<ScheduledTask>("schedule_toggle", { id, enabled });
}

export async function deleteSchedule(id: string): Promise<void> {
  return invoke<void>("schedule_delete", { id });
}

export async function runScheduleNow(id: string): Promise<ScheduledTaskRun> {
  return invoke<ScheduledTaskRun>("schedule_run_now", { id });
}

export async function listScheduleRuns(taskId?: string, limit = 50): Promise<ScheduledTaskRun[]> {
  if (!desktop) return [];
  return invoke<ScheduledTaskRun[]>("schedule_runs", { taskId: taskId ?? null, limit });
}

const WEEKDAY_LABELS: Record<string, string> = {
  "1": "周一",
  "2": "周二",
  "3": "周三",
  "4": "周四",
  "5": "周五",
  "6": "周六",
  "7": "周日",
};

export function formatScheduleRule(task: {
  schedule_kind: ScheduleKind;
  interval_minutes: number | null;
  time_of_day: string | null;
  weekdays: string | null;
  cron_expr: string | null;
}): string {
  switch (task.schedule_kind) {
    case "interval": {
      const mins = task.interval_minutes ?? 60;
      if (mins % 1440 === 0) return `每 ${mins / 1440} 天`;
      if (mins % 60 === 0) return `每 ${mins / 60} 小时`;
      return `每 ${mins} 分钟`;
    }
    case "daily":
      return `每天 ${task.time_of_day ?? "09:00"}`;
    case "weekly": {
      const days = (task.weekdays ?? "1")
        .split(",")
        .map(d => d.trim())
        .filter(Boolean);
      const label =
        days.join(",") === "1,2,3,4,5"
          ? "工作日"
          : days.join(",") === "6,7"
            ? "周末"
            : days.map(d => WEEKDAY_LABELS[d] ?? d).join("、");
      return `每周${label} ${task.time_of_day ?? "09:00"}`;
    }
    case "cron":
      return `Cron · ${task.cron_expr ?? ""}`;
    default:
      return "自定义周期";
  }
}

export function formatEpochTime(ts: number | null): string {
  if (!ts) return "—";
  const date = new Date(ts * 1000);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  const tomorrow = new Date(now);
  tomorrow.setDate(now.getDate() + 1);
  const isTomorrow =
    date.getFullYear() === tomorrow.getFullYear() &&
    date.getMonth() === tomorrow.getMonth() &&
    date.getDate() === tomorrow.getDate();
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  if (sameDay) return `今天 ${hh}:${mm}`;
  if (isTomorrow) return `明天 ${hh}:${mm}`;
  const m = date.getMonth() + 1;
  const d = date.getDate();
  return `${m}月${d}日 ${hh}:${mm}`;
}

export function formatRelativeTime(ts: number | null): string {
  if (!ts) return "";
  const diff = ts - Math.floor(Date.now() / 1000);
  const abs = Math.abs(diff);
  if (abs < 60) return diff >= 0 ? "1 分钟内" : "刚刚";
  if (abs < 3600) {
    const m = Math.round(abs / 60);
    return diff >= 0 ? `${m} 分钟后` : `${m} 分钟前`;
  }
  if (abs < 86400) {
    const h = Math.round(abs / 3600);
    return diff >= 0 ? `${h} 小时后` : `${h} 小时前`;
  }
  const d = Math.round(abs / 86400);
  return diff >= 0 ? `${d} 天后` : `${d} 天前`;
}
