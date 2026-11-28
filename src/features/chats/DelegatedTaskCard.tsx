import { useEffect, useState } from "react";
import Avatar from "../../components/Avatar";
import { agentName, delegatedTaskDetail, type DelegatedTask, type DelegatedTaskDetail } from "./api";

const statusMap: Record<string, { label: string; tone: string }> = {
  queued: { label: "排队中", tone: "neutral" },
  launching: { label: "启动中", tone: "running" },
  running: { label: "执行中", tone: "running" },
  completed: { label: "已完成", tone: "success" },
  failed: { label: "失败", tone: "danger" },
  cancelled: { label: "已取消", tone: "muted" },
  interrupted_unknown: { label: "已中断", tone: "danger" },
};

function formatDuration(startIso: string, endIso: string | null): string {
  const start = Date.parse(startIso);
  if (Number.isNaN(start)) return "";
  const end = endIso ? Date.parse(endIso) : Date.now();
  if (Number.isNaN(end) || end < start) return "";
  const secs = Math.round((end - start) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return `${mins}m ${rem}s`;
}

export default function DelegatedTaskCard({ task }: { task: DelegatedTask }) {
  const [expanded, setExpanded] = useState(false);
  const [detail, setDetail] = useState<DelegatedTaskDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    if (!expanded) return;
    let active = true;
    setLoading(true);
    setLoadError(null);
    delegatedTaskDetail(task.id)
      .then(res => {
        if (active) setDetail(res);
      })
      .catch(err => {
        if (active) setLoadError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [expanded, task.id, task.status, task.finished_at]);

  const badge = statusMap[task.status] ?? { label: task.status, tone: "neutral" };
  const duration = formatDuration(task.created_at, task.finished_at);
  const snippet = task.instruction.replace(/\s+/g, " ").trim();

  return (
    <div className={`delegated-task-card tone-${badge.tone}${expanded ? " expanded" : ""}`}>
      <button
        type="button"
        className="delegated-task-header"
        onClick={() => setExpanded(v => !v)}
        aria-expanded={expanded}
      >
        <span className="delegated-task-lead">
          <Avatar agent={task.worker_agent} small />
          <strong className="delegated-task-worker">{agentName(task.worker_agent)}</strong>
          <span className={`delegated-task-badge tone-${badge.tone}`}>{badge.label}</span>
          <span className="delegated-task-snippet" title={task.instruction}>{snippet}</span>
        </span>
        <span className="delegated-task-meta">
          {task.changed_files_count > 0 && (
            <span className="delegated-task-files">{task.changed_files_count} 个文件变更</span>
          )}
          {duration && <span className="delegated-task-duration">{duration}</span>}
          <span className="delegated-task-toggle">{expanded ? "收起" : "展开"}</span>
        </span>
      </button>

      {expanded && (
        <div className="delegated-task-body">
          <div className="delegated-task-section">
            <div className="delegated-task-section-title">委派指令</div>
            <pre className="delegated-task-instruction">{task.instruction}</pre>
          </div>

          {task.error && (
            <div className="delegated-task-error">{task.error}</div>
          )}

          {loading && !detail && (
            <div className="delegated-task-loading">正在加载子任务详情…</div>
          )}

          {loadError && (
            <div className="delegated-task-error">加载详情失败：{loadError}</div>
          )}

          {detail && (
            <>
              {detail.messages.length > 0 && (
                <div className="delegated-task-section">
                  <div className="delegated-task-section-title">
                    执行记录与回复（{detail.messages.length}）
                  </div>
                  <div className="delegated-task-messages">
                    {detail.messages.map(m => (
                      <div key={m.id} className={`delegated-task-msg role-${m.role}`}>
                        <span className="delegated-task-msg-role">
                          {m.role === "assistant" ? agentName(task.worker_agent) : m.role === "tool" ? "工具" : "思考"}
                        </span>
                        <pre className="delegated-task-msg-text">{m.text}</pre>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {Array.isArray(detail.turn_diffs) && detail.turn_diffs.length > 0 && (
                <div className="delegated-task-section">
                  <div className="delegated-task-section-title">独立 Turn Diff</div>
                  <div className="delegated-task-diffs">
                    {detail.turn_diffs.map((turn: { turn_id?: string; files?: { path: string; status?: string }[] }, idx: number) => (
                      <div key={turn.turn_id ?? idx} className="delegated-task-diff-turn">
                        {(turn.files ?? []).map(f => (
                          <div key={f.path} className="delegated-task-diff-file">
                            <span>{f.status ?? "M"}</span>
                            <code>{f.path}</code>
                          </div>
                        ))}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
