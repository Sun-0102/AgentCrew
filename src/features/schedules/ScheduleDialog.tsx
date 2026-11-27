import { useEffect, useMemo, useState, type FormEvent } from "react";
import Avatar from "../../components/Avatar";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { type Agent, type Group, type Probe } from "../chats/api";
import { type Project } from "../projects/api";
import {
  saveSchedule,
  type ScheduleKind,
  type ScheduledTask,
  type TargetType,
} from "./api";
import SearchableSelect, { type SelectOption } from "./SearchableSelect";

const WEEKDAY_OPTIONS = [
  { value: "1", label: "一" },
  { value: "2", label: "二" },
  { value: "3", label: "三" },
  { value: "4", label: "四" },
  { value: "5", label: "五" },
  { value: "6", label: "六" },
  { value: "7", label: "日" },
];

const INTERVAL_PRESETS = [
  { label: "15 分钟", value: 15 },
  { label: "30 分钟", value: 30 },
  { label: "1 小时", value: 60 },
  { label: "2 小时", value: 120 },
  { label: "6 小时", value: 360 },
  { label: "12 小时", value: 720 },
  { label: "每天", value: 1440 },
];

const TIME_PRESETS = ["09:00", "12:00", "18:00", "22:00"];
const HOUR_OPTIONS: SelectOption[] = Array.from({ length: 24 }, (_, i) => {
  const v = String(i).padStart(2, "0");
  return { value: v, label: `${v} 时` };
});
const MINUTE_OPTIONS: SelectOption[] = Array.from({ length: 60 }, (_, i) => {
  const v = String(i).padStart(2, "0");
  return { value: v, label: `${v} 分` };
});

export interface ScheduleDraftPreset {
  name: string;
  prompt: string;
  schedule_kind: ScheduleKind;
  time_of_day?: string;
  weekdays?: string;
  interval_minutes?: number;
}

function TimePicker({
  value,
  onChange,
}: {
  value: string;
  onChange: (next: string) => void;
}) {
  const [hh, mm] = useMemo(() => {
    const [h = "09", m = "00"] = value.split(":");
    return [h.padStart(2, "0"), m.padStart(2, "0")];
  }, [value]);

  return (
    <div className="sched-time-picker">
      <div className="sched-time-selects">
        <SearchableSelect
          value={hh}
          options={HOUR_OPTIONS}
          searchable={false}
          compact
          onChange={h => onChange(`${h}:${mm}`)}
        />
        <span className="sched-time-colon">:</span>
        <SearchableSelect
          value={mm}
          options={MINUTE_OPTIONS}
          searchable={false}
          compact
          onChange={m => onChange(`${hh}:${m}`)}
        />
      </div>
      <div className="sched-time-presets">
        {TIME_PRESETS.map(preset => (
          <button
            key={preset}
            type="button"
            className={`sched-pill ${value === preset ? "active" : ""}`}
            onClick={() => onChange(preset)}
          >
            {preset}
          </button>
        ))}
      </div>
    </div>
  );
}

export default function ScheduleDialog({
  existing,
  preset,
  agents,
  projects,
  groups,
  probes,
  onClose,
  onSaved,
}: {
  existing?: ScheduledTask;
  preset?: ScheduleDraftPreset;
  agents: Agent[];
  projects: Project[];
  groups: Group[];
  probes: Record<string, Probe>;
  onClose: () => void;
  onSaved: (saved: ScheduledTask, isNew: boolean) => void;
}) {
  const connectedAgents = useMemo(
    () => agents.filter(a => a.connected || a.enabled),
    [agents],
  );
  const defaultAgentId = existing?.agent_id ?? connectedAgents[0]?.id ?? "codex";

  const [name, setName] = useState(existing?.name ?? preset?.name ?? "");
  const [prompt, setPrompt] = useState(existing?.prompt ?? preset?.prompt ?? "");
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [targetType, setTargetType] = useState<TargetType>(existing?.target_type ?? "agent");
  const [agentId, setAgentId] = useState(defaultAgentId);
  const [projectId, setProjectId] = useState(existing?.project_id ?? projects[0]?.id ?? "");
  const [groupId, setGroupId] = useState(existing?.group_id ?? groups[0]?.id ?? "");
  const [groupTargetAgent, setGroupTargetAgent] = useState(
    existing?.target_type === "group" ? (existing.agent_id ?? "all") : "all",
  );
  const [model, setModel] = useState(existing?.model ?? "");
  const [scheduleKind, setScheduleKind] = useState<ScheduleKind>(
    existing?.schedule_kind ?? preset?.schedule_kind ?? "daily",
  );
  const [intervalMinutes, setIntervalMinutes] = useState<number>(
    existing?.interval_minutes ?? preset?.interval_minutes ?? 60,
  );
  const [timeOfDay, setTimeOfDay] = useState(
    existing?.time_of_day ?? preset?.time_of_day ?? "09:00",
  );
  const [weekdays, setWeekdays] = useState<string[]>(() => {
    const raw = existing?.weekdays ?? preset?.weekdays ?? "1,2,3,4,5";
    return raw.split(",").map(s => s.trim()).filter(Boolean);
  });
  const [cronExpr, setCronExpr] = useState(existing?.cron_expr ?? "0 9 * * 1-5");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const activeProbe = probes[agentId];
  const availableModels = activeProbe?.models ?? [];

  useEffect(() => {
    if (targetType === "group") return;
    const probe = probes[agentId];
    if (!probe) {
      if (existing && agentId !== existing.agent_id) {
        setModel("");
      }
      return;
    }
    const fallback = probe.models.find(m => m.isDefault)?.model || probe.models[0]?.model || "";
    const isValidForAgent = model && probe.models.some(m => m.model === model);
    if (!isValidForAgent) {
      setModel(fallback);
    }
  }, [agentId, targetType, probes, existing]);

  const selectedGroup = groups.find(g => g.id === groupId);

  const agentOptions: SelectOption[] = useMemo(
    () =>
      connectedAgents.map(a => ({
        value: a.id,
        label: a.name,
        subtitle: a.provider_kind,
        leading: <Avatar agent={a.id} small />,
      })),
    [connectedAgents],
  );

  const projectOptions: SelectOption[] = useMemo(
    () =>
      projects.map(p => ({
        value: p.id,
        label: p.name,
        subtitle: p.primary_root,
        leading: <Icon name="folder" size={15} />,
      })),
    [projects],
  );

  const groupOptions: SelectOption[] = useMemo(
    () =>
      groups.map(g => ({
        value: g.id,
        label: g.name,
        subtitle: `${g.members.length} 位 Agent 成员`,
        leading: <Avatar agent="group" small />,
      })),
    [groups],
  );

  const groupMemberOptions: SelectOption[] = useMemo(() => {
    const base: SelectOption[] = [
      {
        value: "all",
        label: "全体成员协同分工",
        subtitle: "自动广播协商并并行协作",
        leading: <Icon name="group" size={15} />,
      },
    ];
    for (const memberId of selectedGroup?.members ?? []) {
      const m = agents.find(a => a.id === memberId);
      base.push({
        value: memberId,
        label: `仅 @${m?.name ?? memberId}`,
        subtitle: "定向投递给该成员",
        leading: <Avatar agent={memberId} small />,
      });
    }
    return base;
  }, [selectedGroup, agents]);

  const modelOptions: SelectOption[] = useMemo(() => {
    const base: SelectOption[] = [
      {
        value: "",
        label: "自动使用默认模型",
        subtitle: "跟随该 Agent 当前默认配置",
      },
    ];
    for (const m of availableModels) {
      base.push({
        value: m.model,
        label: m.displayName || m.model,
        subtitle: m.displayName && m.displayName !== m.model ? m.model : undefined,
        badge: m.isDefault ? "默认" : undefined,
      });
    }
    return base;
  }, [availableModels]);

  function toggleWeekday(day: string) {
    setWeekdays(prev => {
      if (prev.includes(day)) {
        return prev.length > 1 ? prev.filter(d => d !== day) : prev;
      }
      return [...prev, day].sort();
    });
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (saving) return;
    setError(null);
    setSaving(true);
    try {
      const saved = await saveSchedule({
        id: existing?.id,
        name: name.trim(),
        prompt: prompt.trim(),
        enabled,
        schedule_kind: scheduleKind,
        interval_minutes: scheduleKind === "interval" ? intervalMinutes : null,
        time_of_day: scheduleKind === "daily" || scheduleKind === "weekly" ? timeOfDay : null,
        weekdays: scheduleKind === "weekly" ? weekdays.join(",") : null,
        cron_expr: scheduleKind === "cron" ? cronExpr.trim() : null,
        target_type: targetType,
        agent_id: targetType === "group" ? groupTargetAgent : agentId,
        project_id: targetType === "project" ? projectId : null,
        group_id: targetType === "group" ? groupId : null,
        model: targetType === "group" ? "" : model.trim(),
        reasoning_effort: existing?.reasoning_effort ?? null,
        service_tier: existing?.service_tier ?? null,
      });
      onSaved(saved, !existing);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog
      title={existing ? "编辑定时任务" : "新建定时任务"}
      onClose={() => { if (!saving) onClose(); }}
    >
      <form className="schedule-form" onSubmit={submit}>
        <div className="sched-field">
          <label className="sched-label" htmlFor="sched-name">任务名称</label>
          <input
            id="sched-name"
            className="sched-input"
            value={name}
            onChange={e => setName(e.target.value)}
            maxLength={60}
            placeholder="例如：每日代码变更巡检"
            autoFocus
            required
          />
        </div>

        <div className="sched-card-group">
          <div className="sched-group-head">
            <span className="sched-label">执行目标</span>
            <div className="sched-segmented" role="tablist">
              <button
                type="button"
                role="tab"
                aria-selected={targetType === "agent"}
                className={targetType === "agent" ? "active" : ""}
                onClick={() => setTargetType("agent")}
              >
                单 Agent
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={targetType === "project"}
                className={targetType === "project" ? "active" : ""}
                onClick={() => setTargetType("project")}
                disabled={projects.length === 0}
                title={projects.length === 0 ? "请先在项目空间添加本地项目" : undefined}
              >
                项目空间
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={targetType === "group"}
                className={targetType === "group" ? "active" : ""}
                onClick={() => setTargetType("group")}
                disabled={groups.length === 0}
                title={groups.length === 0 ? "请先创建一个协作群聊" : undefined}
              >
                协作群聊
              </button>
            </div>
          </div>

          {targetType === "agent" && (
            <div className="sched-grid-2">
              <div className="sched-sub-field">
                <span>执行 Agent</span>
                <SearchableSelect
                  value={agentId}
                  options={agentOptions}
                  searchPlaceholder="搜索 Agent…"
                  onChange={setAgentId}
                />
              </div>
              <div className="sched-sub-field">
                <span>执行模型</span>
                {availableModels.length > 0 ? (
                  <SearchableSelect
                    value={model}
                    options={modelOptions}
                    searchPlaceholder="搜索模型…"
                    onChange={setModel}
                  />
                ) : (
                  <input
                    className="sched-input"
                    value={model}
                    onChange={e => setModel(e.target.value)}
                    placeholder="留空自动使用默认模型"
                  />
                )}
              </div>
            </div>
          )}

          {targetType === "project" && (
            <>
              <div className="sched-grid-2">
                <div className="sched-sub-field">
                  <span>绑定项目</span>
                  <SearchableSelect
                    value={projectId}
                    options={projectOptions}
                    searchPlaceholder="搜索项目名或路径…"
                    onChange={setProjectId}
                  />
                </div>
                <div className="sched-sub-field">
                  <span>执行 Agent</span>
                  <SearchableSelect
                    value={agentId}
                    options={agentOptions}
                    searchPlaceholder="搜索 Agent…"
                    onChange={setAgentId}
                  />
                </div>
              </div>
              <div className="sched-sub-field">
                <span>执行模型</span>
                {availableModels.length > 0 ? (
                  <SearchableSelect
                    value={model}
                    options={modelOptions}
                    searchPlaceholder="搜索模型…"
                    onChange={setModel}
                  />
                ) : (
                  <input
                    className="sched-input"
                    value={model}
                    onChange={e => setModel(e.target.value)}
                    placeholder="留空自动使用默认模型"
                  />
                )}
              </div>
            </>
          )}

          {targetType === "group" && (
            <div className="sched-grid-2">
              <div className="sched-sub-field">
                <span>目标协作群</span>
                <SearchableSelect
                  value={groupId}
                  options={groupOptions}
                  searchPlaceholder="搜索群聊名称…"
                  onChange={setGroupId}
                />
              </div>
              <div className="sched-sub-field">
                <span>触发成员</span>
                <SearchableSelect
                  value={groupTargetAgent}
                  options={groupMemberOptions}
                  searchPlaceholder="搜索群成员…"
                  onChange={setGroupTargetAgent}
                />
              </div>
            </div>
          )}
        </div>

        <div className="sched-card-group">
          <div className="sched-group-head">
            <span className="sched-label">触发周期</span>
            <div className="sched-segmented" role="tablist">
              {(
                [
                  ["daily", "每天"],
                  ["weekly", "每周"],
                  ["interval", "间隔"],
                  ["cron", "Cron"],
                ] as [ScheduleKind, string][]
              ).map(([kind, label]) => (
                <button
                  key={kind}
                  type="button"
                  role="tab"
                  aria-selected={scheduleKind === kind}
                  className={scheduleKind === kind ? "active" : ""}
                  onClick={() => setScheduleKind(kind)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          {scheduleKind === "daily" && (
            <div className="sched-sub-field">
              <span>每天执行时间（本地时间）</span>
              <TimePicker value={timeOfDay} onChange={setTimeOfDay} />
            </div>
          )}

          {scheduleKind === "weekly" && (
            <div className="sched-stack">
              <div className="sched-sub-field">
                <span>重复日期</span>
                <div className="weekday-pills">
                  {WEEKDAY_OPTIONS.map(opt => (
                    <button
                      key={opt.value}
                      type="button"
                      className={`sched-pill ${weekdays.includes(opt.value) ? "active" : ""}`}
                      onClick={() => toggleWeekday(opt.value)}
                    >
                      周{opt.label}
                    </button>
                  ))}
                </div>
              </div>
              <div className="sched-sub-field">
                <span>执行时间（本地时间）</span>
                <TimePicker value={timeOfDay} onChange={setTimeOfDay} />
              </div>
            </div>
          )}

          {scheduleKind === "interval" && (
            <div className="sched-stack">
              <div className="interval-presets">
                {INTERVAL_PRESETS.map(p => (
                  <button
                    key={p.value}
                    type="button"
                    className={`sched-pill ${intervalMinutes === p.value ? "active" : ""}`}
                    onClick={() => setIntervalMinutes(p.value)}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
              <label className="sched-sub-field inline">
                <span>自定义间隔（分钟）</span>
                <input
                  type="number"
                  min={1}
                  max={43200}
                  className="sched-input compact"
                  value={intervalMinutes}
                  onChange={e => setIntervalMinutes(Math.max(1, Number(e.target.value) || 1))}
                />
              </label>
            </div>
          )}

          {scheduleKind === "cron" && (
            <div className="sched-sub-field">
              <span>5 段 Cron 表达式（分 时 日 月 周）</span>
              <input
                className="sched-input schedule-cron-input"
                value={cronExpr}
                onChange={e => setCronExpr(e.target.value)}
                placeholder="0 9 * * 1-5"
                required
              />
              <small className="sched-hint">
                周一=1，周日=7 或 0。例如 <code>0 9 * * 1-5</code> 表示工作日 09:00。
              </small>
            </div>
          )}
        </div>

        <div className="sched-field">
          <label className="sched-label" htmlFor="sched-prompt">执行指令</label>
          <textarea
            id="sched-prompt"
            className="sched-textarea"
            rows={4}
            value={prompt}
            onChange={e => setPrompt(e.target.value)}
            placeholder="写明到点后自动发送给 Agent 的内容，例如：检查过去 24 小时的 git 提交，总结潜在风险和待办事项。"
            required
          />
        </div>

        <div className="sched-switch-row" onClick={() => setEnabled(v => !v)}>
          <div>
            <strong>保存后立即启用</strong>
            <small>开启后将在下次计划时间自动触发（需保持托盘后台运行）</small>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={enabled}
            className={`schedule-switch ${enabled ? "on" : ""}`}
            onClick={e => {
              e.stopPropagation();
              setEnabled(v => !v);
            }}
          >
            <span />
          </button>
        </div>

        {error && (
          <div className="schedule-form-error" role="alert">
            <Icon name="alert" size={15} />
            <span>{error}</span>
          </div>
        )}

        <div className="sched-dialog-footer">
          <button type="button" className="outline-button" disabled={saving} onClick={onClose}>
            取消
          </button>
          <button type="submit" className="primary" disabled={saving}>
            {saving ? "保存中…" : existing ? "保存修改" : "创建任务"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
