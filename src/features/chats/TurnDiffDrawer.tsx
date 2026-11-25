import { useCallback, useEffect, useMemo, useRef, useState, type ComponentProps } from "react";
import { File as CodeFile } from "@pierre/diffs/react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import Icon from "../../components/Icon";
import DiffViewer from "../git/DiffViewer";
import {
  gitFileDiff,
  gitOverview,
  gitTurnFileDiff,
  workspaceReadFile,
  type GitFileDiff,
  type GitOverview,
  type TurnDiffFile,
  type TurnDiffSummary,
  type WorkspaceFileContent,
} from "../git/api";
import { desktop } from "./api";
import "./turn-diff.css";

export interface TurnDiffOption {
  id: string;
  label: string;
  summary: TurnDiffSummary;
}

export interface FilePreviewTarget {
  path: string;
  line?: number;
}

interface Props {
  turns: TurnDiffOption[];
  selectedTurnId: string;
  selectedFile: string | null;
  previewFile?: FilePreviewTarget | null;
  workspacePath?: string;
  refreshToken?: number;
  onSelectTurn: (turnId: string) => void;
  onSelectFile: (filePath: string | null) => void;
  onClearPreview?: () => void;
  onOpenFullGit?: (path?: string) => void;
  onClose: () => void;
}

const WIDTH_STORAGE_KEY = "ai-gent.diff-drawer-width.v1";

function readSavedWidth(): number | null {
  try {
    const raw = Number(localStorage.getItem(WIDTH_STORAGE_KEY));
    return Number.isFinite(raw) && raw >= 320 ? raw : null;
  } catch {
    return null;
  }
}

function splitPath(fullPath: string): { dir: string; name: string } {
  const normalized = fullPath.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx === -1) return { dir: "", name: normalized };
  return { dir: normalized.slice(0, idx + 1), name: normalized.slice(idx + 1) };
}

function statusBadgeClass(status: string): string {
  const c = status.trim().toUpperCase();
  if (c === "A" || c === "?") return "is-a";
  if (c === "D") return "is-d";
  if (c === "R" || c === "C") return "is-r";
  return "is-m";
}

interface ParsedUnifiedRow {
  kind: "hunk" | "context" | "add" | "del";
  oldLine: number | null;
  newLine: number | null;
  text: string;
}

interface ParsedSplitRow {
  kind: "hunk" | "pair";
  hunkText?: string;
  left?: { line: number | null; text: string; kind: "context" | "del" | "empty" };
  right?: { line: number | null; text: string; kind: "context" | "add" | "empty" };
}

function parseUnifiedPatch(patch: string): ParsedUnifiedRow[] {
  const rows: ParsedUnifiedRow[] = [];
  let oldNum = 1;
  let newNum = 1;
  for (const raw of patch.split("\n")) {
    if (raw.startsWith("@@")) {
      const match = raw.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldNum = Number(match[1]);
        newNum = Number(match[2]);
      }
      rows.push({ kind: "hunk", oldLine: null, newLine: null, text: raw });
    } else if (raw.startsWith("+++") || raw.startsWith("---") || raw.startsWith("diff --git") || raw.startsWith("index ")) {
      continue;
    } else if (raw.startsWith("+")) {
      rows.push({ kind: "add", oldLine: null, newLine: newNum++, text: raw.slice(1) });
    } else if (raw.startsWith("-")) {
      rows.push({ kind: "del", oldLine: oldNum++, newLine: null, text: raw.slice(1) });
    } else {
      const content = raw.startsWith(" ") ? raw.slice(1) : raw;
      rows.push({ kind: "context", oldLine: oldNum++, newLine: newNum++, text: content });
    }
  }
  return rows;
}

function toSplitRows(unified: ParsedUnifiedRow[]): ParsedSplitRow[] {
  const out: ParsedSplitRow[] = [];
  let i = 0;
  while (i < unified.length) {
    const row = unified[i];
    if (row.kind === "hunk") {
      out.push({ kind: "hunk", hunkText: row.text });
      i++;
      continue;
    }
    if (row.kind === "context") {
      out.push({
        kind: "pair",
        left: { line: row.oldLine, text: row.text, kind: "context" },
        right: { line: row.newLine, text: row.text, kind: "context" },
      });
      i++;
      continue;
    }
    const dels: ParsedUnifiedRow[] = [];
    const adds: ParsedUnifiedRow[] = [];
    while (i < unified.length && unified[i].kind === "del") {
      dels.push(unified[i++]);
    }
    while (i < unified.length && unified[i].kind === "add") {
      adds.push(unified[i++]);
    }
    const maxLen = Math.max(dels.length, adds.length);
    for (let k = 0; k < maxLen; k++) {
      const d = dels[k];
      const a = adds[k];
      out.push({
        kind: "pair",
        left: d
          ? { line: d.oldLine, text: d.text, kind: "del" }
          : { line: null, text: "", kind: "empty" },
        right: a
          ? { line: a.newLine, text: a.text, kind: "add" }
          : { line: null, text: "", kind: "empty" },
      });
    }
  }
  return out;
}

function InlinePatchView({
  filePath,
  patch,
  diffStyle,
  wrap,
}: {
  filePath: string;
  patch: string;
  diffStyle: "unified" | "split";
  wrap: boolean;
}) {
  const unifiedRows = useMemo(() => parseUnifiedPatch(patch), [patch]);
  const splitRows = useMemo(
    () => (diffStyle === "split" ? toSplitRows(unifiedRows) : []),
    [diffStyle, unifiedRows],
  );

  return (
    <div className={`turn-diff-inline-patch ${wrap ? "is-wrap" : "is-scroll"}`}>
      <div className="turn-diff-inline-header">{filePath}</div>
      {diffStyle === "split" ? (
        <div className="turn-diff-split-table" role="table" aria-label="双栏代码对比">
          {splitRows.map((row, idx) =>
            row.kind === "hunk" ? (
              <div key={idx} className="turn-diff-split-hunk">
                {row.hunkText}
              </div>
            ) : (
              <div key={idx} className="turn-diff-split-row">
                <div className={`turn-diff-split-cell is-${row.left?.kind ?? "empty"}`}>
                  <span className="turn-diff-ln">{row.left?.line ?? ""}</span>
                  <span className="turn-diff-sign">
                    {row.left?.kind === "del" ? "-" : " "}
                  </span>
                  <span className="turn-diff-code-text">{row.left?.text ?? ""}</span>
                </div>
                <div className={`turn-diff-split-cell is-${row.right?.kind ?? "empty"}`}>
                  <span className="turn-diff-ln">{row.right?.line ?? ""}</span>
                  <span className="turn-diff-sign">
                    {row.right?.kind === "add" ? "+" : " "}
                  </span>
                  <span className="turn-diff-code-text">{row.right?.text ?? ""}</span>
                </div>
              </div>
            ),
          )}
        </div>
      ) : (
        <div className="turn-diff-unified-table" role="table" aria-label="单栏代码对比">
          {unifiedRows.map((row, idx) =>
            row.kind === "hunk" ? (
              <div key={idx} className="turn-diff-unified-hunk">
                {row.text}
              </div>
            ) : (
              <div key={idx} className={`turn-diff-unified-row is-${row.kind}`}>
                <span className="turn-diff-ln">{row.oldLine ?? ""}</span>
                <span className="turn-diff-ln">{row.newLine ?? ""}</span>
                <span className="turn-diff-sign">
                  {row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}
                </span>
                <span className="turn-diff-code-text">{row.text}</span>
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}

type PreviewFormat = "markdown" | "html" | null;

function detectPreviewFormat(path?: string | null): PreviewFormat {
  if (!path) return null;
  const lower = path.trim().toLowerCase();
  if (lower.endsWith(".md") || lower.endsWith(".markdown") || lower.endsWith(".mdx")) {
    return "markdown";
  }
  if (lower.endsWith(".html") || lower.endsWith(".htm")) {
    return "html";
  }
  return null;
}

function readTheme(): "light" | "dark" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

function FileCodePreview({
  file,
  loading,
  error,
  targetLine,
  wrap,
  mode,
  format,
}: {
  file: WorkspaceFileContent | null;
  loading: boolean;
  error: string | null;
  targetLine?: number;
  wrap: boolean;
  mode: "rendered" | "source";
  format: PreviewFormat;
}) {
  const [theme, setTheme] = useState<"light" | "dark">(readTheme);

  useEffect(() => {
    const observer = new MutationObserver(() => setTheme(readTheme()));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => observer.disconnect();
  }, []);

  const codeOptions = useMemo(
    () => ({
      overflow: wrap ? ("wrap" as const) : ("scroll" as const),
      themeType: theme,
      disableFileHeader: true,
    }),
    [wrap, theme],
  );

  const selectedLines = useMemo(
    () => (targetLine ? { start: targetLine, end: targetLine } : null),
    [targetLine],
  );

  const markdownComponents = useMemo<NonNullable<ComponentProps<typeof Markdown>["components"]>>(
    () => ({
      table: ({ children }) => (
        <div className="message-table" role="region" aria-label="文档表格" tabIndex={0}>
          <table>{children}</table>
        </div>
      ),
      a: ({ node: _node, href, children, ...props }) => (
        <a {...props} href={href} target="_blank" rel="noopener noreferrer">
          {children}
        </a>
      ),
    }),
    [],
  );

  if (loading) {
    return (
      <div className="git-diff-empty">
        <span className="small-spinner" />
        <span>正在读取文件内容…</span>
      </div>
    );
  }
  if (error) {
    return (
      <div className="git-diff-empty">
        <strong>无法打开文件</strong>
        <p>{error}</p>
      </div>
    );
  }
  if (!file) {
    return (
      <div className="git-diff-empty">
        <strong>未选择文件</strong>
      </div>
    );
  }
  if (file.is_binary) {
    return (
      <div className="git-diff-empty">
        <strong>二进制文件</strong>
        <p>{file.display_path} 为二进制文件，暂不支持文本预览。</p>
      </div>
    );
  }

  if (mode === "rendered" && format === "markdown") {
    return (
      <div className="file-markdown-preview">
        {file.truncated && (
          <div className="git-diff-banner">
            文件超过 512 KB，已截取前部内容用于快速预览。
          </div>
        )}
        <div className="message-body file-markdown-body">
          <Markdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {file.contents}
          </Markdown>
        </div>
      </div>
    );
  }

  if (mode === "rendered" && format === "html") {
    return (
      <div className="file-html-preview-wrap">
        {file.truncated && (
          <div className="git-diff-banner">
            文件超过 512 KB，已截取前部内容用于快速预览。
          </div>
        )}
        <iframe
          key={`${file.path}:${file.contents.length}`}
          className="file-html-preview"
          title={file.display_path || file.path}
          srcDoc={file.contents}
          sandbox="allow-scripts allow-forms allow-modals allow-popups"
        />
      </div>
    );
  }

  return (
    <div className="git-diff-canvas file-code-highlight-canvas">
      {file.truncated && (
        <div className="git-diff-banner">
          文件超过 512 KB，已截取前部内容用于快速预览。
        </div>
      )}
      <CodeFile
        key={`${file.path}:${theme}:${wrap ? "wrap" : "scroll"}:${file.contents.length}:${targetLine ?? ""}`}
        file={{
          name: file.display_path || file.path,
          contents: file.contents,
        }}
        selectedLines={selectedLines}
        options={codeOptions}
        disableWorkerPool
      />
    </div>
  );
}

export default function TurnDiffDrawer({
  turns,
  selectedTurnId,
  selectedFile,
  previewFile = null,
  workspacePath,
  refreshToken = 0,
  onSelectTurn,
  onSelectFile,
  onClearPreview,
  onOpenFullGit,
  onClose,
}: Props) {
  const [diffStyle, setDiffStyle] = useState<"unified" | "split">("unified");
  const [wrapLines, setWrapLines] = useState(true);
  const [diffData, setDiffData] = useState<GitFileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [diffError, setDiffError] = useState<string | null>(null);
  const [workingOverview, setWorkingOverview] = useState<GitOverview | null>(null);
  const [workingLoading, setWorkingLoading] = useState(false);
  const [localRefresh, setLocalRefresh] = useState(0);

  const [previewData, setPreviewData] = useState<WorkspaceFileContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewViewMode, setPreviewViewMode] = useState<"rendered" | "source">("rendered");

  const [drawerWidth, setDrawerWidth] = useState<number | null>(readSavedWidth);
  const [resizing, setResizing] = useState(false);
  const drawerRef = useRef<HTMLElement>(null);

  const isPreviewMode = Boolean(previewFile);
  const previewFormat = useMemo(
    () => detectPreviewFormat(previewData?.display_path ?? previewFile?.path),
    [previewData?.display_path, previewFile?.path],
  );
  const effectivePreviewMode: "rendered" | "source" =
    previewFormat ? previewViewMode : "source";

  useEffect(() => {
    const fmt = detectPreviewFormat(previewFile?.path);
    if (fmt && !previewFile?.line) {
      setPreviewViewMode("rendered");
    } else {
      setPreviewViewMode("source");
    }
  }, [previewFile?.path, previewFile?.line]);
  const isWorkingMode = selectedTurnId === "__working__";
  const activeTurn = useMemo(
    () => turns.find(t => t.id === selectedTurnId) ?? turns.at(-1) ?? null,
    [turns, selectedTurnId],
  );
  const repoPath = activeTurn?.summary.repo_root || workspacePath || "";

  // Drag resize handler on the left edge of the drawer
  const startResize = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const drawerEl = drawerRef.current;
    const parentEl = drawerEl?.parentElement;
    if (!drawerEl || !parentEl) return;

    const startX = event.clientX;
    const startWidth = drawerEl.getBoundingClientRect().width;
    const maxWidth = Math.max(420, Math.floor(parentEl.getBoundingClientRect().width * 0.78));
    setResizing(true);

    const onMove = (moveEvent: MouseEvent) => {
      const delta = startX - moveEvent.clientX;
      const next = Math.min(maxWidth, Math.max(340, Math.round(startWidth + delta)));
      setDrawerWidth(next);
    };
    const onUp = () => {
      setResizing(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (drawerRef.current) {
        const finalWidth = Math.round(drawerRef.current.getBoundingClientRect().width);
        try {
          localStorage.setItem(WIDTH_STORAGE_KEY, String(finalWidth));
        } catch {
          // ignore storage quota errors
        }
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, []);

  // Load preview file when in code preview mode
  useEffect(() => {
    if (!previewFile) {
      setPreviewData(null);
      setPreviewError(null);
      return;
    }
    if (!desktop) {
      setPreviewError("请在桌面版中查看本地代码文件");
      return;
    }
    let active = true;
    setPreviewLoading(true);
    setPreviewError(null);
    void workspaceReadFile(repoPath || workspacePath, previewFile.path)
      .then(result => {
        if (active) setPreviewData(result);
      })
      .catch(cause => {
        if (active) {
          setPreviewError(String(cause));
          setPreviewData(null);
        }
      })
      .finally(() => {
        if (active) setPreviewLoading(false);
      });
    return () => {
      active = false;
    };
  }, [previewFile?.path, previewFile?.line, repoPath, workspacePath, localRefresh]);

  const loadWorkingTree = useCallback(async () => {
    if (!desktop || !repoPath) {
      setWorkingOverview(null);
      return;
    }
    setWorkingLoading(true);
    setDiffError(null);
    try {
      const ov = await gitOverview(repoPath);
      setWorkingOverview(ov);
    } catch (cause) {
      setDiffError(String(cause));
      setWorkingOverview(null);
    } finally {
      setWorkingLoading(false);
    }
  }, [repoPath]);

  useEffect(() => {
    if (!isPreviewMode && isWorkingMode) {
      void loadWorkingTree();
    }
  }, [isPreviewMode, isWorkingMode, loadWorkingTree, refreshToken, localRefresh]);

  const files: TurnDiffFile[] = useMemo(() => {
    if (isWorkingMode) {
      return (workingOverview?.files ?? []).map(f => ({
        path: f.path,
        status: f.untracked ? "A" : f.unstaged_status.trim() || f.staged_status.trim() || "M",
        additions: f.additions,
        deletions: f.deletions,
      }));
    }
    return activeTurn?.summary.files ?? [];
  }, [isWorkingMode, workingOverview?.files, activeTurn]);

  const effectiveFile = useMemo(() => {
    if (selectedFile && files.some(f => f.path === selectedFile)) return selectedFile;
    return files[0]?.path ?? null;
  }, [selectedFile, files]);

  const effectiveFileObj = useMemo(
    () => files.find(f => f.path === effectiveFile) ?? null,
    [files, effectiveFile],
  );

  const totalAdditions = useMemo(
    () => files.reduce((sum, f) => sum + f.additions, 0),
    [files],
  );
  const totalDeletions = useMemo(
    () => files.reduce((sum, f) => sum + f.deletions, 0),
    [files],
  );

  useEffect(() => {
    if (isPreviewMode || !effectiveFile) {
      setDiffData(null);
      setDiffError(null);
      return;
    }
    if (!isWorkingMode && activeTurn && !activeTurn.summary.from_git) {
      setDiffData(null);
      setDiffError(null);
      return;
    }
    if (!desktop) {
      setDiffData(null);
      return;
    }
    let active = true;
    setDiffLoading(true);
    setDiffError(null);
    const promise = isWorkingMode
      ? workingOverview?.repo_root
        ? gitFileDiff(workingOverview.repo_root, effectiveFile, "all")
        : Promise.resolve(null)
      : activeTurn?.summary.repo_root &&
          activeTurn.summary.before_tree &&
          activeTurn.summary.after_tree
        ? gitTurnFileDiff(
            activeTurn.summary.repo_root,
            activeTurn.summary.before_tree,
            activeTurn.summary.after_tree,
            effectiveFile,
          )
        : Promise.resolve(null);

    void promise
      .then(result => {
        if (active) setDiffData(result);
      })
      .catch(cause => {
        if (active) {
          setDiffError(String(cause));
          setDiffData(null);
        }
      })
      .finally(() => {
        if (active) setDiffLoading(false);
      });
    return () => {
      active = false;
    };
  }, [
    isPreviewMode,
    isWorkingMode,
    activeTurn?.id,
    activeTurn?.summary.repo_root,
    activeTurn?.summary.before_tree,
    activeTurn?.summary.after_tree,
    activeTurn?.summary.from_git,
    workingOverview?.repo_root,
    effectiveFile,
    refreshToken,
    localRefresh,
  ]);

  const drawerStyle = drawerWidth
    ? { width: `${drawerWidth}px`, flexBasis: `${drawerWidth}px` }
    : undefined;

  return (
    <aside
      ref={drawerRef}
      style={drawerStyle}
      className={`turn-diff-drawer ${resizing ? "is-resizing" : ""}`}
      aria-label={isPreviewMode ? "代码文件预览" : "代码变更对比"}
    >
      <div
        className="turn-diff-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="拖拽调整代码面板宽度"
        title="拖拽调整宽度"
        onMouseDown={startResize}
      />

      <header className="turn-diff-drawer-header">
        {isPreviewMode ? (
          <div className="turn-diff-scope-group">
            {turns.length > 0 && onClearPreview && (
              <button
                type="button"
                className="turn-diff-tool-btn"
                onClick={onClearPreview}
                title="返回代码变更对比"
              >
                <Icon name="back" size={13} />
                变更
              </button>
            )}
            <span
              className="turn-diff-preview-title"
              title={previewData?.path ?? previewFile?.path}
            >
              {previewData?.display_path ?? previewFile?.path}
              {previewFile?.line ? `:${previewFile.line}` : ""}
            </span>
          </div>
        ) : (
          <div className="turn-diff-scope-group">
            <select
              className="turn-diff-scope-select"
              aria-label="选择变更范围"
              value={isWorkingMode ? "__working__" : activeTurn?.id ?? ""}
              onChange={e => {
                onSelectTurn(e.target.value);
                onSelectFile(null);
              }}
            >
              {turns.map(opt => (
                <option key={opt.id} value={opt.id}>
                  {opt.label}
                </option>
              ))}
              {repoPath && <option value="__working__">当前工作区全部变更</option>}
            </select>
            <span className="turn-diff-drawer-stats">
              <span className="diff-add">+{totalAdditions}</span>
              <span className="diff-del">-{totalDeletions}</span>
            </span>
          </div>
        )}

        <div className="turn-diff-drawer-tools">
          {isPreviewMode && previewFormat && (
            <div className="turn-diff-seg" role="group" aria-label="文件预览模式">
              <button
                type="button"
                className={effectivePreviewMode === "rendered" ? "active" : ""}
                onClick={() => setPreviewViewMode("rendered")}
                title="按原格式预览"
              >
                预览
              </button>
              <button
                type="button"
                className={effectivePreviewMode === "source" ? "active" : ""}
                onClick={() => setPreviewViewMode("source")}
                title="查看语法高亮源码"
              >
                源码
              </button>
            </div>
          )}
          {!isPreviewMode && (
            <div className="turn-diff-seg" role="group" aria-label="Diff 布局">
              <button
                type="button"
                className={diffStyle === "unified" ? "active" : ""}
                onClick={() => setDiffStyle("unified")}
                title="单栏对比"
              >
                单栏
              </button>
              <button
                type="button"
                className={diffStyle === "split" ? "active" : ""}
                onClick={() => setDiffStyle("split")}
                title="双栏对比"
              >
                双栏
              </button>
            </div>
          )}
          {(!isPreviewMode || effectivePreviewMode === "source") && (
            <button
              type="button"
              className={`turn-diff-tool-btn ${wrapLines ? "active" : ""}`}
              onClick={() => setWrapLines(w => !w)}
              title="切换自动换行"
            >
              换行
            </button>
          )}
          <button
            type="button"
            className="icon-button"
            onClick={() => setLocalRefresh(v => v + 1)}
            title="刷新"
            aria-label="刷新"
          >
            <Icon name="reset" size={14} />
          </button>
          {onOpenFullGit && repoPath && (
            <button
              type="button"
              className="turn-diff-tool-btn"
              onClick={() => onOpenFullGit(repoPath)}
              title="在完整 Git 审阅面板中打开"
            >
              <Icon name="external" size={13} />
              Git
            </button>
          )}
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="关闭代码面板"
            title="关闭代码面板"
          >
            <Icon name="close" size={16} />
          </button>
        </div>
      </header>

      {!isPreviewMode && (
        <div className="turn-diff-file-strip" role="list" aria-label="变更文件列表">
          {workingLoading && isWorkingMode ? (
            <div className="turn-diff-strip-empty">正在读取工作区变更…</div>
          ) : files.length === 0 ? (
            <div className="turn-diff-strip-empty">暂无文件变更</div>
          ) : (
            files.map(file => {
              const { dir, name } = splitPath(file.path);
              const selected = file.path === effectiveFile;
              return (
                <button
                  key={file.path}
                  type="button"
                  role="listitem"
                  className={`turn-diff-strip-item ${selected ? "selected" : ""}`}
                  onClick={() => onSelectFile(file.path)}
                  title={file.path}
                >
                  <span className={`git-status-badge ${statusBadgeClass(file.status)}`}>
                    {file.status || "M"}
                  </span>
                  <span className="turn-diff-strip-path">
                    {dir && <span className="turn-diff-strip-dir">{dir}</span>}
                    <span className="turn-diff-strip-name">{name}</span>
                  </span>
                  <span className="turn-diff-strip-stats">
                    {file.additions > 0 && <span className="diff-add">+{file.additions}</span>}
                    {file.deletions > 0 && <span className="diff-del">-{file.deletions}</span>}
                  </span>
                </button>
              );
            })
          )}
        </div>
      )}

      {!isPreviewMode && diffError && (
        <div className="turn-diff-error" role="alert">
          <span>{diffError}</span>
        </div>
      )}

      <div className="turn-diff-viewer-wrap">
        {isPreviewMode ? (
          <FileCodePreview
            file={previewData}
            loading={previewLoading}
            error={previewError}
            targetLine={previewFile?.line}
            wrap={wrapLines}
            mode={effectivePreviewMode}
            format={previewFormat}
          />
        ) : !isWorkingMode && activeTurn && !activeTurn.summary.from_git ? (
          effectiveFileObj?.inline_diff ? (
            <InlinePatchView
              filePath={effectiveFileObj.path}
              patch={effectiveFileObj.inline_diff}
              diffStyle={diffStyle}
              wrap={wrapLines}
            />
          ) : (
            <div className="git-diff-empty">
              <strong>暂无可预览的文本差异</strong>
              <p>该记录来自历史工具日志，未包含完整文件快照。</p>
            </div>
          )
        ) : (
          <DiffViewer
            diff={diffData}
            loading={diffLoading || (isWorkingMode && workingLoading)}
            diffStyle={diffStyle}
            wrap={wrapLines}
          />
        )}
      </div>
    </aside>
  );
}
