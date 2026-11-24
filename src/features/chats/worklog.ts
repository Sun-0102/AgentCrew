import type { Message } from "./api";

export type WorkLogStatus = "running" | "succeeded" | "failed" | "neutral";

export interface FileDiffEntry {
  path: string;
  kind?: string;
  diff: string;
  additions: number;
  deletions: number;
}

export interface PlanEntry {
  content: string;
  status: string;
}

export interface WorkLogItem {
  id: string;
  kind: "thinking" | "command" | "file_read" | "file_change" | "search" | "plan" | "handoff" | "tool" | "note";
  action?: string;
  title: string;
  subtitle?: string;
  fullPath?: string;
  status: WorkLogStatus;
  badge?: string;
  additions?: number;
  deletions?: number;
  count?: number;
  duration?: string;
  exitCode?: number;
  body?: string;
  rawBody?: string;
  diffs?: FileDiffEntry[];
  plan?: PlanEntry[];
  createdAt: string | null;
}

const COMMON_ROOT_MARKERS = [
  "/src-tauri/",
  "/src/",
  "/packages/",
  "/apps/",
  "/scripts/",
  "/tests/",
  "/docs/",
  "/crates/",
  "/components/",
  "/features/",
];

export function shortenPath(raw: string): string {
  const cleaned = raw.trim().replace(/\\/g, "/").replace(/^\.\//, "");
  if (!cleaned) return "";
  const isAbs = cleaned.startsWith("/") || /^[a-zA-Z]:\//.test(cleaned);
  if (!isAbs) return cleaned;
  for (const marker of COMMON_ROOT_MARKERS) {
    const idx = cleaned.indexOf(marker);
    if (idx !== -1) return cleaned.slice(idx + 1);
  }
  const parts = cleaned.split("/").filter(Boolean);
  if (parts.length <= 3) return parts.join("/");
  return `…/${parts.slice(-3).join("/")}`;
}

function isSamePath(a?: string, b?: string): boolean {
  if (!a || !b) return false;
  const na = a.trim().replace(/\\/g, "/").replace(/^\.\//, "");
  const nb = b.trim().replace(/\\/g, "/").replace(/^\.\//, "");
  if (!na || !nb) return false;
  return na === nb || na.endsWith(`/${nb}`) || nb.endsWith(`/${na}`);
}

function normalizeStatus(raw?: string | null, fallback: WorkLogStatus = "neutral"): WorkLogStatus {
  const s = (raw ?? "").toLowerCase().trim();
  if (!s) return fallback;
  if (["inprogress", "in_progress", "running", "pending", "started", "queued"].includes(s)) return "running";
  if (["completed", "succeeded", "success", "done", "ok"].includes(s)) return "succeeded";
  if (["failed", "error", "declined", "cancelled", "canceled", "interrupted", "denied"].includes(s)) return "failed";
  return fallback;
}

function countDiff(diff: string): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) additions++;
    else if (line.startsWith("-") && !line.startsWith("---")) deletions++;
  }
  return { additions, deletions };
}

function formatDurationMs(raw: string): string {
  const ms = Number(raw);
  if (!Number.isFinite(ms)) return raw;
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

function firstLinePreview(text: string, max = 96): string {
  const line = text
    .split("\n")
    .map(l => l.replace(/^[#>*`-\s]+/, "").trim())
    .find(Boolean) ?? "";
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

function safeJson(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function parseCodexFileChanges(rawJson: string): FileDiffEntry[] {
  const parsed = safeJson(rawJson);
  if (!parsed) return [];
  const entries: FileDiffEntry[] = [];
  const pushEntry = (path: string, value: Record<string, unknown>) => {
    const diff =
      (typeof value.diff === "string" && value.diff) ||
      (typeof value.patch === "string" && value.patch) ||
      (typeof value.unified_diff === "string" && value.unified_diff) ||
      (typeof value.content === "string" && value.content) ||
      "";
    const kind =
      (typeof value.kind === "string" && value.kind) ||
      (typeof value.type === "string" && value.type) ||
      undefined;
    const { additions, deletions } = countDiff(diff);
    entries.push({ path, kind, diff, additions, deletions });
  };
  if (Array.isArray(parsed)) {
    for (const item of parsed) {
      if (item && typeof item === "object") {
        const rec = item as Record<string, unknown>;
        const path =
          (typeof rec.path === "string" && rec.path) ||
          (typeof rec.file === "string" && rec.file) ||
          (typeof rec.filePath === "string" && rec.filePath) ||
          "未知文件";
        pushEntry(path, rec);
      }
    }
  } else if (typeof parsed === "object") {
    for (const [path, val] of Object.entries(parsed as Record<string, unknown>)) {
      if (val && typeof val === "object") pushEntry(path, val as Record<string, unknown>);
      else entries.push({ path, diff: String(val ?? ""), additions: 0, deletions: 0 });
    }
  }
  return entries;
}

function buildSimpleDiff(oldText: string, newText: string): string {
  const out: string[] = [];
  if (oldText) {
    for (const line of oldText.split("\n")) out.push(`-${line}`);
  }
  if (newText) {
    for (const line of newText.split("\n")) out.push(`+${line}`);
  }
  return out.join("\n");
}

function parseGeminiUpdate(message: Message, parsed: Record<string, unknown>): WorkLogItem {
  const updateType = String(parsed.sessionUpdate ?? "");
  if (updateType === "plan") {
    const rawEntries = Array.isArray(parsed.entries) ? parsed.entries : [];
    const plan: PlanEntry[] = rawEntries
      .filter((e): e is Record<string, unknown> => !!e && typeof e === "object")
      .map(e => ({
        content: String(e.content ?? ""),
        status: String(e.status ?? "pending"),
      }))
      .filter(e => e.content);
    const doneCount = plan.filter(p => p.status === "completed").length;
    return {
      id: message.id,
      kind: "plan",
      title: "执行计划",
      subtitle: plan.length ? `${doneCount}/${plan.length} 已完成` : undefined,
      status: plan.length > 0 && doneCount === plan.length ? "succeeded" : "neutral",
      plan,
      createdAt: message.created_at,
    };
  }

  const rawTitle = String(parsed.title ?? parsed.kind ?? "工具调用").trim() || "工具调用";
  const toolKind = String(parsed.kind ?? "").toLowerCase();
  const status = normalizeStatus(typeof parsed.status === "string" ? parsed.status : undefined, "neutral");

  const locations = Array.isArray(parsed.locations)
    ? parsed.locations
        .map(loc => {
          if (!loc || typeof loc !== "object") return null;
          const rec = loc as Record<string, unknown>;
          const path = typeof rec.path === "string" ? rec.path.trim() : "";
          if (!path) return null;
          const line = typeof rec.line === "number" && Number.isFinite(rec.line) ? rec.line : undefined;
          return { path, line };
        })
        .filter((loc): loc is { path: string; line: number | undefined } => loc !== null)
    : [];
  const primaryLoc = locations[0];

  const contentList = Array.isArray(parsed.content) ? parsed.content : [];
  const diffs: FileDiffEntry[] = [];
  const textParts: string[] = [];

  for (const item of contentList) {
    if (!item || typeof item !== "object") continue;
    const rec = item as Record<string, unknown>;
    if (rec.type === "diff") {
      const path = String(rec.path ?? primaryLoc?.path ?? "文件修改");
      const diff =
        typeof rec.diff === "string"
          ? rec.diff
          : buildSimpleDiff(String(rec.oldText ?? ""), String(rec.newText ?? ""));
      const { additions, deletions } = countDiff(diff);
      diffs.push({ path: shortenPath(path) || path, kind: "update", diff, additions, deletions });
    } else if (rec.type === "content" && rec.content && typeof rec.content === "object") {
      const inner = rec.content as Record<string, unknown>;
      if (typeof inner.text === "string" && inner.text.trim()) textParts.push(inner.text);
    } else if (typeof rec.text === "string" && rec.text.trim()) {
      textParts.push(rec.text);
    }
  }

  if (parsed.rawInput !== undefined && parsed.rawInput !== null) {
    const inputStr = typeof parsed.rawInput === "string" ? parsed.rawInput : JSON.stringify(parsed.rawInput, null, 2);
    if (inputStr && inputStr !== "{}") textParts.unshift(`输入：\n${inputStr}`);
  }
  if (parsed.rawOutput !== undefined && parsed.rawOutput !== null && !textParts.length) {
    const outStr = typeof parsed.rawOutput === "string" ? parsed.rawOutput : JSON.stringify(parsed.rawOutput, null, 2);
    if (outStr && outStr !== "{}") textParts.push(outStr);
  }

  const editMatch = rawTitle.match(/^(?:edit|write|patch|modify|update|create|delete|file_edit|write_file|replace)\s+(.+)$/i);
  if (diffs.length > 0 || toolKind === "edit" || toolKind === "delete" || editMatch) {
    const totalAdd = diffs.reduce((sum, d) => sum + d.additions, 0);
    const totalDel = diffs.reduce((sum, d) => sum + d.deletions, 0);
    const displayPath =
      diffs.length === 1
        ? diffs[0].path
        : editMatch
          ? shortenPath(editMatch[1])
          : primaryLoc
            ? shortenPath(primaryLoc.path)
            : rawTitle;
    const action = toolKind === "delete" || /^delete\b/i.test(rawTitle) ? "删除" : /^create\b/i.test(rawTitle) ? "新建" : "修改";
    return {
      id: message.id,
      kind: "file_change",
      action,
      title: diffs.length > 1 ? `${diffs.length} 个文件` : displayPath,
      subtitle: diffs.length > 1 ? diffs.map(d => d.path.split("/").pop()).join("、") : undefined,
      fullPath: primaryLoc?.path,
      status,
      additions: totalAdd || undefined,
      deletions: totalDel || undefined,
      diffs: diffs.length ? diffs : undefined,
      body: textParts.join("\n\n") || undefined,
      createdAt: message.created_at,
    };
  }

  const readMatch = rawTitle.match(/^(?:file|read|read_file|view|view_file|open|cat|inspect)\s+(.+)$/i);
  if (toolKind === "read" || readMatch) {
    const pathTarget = readMatch ? shortenPath(readMatch[1]) : primaryLoc ? shortenPath(primaryLoc.path) : rawTitle;
    const lineHint = primaryLoc?.line ? `L${primaryLoc.line}` : undefined;
    const extraLoc =
      primaryLoc && !isSamePath(pathTarget, primaryLoc.path) ? shortenPath(primaryLoc.path) : undefined;
    return {
      id: message.id,
      kind: "file_read",
      action: "读取",
      title: pathTarget,
      subtitle: lineHint ?? extraLoc,
      fullPath: primaryLoc?.path,
      status,
      body: textParts.join("\n\n") || undefined,
      createdAt: message.created_at,
    };
  }

  const isCommand = toolKind === "execute" || /^(run|exec|shell|bash|terminal|pwsh|cmd)\b/i.test(rawTitle);
  if (isCommand) {
    const cleanCmd = rawTitle.replace(/^(?:run|exec|execute|shell|bash|terminal)\s*[:：]?\s+/i, "").trim() || rawTitle;
    const locHint =
      primaryLoc && !cleanCmd.includes(shortenPath(primaryLoc.path)) ? shortenPath(primaryLoc.path) : undefined;
    return {
      id: message.id,
      kind: "command",
      title: cleanCmd,
      subtitle: locHint,
      fullPath: primaryLoc?.path,
      status,
      body: textParts.join("\n\n") || undefined,
      createdAt: message.created_at,
    };
  }

  const isSearch = toolKind === "search" || /^(search|grep|rg|find|glob|list|web)/i.test(rawTitle);
  if (isSearch) {
    const cleanQuery = rawTitle.replace(/^(?:search|grep|find|web_search|search_web)\s*[:：]?\s+/i, "").trim() || rawTitle;
    const locHint =
      primaryLoc && !isSamePath(cleanQuery, primaryLoc.path) ? shortenPath(primaryLoc.path) : undefined;
    return {
      id: message.id,
      kind: "search",
      action: "搜索",
      title: cleanQuery,
      subtitle: locHint,
      fullPath: primaryLoc?.path,
      status,
      body: textParts.join("\n\n") || undefined,
      createdAt: message.created_at,
    };
  }

  const nonDuplicateSubtitle =
    primaryLoc && !isSamePath(rawTitle, primaryLoc.path) ? shortenPath(primaryLoc.path) : undefined;

  return {
    id: message.id,
    kind: "tool",
    title: rawTitle,
    subtitle: nonDuplicateSubtitle,
    fullPath: primaryLoc?.path,
    status,
    body: textParts.join("\n\n") || undefined,
    createdAt: message.created_at,
  };
}

export function parseWorkLogItem(message: Message, turnActive: boolean, isLast: boolean): WorkLogItem {
  if (message.role === "thinking") {
    return {
      id: message.id,
      kind: "thinking",
      title: turnActive && isLast ? "正在思考…" : "思考过程",
      subtitle: firstLinePreview(message.text, 88),
      status: turnActive && isLast ? "running" : "neutral",
      body: message.text,
      createdAt: message.created_at,
    };
  }

function extractAssignmentDisplay(text: string): { summary: string; rawBody?: string } {
  const marker = "本轮投递内容：\n";
  const start = text.indexOf(marker);
  if (start === -1) {
    return { summary: text };
  }
  let content = text.slice(start + marker.length);
  const stopMarkers = [
    "\n\n本群最近聊天",
    "\n\n上一轮归档记录",
    "\n\n程序采集的当前工作区依据",
    "\n\n当前工作区依据",
    "\n\n截断、非 Git",
  ];
  let end = content.length;
  for (const stop of stopMarkers) {
    const idx = content.indexOf(stop);
    if (idx !== -1 && idx < end) end = idx;
  }
  content = content.slice(0, end).trim();
  const userHeader = /^本轮处理用户消息 [^\n]*?以用户的新消息为准。\n\n/;
  if (userHeader.test(content)) {
    content = content.replace(userHeader, "").trim();
  }
  if (!content || content === text.trim()) {
    return { summary: text };
  }
  return { summary: content, rawBody: text };
}

  if (message.role === "assignment") {
    const { summary, rawBody } = extractAssignmentDisplay(message.text);
    return {
      id: message.id,
      kind: "handoff",
      title: "交接要求",
      subtitle: firstLinePreview(summary, 88),
      status: "neutral",
      body: summary,
      rawBody,
      createdAt: message.created_at,
    };
  }

  if (message.role === "assistant") {
    return {
      id: message.id,
      kind: "note",
      title: "过程说明",
      status: "neutral",
      body: message.text,
      createdAt: message.created_at,
    };
  }

  const text = message.text.trim();

  // 1. Codex commandExecution
  if (text.startsWith("命令：")) {
    const lines = message.text.split("\n");
    const cmd = lines[0].slice("命令：".length).trim();
    let statusRaw = "";
    let exitCode: number | undefined;
    let duration: string | undefined;
    let bodyStart = 1;
    for (let i = 1; i < Math.min(lines.length, 6); i++) {
      const line = lines[i];
      if (line.startsWith("状态：")) {
        statusRaw = line.slice("状态：".length).trim();
        bodyStart = i + 1;
      } else if (line.startsWith("退出码：")) {
        const parsed = Number(line.slice("退出码：".length).trim());
        if (Number.isFinite(parsed)) exitCode = parsed;
        bodyStart = i + 1;
      } else if (line.startsWith("耗时：")) {
        duration = formatDurationMs(line.slice("耗时：".length).replace(/ms$/i, "").trim());
        bodyStart = i + 1;
      } else {
        break;
      }
    }
    const output = lines.slice(bodyStart).join("\n").trim();
    let status = normalizeStatus(statusRaw, turnActive && isLast ? "running" : "succeeded");
    if (exitCode !== undefined && exitCode !== 0 && status === "succeeded") status = "failed";
    return {
      id: message.id,
      kind: "command",
      title: cmd || "执行命令",
      status,
      exitCode,
      duration,
      body: output || undefined,
      createdAt: message.created_at,
    };
  }

  // 2. Codex fileChange
  if (text.startsWith("文件变更：")) {
    const rawJson = text.slice("文件变更：".length).trim();
    const diffs = parseCodexFileChanges(rawJson).map(entry => ({
      ...entry,
      path: shortenPath(entry.path) || entry.path,
    }));
    const totalAdd = diffs.reduce((sum, d) => sum + d.additions, 0);
    const totalDel = diffs.reduce((sum, d) => sum + d.deletions, 0);
    const title =
      diffs.length === 1
        ? diffs[0].path
        : diffs.length > 1
          ? `${diffs.length} 个文件`
          : "文件变更";
    return {
      id: message.id,
      kind: "file_change",
      action: "修改",
      title,
      subtitle: diffs.length > 1 ? diffs.map(d => d.path.split(/[\\/]/).pop()).join("、") : undefined,
      status: turnActive && isLast ? "running" : "succeeded",
      additions: totalAdd || undefined,
      deletions: totalDel || undefined,
      diffs: diffs.length ? diffs : undefined,
      body: diffs.length ? undefined : rawJson,
      createdAt: message.created_at,
    };
  }

  // 3. Codex webSearch
  if (text.startsWith("联网搜索：")) {
    const lines = text.split("\n");
    const query = lines[0].slice("联网搜索：".length).trim();
    const statusRaw = lines[1]?.startsWith("状态：") ? lines[1].slice("状态：".length).trim() : "";
    return {
      id: message.id,
      kind: "search",
      action: "搜索",
      title: query || "联网搜索",
      status: normalizeStatus(statusRaw, turnActive && isLast ? "running" : "succeeded"),
      createdAt: message.created_at,
    };
  }

  // 4. Codex MCP tool
  if (text.startsWith("MCP 工具：")) {
    const lines = text.split("\n");
    const name = lines[0].slice("MCP 工具：".length).trim();
    const statusRaw = lines[1]?.startsWith("状态：") ? lines[1].slice("状态：".length).trim() : "";
    const payloadRaw = lines.slice(2).join("\n").trim();
    const parsed = safeJson(payloadRaw);
    const pretty = parsed ? JSON.stringify(parsed, null, 2) : payloadRaw;
    return {
      id: message.id,
      kind: "tool",
      title: name || "MCP 工具",
      status: normalizeStatus(statusRaw, turnActive && isLast ? "running" : "succeeded"),
      body: pretty || undefined,
      createdAt: message.created_at,
    };
  }

  // 5. DeepSeek tool call start
  if (text.startsWith("工具开始：")) {
    const lines = text.split("\n");
    const name = lines[0].slice("工具开始：".length).trim();
    const argsRaw = lines.slice(1).join("\n").trim();
    const parsed = safeJson(argsRaw);
    const rec = parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : null;
    const isCommand = /^(pwsh|bash|shell|sh|cmd|terminal|exec|run_command)/i.test(name);
    const isRead = /^(read_file|view_file|read|open_file|cat)/i.test(name);
    const isEdit = /^(edit_file|write_file|replace|patch|apply_patch)/i.test(name);
    const isSearch = /^(search|grep|rg|find|glob|list_dir|web_search)/i.test(name);

    const rawPath = rec ? String(rec.path ?? rec.file_path ?? rec.file ?? "") : "";
    const rawCmd = rec ? String(rec.command ?? rec.cmd ?? "") : "";
    const rawQuery = rec ? String(rec.query ?? rec.pattern ?? "") : "";
    const status = turnActive && isLast ? "running" : "succeeded";
    const body = parsed ? JSON.stringify(parsed, null, 2) : argsRaw || undefined;

    if (isCommand) {
      return {
        id: message.id,
        kind: "command",
        title: rawCmd || name || "执行命令",
        status,
        body,
        createdAt: message.created_at,
      };
    }
    if (isRead && rawPath) {
      return {
        id: message.id,
        kind: "file_read",
        action: "读取",
        title: shortenPath(rawPath),
        fullPath: rawPath,
        status,
        body,
        createdAt: message.created_at,
      };
    }
    if (isEdit && rawPath) {
      return {
        id: message.id,
        kind: "file_change",
        action: "修改",
        title: shortenPath(rawPath),
        fullPath: rawPath,
        status,
        body,
        createdAt: message.created_at,
      };
    }
    if (isSearch) {
      return {
        id: message.id,
        kind: "search",
        action: "搜索",
        title: rawQuery || shortenPath(rawPath) || name,
        status,
        body,
        createdAt: message.created_at,
      };
    }
    const subtitle = (rawCmd || (rawPath ? shortenPath(rawPath) : "") || rawQuery || argsRaw).slice(0, 88) || undefined;
    return {
      id: message.id,
      kind: "tool",
      title: name || "工具调用",
      subtitle,
      status,
      body,
      createdAt: message.created_at,
    };
  }

  // 6. Gemini ACP JSON tool/plan update
  if (text.startsWith("{")) {
    const parsed = safeJson(text);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parseGeminiUpdate(message, parsed as Record<string, unknown>);
    }
  }

  // Fallback generic tool output (e.g. DeepSeek tool/result)
  return {
    id: message.id,
    kind: "tool",
    title: "工具输出",
    subtitle: firstLinePreview(text, 88),
    status: "succeeded",
    body: message.text,
    createdAt: message.created_at,
  };
}

export function collapseWorkLogItems(items: WorkLogItem[]): WorkLogItem[] {
  const result: WorkLogItem[] = [];
  for (const item of items) {
    const prev = result.at(-1);
    if (
      prev &&
      prev.kind === "file_read" &&
      item.kind === "file_read" &&
      prev.title === item.title &&
      prev.status !== "failed" &&
      item.status !== "failed"
    ) {
      const mergedBody =
        prev.body && item.body && prev.body !== item.body
          ? `${prev.body}\n\n---\n\n${item.body}`
          : item.body ?? prev.body;
      result[result.length - 1] = {
        ...prev,
        status: item.status === "running" ? "running" : prev.status === "succeeded" || item.status === "succeeded" ? "succeeded" : item.status,
        count: (prev.count ?? 1) + (item.count ?? 1),
        subtitle: item.subtitle ?? prev.subtitle,
        body: mergedBody,
        createdAt: item.createdAt ?? prev.createdAt,
      };
      continue;
    }
    result.push(item);
  }
  return result;
}
