import { useEffect, useState } from "react";
import Icon, { type IconName } from "../../components/Icon";
import MessageBody from "./MessageBody";
import { elapsed, type ReplyBlock } from "./activity";
import {
  collapseWorkLogItems,
  parseWorkLogItem,
  type FileDiffEntry,
  type WorkLogItem,
  type WorkLogStatus,
} from "./worklog";

const MAX_COMPACT_ITEMS = 6;

const outcomes: Record<string, string> = {
  cancelled: "已停止",
  failed: "执行失败",
  interrupted: "执行中断",
};

function itemIcon(kind: WorkLogItem["kind"]): IconName {
  switch (kind) {
    case "thinking":
      return "thought";
    case "command":
      return "terminal";
    case "file_read":
      return "file";
    case "file_change":
      return "diff";
    case "search":
      return "search";
    case "handoff":
      return "at";
    case "plan":
      return "check";
    default:
      return "bolt";
  }
}

function statusPillLabel(status: WorkLogStatus): string {
  if (status === "running") return "运行中";
  if (status === "failed") return "失败";
  return "";
}

function DiffBlock({ entry }: { entry: FileDiffEntry }) {
  const lines = entry.diff ? entry.diff.split("\n") : [];
  return (
    <div className="worklog-diff">
      <div className="worklog-diff-header">
        <span className="worklog-diff-path" title={entry.path}>{entry.path}</span>
        <span className="worklog-diff-stats">
          {entry.kind && <span className="worklog-diff-kind">{entry.kind}</span>}
          {entry.additions > 0 && <span className="worklog-diff-add">+{entry.additions}</span>}
          {entry.deletions > 0 && <span className="worklog-diff-del">-{entry.deletions}</span>}
        </span>
      </div>
      {lines.length > 0 && (
        <pre className="worklog-diff-code">
          {lines.map((line, index) => {
            const cls = line.startsWith("@@")
              ? "is-hunk"
              : line.startsWith("+") && !line.startsWith("+++")
                ? "is-add"
                : line.startsWith("-") && !line.startsWith("---")
                  ? "is-del"
                  : "";
            return (
              <span key={index} className={`worklog-diff-line ${cls}`}>
                {line}
                {"\n"}
              </span>
            );
          })}
        </pre>
      )}
    </div>
  );
}

function HandoffContext({ item }: { item: WorkLogItem }) {
  const [open, setOpen] = useState(false);
  const hasBody = Boolean(item.body);
  return (
    <div className={`worklog-context ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="worklog-context-trigger"
        disabled={!hasBody}
        aria-expanded={hasBody ? open : undefined}
        onClick={() => hasBody && setOpen(v => !v)}
      >
        <Icon name="at" size={13} />
        <span className="worklog-context-label">{item.title}</span>
        {item.subtitle && !open && <span className="worklog-context-preview">{item.subtitle}</span>}
        {hasBody && (
          <span className="worklog-chevron">
            <Icon name={open ? "down" : "right"} size={12} />
          </span>
        )}
      </button>
      {open && item.body && <pre className="worklog-context-body">{item.body}</pre>}
    </div>
  );
}

function WorkLogRow({ item, defaultOpen }: { item: WorkLogItem; defaultOpen: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (defaultOpen) setOpen(true);
  }, [defaultOpen]);

  if (item.kind === "note" && item.body) {
    return (
      <div className="worklog-note">
        <MessageBody text={item.body} />
      </div>
    );
  }

  const hasDetails = Boolean(item.body || item.diffs?.length || item.plan?.length);
  const pill = statusPillLabel(item.status);
  const isMonoTitle =
    item.kind === "command" || item.kind === "file_read" || item.kind === "file_change";

  const copyOutput = () => {
    const payload = item.kind === "command" ? (item.body ? `${item.title}\n\n${item.body}` : item.title) : item.body ?? item.title;
    void navigator.clipboard?.writeText(payload).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };

  return (
    <div className={`worklog-row is-${item.kind} status-${item.status} ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="worklog-row-trigger"
        disabled={!hasDetails}
        aria-expanded={hasDetails ? open : undefined}
        onClick={() => hasDetails && setOpen(value => !value)}
      >
        <span className={`worklog-row-icon status-${item.status}`}>
          {item.status === "running" ? (
            <span className="worklog-spinner" aria-hidden="true" />
          ) : item.status === "failed" ? (
            <Icon name="alert" size={13} />
          ) : (
            <Icon name={itemIcon(item.kind)} size={13} />
          )}
        </span>
        <span className="worklog-row-main">
          {item.action && <span className="worklog-row-action">{item.action}</span>}
          <span
            className={`worklog-row-title ${isMonoTitle ? "is-mono" : ""}`}
            title={item.fullPath ?? item.title}
          >
            {item.title}
          </span>
          {item.count && item.count > 1 && <span className="worklog-count">×{item.count}</span>}
          {item.subtitle && !open && (
            <span className="worklog-row-subtitle" title={item.subtitle}>
              {item.subtitle}
            </span>
          )}
        </span>
        <span className="worklog-row-meta">
          {(item.additions !== undefined || item.deletions !== undefined) && (
            <span className="worklog-diff-inline">
              {item.additions ? <span className="worklog-diff-add">+{item.additions}</span> : null}
              {item.deletions ? <span className="worklog-diff-del">-{item.deletions}</span> : null}
            </span>
          )}
          {item.badge && <span className="worklog-chip">{item.badge}</span>}
          {item.exitCode !== undefined && item.exitCode !== 0 && (
            <span className="worklog-chip is-fail">exit {item.exitCode}</span>
          )}
          {item.duration && <span className="worklog-meta-text">{item.duration}</span>}
          {pill && item.kind !== "thinking" && (
            <span className={`worklog-status-pill status-${item.status}`}>{pill}</span>
          )}
          {hasDetails && (
            <span className="worklog-chevron">
              <Icon name={open ? "down" : "right"} size={12} />
            </span>
          )}
        </span>
      </button>

      {open && hasDetails && (
        <div className="worklog-row-body">
          {item.kind === "thinking" && item.body && (
            <div className="worklog-thought-body">
              <MessageBody text={item.body} />
            </div>
          )}
          {item.plan && item.plan.length > 0 && (
            <ul className="worklog-plan">
              {item.plan.map((entry, idx) => (
                <li key={idx} className={`is-${entry.status}`}>
                  <span className="worklog-plan-dot" aria-hidden="true" />
                  <span>{entry.content}</span>
                </li>
              ))}
            </ul>
          )}
          {item.diffs && item.diffs.length > 0 && (
            <div className="worklog-diffs">
              {item.diffs.map((entry, idx) => (
                <DiffBlock key={`${entry.path}-${idx}`} entry={entry} />
              ))}
            </div>
          )}
          {item.kind !== "thinking" && item.body && (
            <div className={`worklog-code-wrap ${item.kind === "command" ? "is-terminal" : ""}`}>
              {item.kind === "command" && (
                <div className="worklog-code-bar">
                  <span className="worklog-code-cmd" title={item.title}>$ {item.title}</span>
                  <span className="worklog-code-bar-meta">
                    {item.exitCode !== undefined && (
                      <span className={`worklog-chip ${item.exitCode === 0 ? "is-ok" : "is-fail"}`}>
                        exit {item.exitCode}
                      </span>
                    )}
                    {item.duration && <span className="worklog-meta-text">{item.duration}</span>}
                    <button type="button" className="worklog-copy-btn" onClick={copyOutput}>
                      {copied ? "已复制" : "复制"}
                    </button>
                  </span>
                </div>
              )}
              <pre className={`worklog-code ${item.kind === "command" ? "is-terminal" : ""}`}>{item.body}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function TaskActivity({ block }: { block: ReplyBlock }) {
  const [open, setOpen] = useState(block.active);
  const [showAll, setShowAll] = useState(false);
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    setOpen(block.active);
  }, [block.active]);

  useEffect(() => {
    if (!block.active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [block.active]);

  const rawItems = block.activity.filter(message => message.text.trim());
  if (!block.active && rawItems.length === 0) return null;

  const parsedItems = rawItems.map((message, index) =>
    parseWorkLogItem(message, block.active, index === rawItems.length - 1),
  );
  const handoffs = parsedItems.filter(item => item.kind === "handoff");
  const timelineItems = collapseWorkLogItems(parsedItems.filter(item => item.kind !== "handoff"));

  const thoughts = timelineItems.filter(i => i.kind === "thinking").length;
  const reads = timelineItems
    .filter(i => i.kind === "file_read")
    .reduce((sum, i) => sum + (i.count ?? 1), 0);
  const changes = timelineItems.filter(i => i.kind === "file_change").length;
  const searches = timelineItems.filter(i => i.kind === "search").length;
  const commands = timelineItems.filter(i => i.kind === "command").length;
  const others = timelineItems.filter(
    i => !["thinking", "file_read", "file_change", "search", "command", "note"].includes(i.kind),
  ).length;
  const failed = timelineItems.some(i => i.status === "failed");

  const duration =
    !block.active && !block.endedAt
      ? "耗时未知"
      : elapsed(block.startedAt, block.active ? null : block.endedAt, now);
  const durationText =
    duration === "耗时未知"
      ? duration
      : `用时 ${duration.replace(" 分 ", "分").replace(" 秒", "秒")}`;

  const summaryParts = [
    reads && `读取 ${reads} 个文件`,
    changes && `修改 ${changes} 个文件`,
    searches && `${searches} 次搜索`,
    commands && `${commands} 次命令`,
    others && `${others} 次工具调用`,
    thoughts && `${thoughts} 段思考`,
  ].filter(Boolean);

  const activeStep = timelineItems.at(-1);
  const activeHeadline =
    activeStep?.kind === "thinking"
      ? "正在思考"
      : activeStep?.kind === "command"
        ? "正在执行命令"
        : activeStep?.kind === "file_read"
          ? `正在读取 ${activeStep.title.split("/").pop() ?? "文件"}`
          : activeStep?.kind === "file_change"
            ? "正在修改文件"
            : activeStep?.kind === "search"
              ? "正在搜索"
              : activeStep?.kind === "tool"
                ? `正在调用 ${activeStep.title.slice(0, 24)}`
                : "正在思考";

  const headerLabel = block.active
    ? `${activeHeadline} · ${durationText}`
    : outcomes[block.outcome ?? ""]
      ? `${outcomes[block.outcome!]} · ${durationText}`
      : summaryParts.length
        ? `${summaryParts.join(" · ")} · ${durationText}`
        : `过程记录 · ${durationText}`;

  const canFoldOverflow = !block.active && !showAll && timelineItems.length > MAX_COMPACT_ITEMS;
  const visibleItems = canFoldOverflow
    ? timelineItems.slice(0, MAX_COMPACT_ITEMS)
    : timelineItems;
  const hiddenCount = timelineItems.length - visibleItems.length;

  return (
    <div className={`task-activity worklog ${block.active ? "is-active" : ""} ${failed ? "has-failure" : ""}`}>
      <button
        type="button"
        className="task-activity-toggle worklog-toggle"
        aria-expanded={open}
        aria-controls={`activity-${block.id}`}
        onClick={() => setOpen(value => !value)}
      >
        <span className="worklog-toggle-leading">
          {block.active ? (
            <span className="worklog-pulse" aria-hidden="true" />
          ) : failed || block.outcome === "failed" ? (
            <Icon name="alert" size={14} />
          ) : (
            <Icon name="thought" size={14} />
          )}
          <span className="worklog-toggle-text">{headerLabel}</span>
        </span>
        <Icon name={open ? "down" : "right"} size={13} />
      </button>

      {open && (
        <div className="task-activity-items worklog-timeline" id={`activity-${block.id}`}>
          {handoffs.map(item => (
            <HandoffContext key={item.id} item={item} />
          ))}

          {visibleItems.length > 0 ? (
            <div className="worklog-list">
              {visibleItems.map((item, index) => {
                const isLatestActive = block.active && index === visibleItems.length - 1;
                const autoExpand =
                  (isLatestActive &&
                    (item.kind === "thinking" || item.kind === "command" || item.kind === "file_change")) ||
                  item.status === "failed";
                return <WorkLogRow key={item.id} item={item} defaultOpen={autoExpand} />;
              })}
              {hiddenCount > 0 && (
                <button
                  type="button"
                  className="worklog-more-btn"
                  onClick={() => setShowAll(true)}
                >
                  <span>展开其余 {hiddenCount} 项</span>
                  <Icon name="down" size={12} />
                </button>
              )}
              {!block.active && showAll && timelineItems.length > MAX_COMPACT_ITEMS && (
                <button
                  type="button"
                  className="worklog-more-btn"
                  onClick={() => setShowAll(false)}
                >
                  <span>收起多余项</span>
                  <Icon name="right" size={12} />
                </button>
              )}
            </div>
          ) : handoffs.length === 0 ? (
            <div className="worklog-waiting">
              <span className="worklog-spinner" aria-hidden="true" />
              <span>等待模型返回思考与执行步骤…</span>
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}
