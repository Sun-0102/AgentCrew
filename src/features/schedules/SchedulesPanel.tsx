import { useMemo, useState } from "react";
import Avatar from "../../components/Avatar";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { agentName, type Agent, type Group, type Probe } from "../chats/api";
import { type Project } from "../projects/api";
import {
  deleteSchedule,
  formatEpochTime,
  formatRelativeTime,
  formatScheduleRule,
  runScheduleNow,
  toggleSchedule,
  type ScheduledTask,
  type ScheduledTaskRun,
} from "./api";
import ScheduleDialog, { type ScheduleDraftPreset } from "./ScheduleDialog";
import "./schedules.css";

const QUICK_TEMPLATES: ScheduleDraftPreset[] = [
  {
    name: "每日代码变更巡检",
    prompt: "检查当前仓库过去 24 小时的 git 提交与未提交改动，总结核心变化、潜在风险以及建议补充的测试点。",
    schedule_kind: "daily",
    time_of_day: "09:30",
  },
  {
    name: "工作日项目进度日报",
    prompt: "汇总今天的工作区改动与待办事项，按「已完成 / 进行中 / 风险阻塞」整理一份简明日报。",
    schedule_kind: "weekly",
    weekdays: "1,2,3,4,5",
    time_of_day: "18:00",
  },
];

type FilterKind = "all" | "active" | "paused";

export default function SchedulesPanel({
  tasks,
  runs,
  loading,
  paused,
  agents,
  projects,
  groups,
  probes,
  onRefresh,
  onToggleDispatch,
  onOpenTarget,
  onNotify,
  onError,
}: {
  tasks: ScheduledTask[];
  runs: ScheduledTaskRun[];
  loading: boolean;
  paused: boolean;
  agents: Agent[];
  projects: Project[];
  groups: Group[];
  probes: Record<string, Probe>;
  onRefresh: () => Promise<void>;
  onToggleDispatch: () => Promise<void>;
  onOpenTarget: (task: ScheduledTask) => void;
  onNotify: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [filter, setFilter] = useState<FilterKind>("all");
  const [query, setQuery] = useState("");
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [dialogTask, setDialogTask] = useState<ScheduledTask | "new" | null>(null);
  const [dialogPreset, setDialogPreset] = useState<ScheduleDraftPreset | undefined>(undefined);
  const [deletingTask, setDeletingTask] = useState<ScheduledTask | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const projectMap = useMemo(() => new Map(projects.map(p => [p.id, p])), [projects]);
  const groupMap = useMemo(() => new Map(groups.map(g => [g.id, g])), [groups]);
  const taskMap = useMemo(() => new Map(tasks.map(t => [t.id, t])), [tasks]);

  const enabledTasks = useMemo(() => tasks.filter(t => t.enabled), [tasks]);
  const nextUpTask = useMemo(() => {
    return enabledTasks
      .filter(t => t.next_run_at != null)
      .sort((a, b) => (a.next_run_at ?? 0) - (b.next_run_at ?? 0))[0] ?? null;
  }, [enabledTasks]);
  const totalRuns = useMemo(() => tasks.reduce((sum, t) => sum + t.run_count, 0), [tasks]);

  const filteredTasks = useMemo(() => {
    const q = query.trim().toLowerCase();
    return tasks.filter(task => {
      if (filter === "active" && !task.enabled) return false;
      if (filter === "paused" && task.enabled) return false;
      if (!q) return true;
      return (
        task.name.toLowerCase().includes(q) ||
        task.prompt.toLowerCase().includes(q) ||
        (task.agent_id ?? "").toLowerCase().includes(q)
      );
    });
  }, [tasks, filter, query]);

  const selectedTask = selectedTaskId ? taskMap.get(selectedTaskId) ?? null : null;
  const visibleRuns = useMemo(() => {
    if (!selectedTask) return runs.slice(0, 30);
    return runs.filter(r => r.task_id === selectedTask.id).slice(0, 30);
  }, [runs, selectedTask]);

  function targetLabel(task: ScheduledTask): string {
    if (task.target_type === "group") {
      const g = task.group_id ? groupMap.get(task.group_id) : undefined;
      return `群聊 · ${g?.name ?? "已删除群聊"}`;
    }
    if (task.target_type === "project") {
      const p = task.project_id ? projectMap.get(task.project_id) : undefined;
      const a = task.agent_id ? agentName(task.agent_id) : "Agent";
      return `${p?.name ?? "项目"} · ${a}`;
    }
    return `${task.agent_id ? agentName(task.agent_id) : "Agent"} 私聊`;
  }

  async function handleToggle(task: ScheduledTask) {
    if (busyId) return;
    setBusyId(task.id);
    try {
      await toggleSchedule(task.id, !task.enabled);
      await onRefresh();
      onNotify(task.enabled ? `已暂停「${task.name}」` : `已启用「${task.name}」`);
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusyId(null);
    }
  }

  async function handleRunNow(task: ScheduledTask) {
    if (busyId) return;
    setBusyId(task.id);
    try {
      const run = await runScheduleNow(task.id);
      await onRefresh();
      if (run.status === "failed") {
        onError(`执行失败：${run.error ?? "未知错误"}`);
      } else if (run.status === "queued") {
        onNotify(`目标会话正忙，已将「${task.name}」加入排队`);
      } else {
        onNotify(`已触发「${task.name}」`);
      }
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusyId(null);
    }
  }

  async function handleDeleteConfirmed() {
    if (!deletingTask || busyId) return;
    setBusyId(deletingTask.id);
    try {
      await deleteSchedule(deletingTask.id);
      if (selectedTaskId === deletingTask.id) setSelectedTaskId(null);
      onNotify(`已删除「${deletingTask.name}」`);
      setDeletingTask(null);
      await onRefresh();
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <main className="schedules-panel">
      <div className="schedules-inner">
        <header className="schedules-header">
          <div>
            <h1>定时任务中心</h1>
            <p>统一管理所有已接入 Agent 与多 Agent 群聊的定时执行 · 托盘常驻自动触发</p>
          </div>
          <div className="schedules-header-actions">
            <button
              type="button"
              className="outline-button"
              onClick={() => void onRefresh()}
              disabled={loading}
              title="刷新任务与运行记录"
            >
              <Icon name="reset" size={15} />
              <span>刷新</span>
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => {
                setDialogPreset(undefined);
                setDialogTask("new");
              }}
            >
              <Icon name="plus" size={15} />
              <span>新建定时任务</span>
            </button>
          </div>
        </header>

        {paused && (
          <div className="schedules-paused-banner" role="status">
            <div className="schedules-paused-text">
              <Icon name="alert" size={17} />
              <div>
                <strong>全局派发当前已暂停</strong>
                <span>到点的定时任务将暂时跳过触发，恢复派发后自动继续按计划运行。</span>
              </div>
            </div>
            <button type="button" className="primary" onClick={() => void onToggleDispatch()}>
              恢复派发
            </button>
          </div>
        )}

        <section className="schedules-kpi-grid" aria-label="定时任务概览">
          <div className="schedules-kpi-card">
            <span className="schedules-kpi-label">运行中任务</span>
            <div className="schedules-kpi-value">
              <strong>{enabledTasks.length}</strong>
              <small>/ {tasks.length} 个总任务</small>
            </div>
            <span className="schedules-kpi-sub">
              {paused ? "全局派发已暂停" : enabledTasks.length > 0 ? "后台调度器正在监听" : "暂无启用的定时任务"}
            </span>
          </div>

          <div className="schedules-kpi-card">
            <span className="schedules-kpi-label">下一次触发</span>
            <div className="schedules-kpi-value">
              <strong>{nextUpTask ? formatEpochTime(nextUpTask.next_run_at) : "—"}</strong>
              {nextUpTask?.next_run_at && (
                <small>{formatRelativeTime(nextUpTask.next_run_at)}</small>
              )}
            </div>
            <span className="schedules-kpi-sub">
              {nextUpTask ? nextUpTask.name : "启用任务后将自动计算下次触发时间"}
            </span>
          </div>

          <div className="schedules-kpi-card">
            <span className="schedules-kpi-label">累计触发次数</span>
            <div className="schedules-kpi-value">
              <strong>{totalRuns}</strong>
              <small>次历史执行</small>
            </div>
            <span className="schedules-kpi-sub">
              最近记录 {runs.length} 条 · 点击任意任务可筛选历史
            </span>
          </div>
        </section>

        <div className="schedules-workspace">
          <section className="schedules-list-column">
            <div className="schedules-toolbar">
              <div className="schedules-filters" role="tablist">
                {(
                  [
                    ["all", `全部 (${tasks.length})`],
                    ["active", `运行中 (${enabledTasks.length})`],
                    ["paused", `已暂停 (${tasks.length - enabledTasks.length})`],
                  ] as [FilterKind, string][]
                ).map(([k, label]) => (
                  <button
                    key={k}
                    type="button"
                    role="tab"
                    aria-selected={filter === k}
                    className={filter === k ? "active" : ""}
                    onClick={() => setFilter(k)}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <label className="schedules-search">
                <Icon name="search" size={15} />
                <input
                  value={query}
                  onChange={e => setQuery(e.target.value)}
                  placeholder="搜索任务名或指令…"
                />
              </label>
            </div>

            {tasks.length === 0 && !loading ? (
              <div className="schedules-empty-state">
                <div className="schedules-empty-icon">
                  <Icon name="clock" size={32} />
                </div>
                <h2>还没有定时任务</h2>
                <p>
                  让本机的 Codex、Claude、Gemini 或 DeepSeek 按计划自动帮你巡检代码、整理日报，也支持定时触发多 Agent 群聊协作。
                </p>
                <div className="schedules-empty-actions">
                  <button
                    type="button"
                    className="primary"
                    onClick={() => {
                      setDialogPreset(undefined);
                      setDialogTask("new");
                    }}
                  >
                    <Icon name="plus" size={15} />
                    <span>新建定时任务</span>
                  </button>
                </div>
                <div className="schedules-templates">
                  <span>快速模板</span>
                  <div className="schedules-template-grid">
                    {QUICK_TEMPLATES.map(tpl => (
                      <button
                        key={tpl.name}
                        type="button"
                        className="schedule-template-card"
                        onClick={() => {
                          setDialogPreset(tpl);
                          setDialogTask("new");
                        }}
                      >
                        <strong>{tpl.name}</strong>
                        <small>{tpl.schedule_kind === "daily" ? `每天 ${tpl.time_of_day}` : `工作日 ${tpl.time_of_day}`}</small>
                        <p>{tpl.prompt}</p>
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            ) : filteredTasks.length === 0 ? (
              <div className="schedules-empty-filter">
                没有匹配当前筛选条件的定时任务。
              </div>
            ) : (
              <div className="schedules-card-list">
                {filteredTasks.map(task => {
                  const isSelected = selectedTask?.id === task.id;
                  const isBusy = busyId === task.id;
                  return (
                    <article
                      key={task.id}
                      className={`schedule-card ${isSelected ? "selected" : ""} ${task.enabled ? "" : "is-paused"}`}
                      onClick={() => setSelectedTaskId(isSelected ? null : task.id)}
                    >
                      <div className="schedule-card-top">
                        <div className="schedule-card-title">
                          <Avatar
                            agent={task.target_type === "group" ? "group" : (task.agent_id ?? "codex")}
                            small
                          />
                          <div className="schedule-card-heading">
                            <div className="schedule-card-name-row">
                              <strong>{task.name}</strong>
                            </div>
                            <div className="schedule-card-chips">
                              <span className="schedule-chip accent">
                                <Icon name="clock" size={12} />
                                {formatScheduleRule(task)}
                              </span>
                              <span className="schedule-chip subtle">{targetLabel(task)}</span>
                              {task.model && (
                                <span className="schedule-chip subtle">{task.model}</span>
                              )}
                            </div>
                          </div>
                        </div>

                        <button
                          type="button"
                          role="switch"
                          aria-checked={task.enabled}
                          aria-label={task.enabled ? "暂停定时任务" : "启用定时任务"}
                          className={`schedule-switch ${task.enabled ? "on" : ""}`}
                          disabled={isBusy}
                          onClick={e => {
                            e.stopPropagation();
                            void handleToggle(task);
                          }}
                        >
                          <span />
                        </button>
                      </div>

                      <p className="schedule-card-prompt">{task.prompt}</p>

                      {task.last_error && task.last_status === "error" && (
                        <div className="schedule-card-error">
                          <Icon name="alert" size={13} />
                          <span>上次失败：{task.last_error}</span>
                        </div>
                      )}

                      <div className="schedule-card-footer">
                        <div className="schedule-card-timing">
                          <span>
                            {task.enabled
                              ? `下次：${formatEpochTime(task.next_run_at)}`
                              : "已暂停"}
                          </span>
                          {task.last_run_at && (
                            <span className={`schedule-status-dot ${task.last_status ?? ""}`}>
                              上次：{formatEpochTime(task.last_run_at)}
                            </span>
                          )}
                        </div>

                        <div
                          className="schedule-card-actions"
                          onClick={e => e.stopPropagation()}
                        >
                          <button
                            type="button"
                            className="schedule-action-btn"
                            title="立即运行一次"
                            disabled={isBusy}
                            onClick={() => void handleRunNow(task)}
                          >
                            <Icon name="play" size={14} />
                            <span>立即运行</span>
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            title="前往对应对话"
                            aria-label="前往对应对话"
                            onClick={() => onOpenTarget(task)}
                          >
                            <Icon name="external" size={15} />
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            title="编辑任务"
                            aria-label="编辑任务"
                            onClick={() => {
                              setDialogPreset(undefined);
                              setDialogTask(task);
                            }}
                          >
                            <Icon name="edit" size={15} />
                          </button>
                          <button
                            type="button"
                            className="icon-button danger"
                            title="删除任务"
                            aria-label="删除任务"
                            onClick={() => setDeletingTask(task)}
                          >
                            <Icon name="trash" size={15} />
                          </button>
                        </div>
                      </div>
                    </article>
                  );
                })}
              </div>
            )}
          </section>

          <aside className="schedules-history-column" aria-label="执行历史">
            <div className="schedules-history-head">
              <div>
                <h3>{selectedTask ? `「${selectedTask.name}」执行记录` : "最近执行记录"}</h3>
                <small>{selectedTask ? "点击任务卡片可切换或取消筛选" : "展示全部定时任务的触发历史"}</small>
              </div>
              {selectedTask && (
                <button
                  type="button"
                  className="schedules-clear-filter"
                  onClick={() => setSelectedTaskId(null)}
                >
                  查看全部
                </button>
              )}
            </div>

            {visibleRuns.length === 0 ? (
              <div className="schedules-history-empty">
                <Icon name="clock" size={22} />
                <p>暂无执行记录。任务到点自动触发或点击「立即运行」后，会在这里记录每次执行结果。</p>
              </div>
            ) : (
              <div className="schedules-history-list">
                {visibleRuns.map(run => {
                  const owner = taskMap.get(run.task_id);
                  return (
                    <div key={run.id} className="schedule-run-item">
                      <div className="schedule-run-top">
                        <span className={`schedule-run-badge ${run.status}`}>
                          {run.status === "dispatched"
                            ? "已派发"
                            : run.status === "queued"
                              ? "已排队"
                              : run.status === "failed"
                                ? "失败"
                                : "跳过"}
                        </span>
                        <span className="schedule-run-trigger">
                          {run.trigger_kind === "manual" ? "手动" : "定时"}
                        </span>
                        <time>{formatEpochTime(run.started_at)}</time>
                      </div>
                      {!selectedTask && owner && (
                        <strong className="schedule-run-name">{owner.name}</strong>
                      )}
                      <div className="schedule-run-meta">
                        <span>{run.target_summary || (owner ? targetLabel(owner) : "")}</span>
                        {owner && (
                          <button
                            type="button"
                            className="schedule-jump-btn"
                            onClick={() => onOpenTarget(owner)}
                          >
                            查看对话 →
                          </button>
                        )}
                      </div>
                      {run.error && <p className="schedule-run-error">{run.error}</p>}
                    </div>
                  );
                })}
              </div>
            )}
          </aside>
        </div>
      </div>

      {dialogTask && (
        <ScheduleDialog
          existing={dialogTask === "new" ? undefined : dialogTask}
          preset={dialogTask === "new" ? dialogPreset : undefined}
          agents={agents}
          projects={projects}
          groups={groups}
          probes={probes}
          onClose={() => setDialogTask(null)}
          onSaved={async (saved, isNew) => {
            setDialogTask(null);
            await onRefresh();
            onNotify(isNew ? `已创建定时任务「${saved.name}」` : `已更新「${saved.name}」`);
          }}
        />
      )}

      {deletingTask && (
        <Dialog title="删除定时任务" onClose={() => setDeletingTask(null)}>
          <p className="field-help" style={{ marginTop: 0, lineHeight: 1.7 }}>
            确定删除定时任务「<strong>{deletingTask.name}</strong>」及其运行历史吗？已产生的聊天记录不会被删除。
          </p>
          <div className="dialog-footer">
            <span />
            <div className="session-dialog-actions">
              <button type="button" className="outline-button" onClick={() => setDeletingTask(null)}>
                取消
              </button>
              <button
                type="button"
                className="danger-button"
                disabled={busyId === deletingTask.id}
                onClick={() => void handleDeleteConfirmed()}
              >
                确认删除
              </button>
            </div>
          </div>
        </Dialog>
      )}
    </main>
  );
}
