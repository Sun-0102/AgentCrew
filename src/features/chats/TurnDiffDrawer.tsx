import { useCallback, useEffect, useMemo, useState } from "react";
import Icon from "../../components/Icon";
import DiffViewer from "../git/DiffViewer";
import {
  gitFileDiff,
  gitOverview,
  gitTurnFileDiff,
  type GitFileDiff,
  type GitOverview,
  type TurnDiffFile,
  type TurnDiffSummary,
} from "../git/api";
import { desktop } from "./api";
import "./turn-diff.css";

export interface TurnDiffOption {
  id: string;
  label: string;
  summary: TurnDiffSummary;
}

interface Props {
  turns: TurnDiffOption[];
  selectedTurnId: string;
  selectedFile: string | null;
  workspacePath?: string;
  refreshToken?: number;
  onSelectTurn: (turnId: string) => void;
  onSelectFile: (filePath: string | null) => void;
  onOpenFullGit?: (path?: string) => void;
  onClose: () => void;
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

function InlinePatchView({ filePath, patch }: { filePath: string; patch: string }) {
  const lines = patch.split("\n");
  return (
    <div className="turn-diff-inline-patch">
      <div className="turn-diff-inline-header">{filePath}</div>
      <pre className="turn-diff-inline-code">
        {lines.map((line, i) => {
          const cls =
            line.startsWith("+") && !line.startsWith("+++")
              ? "is-add"
              : line.startsWith("-") && !line.startsWith("---")
                ? "is-del"
                : line.startsWith("@@")
                  ? "is-hunk"
                  : "";
          return (
            <span key={i} className={`turn-diff-patch-line ${cls}`}>
              {line}
              {"\n"}
            </span>
          );
        })}
      </pre>
    </div>
  );
}

export default function TurnDiffDrawer({
  turns,
  selectedTurnId,
  selectedFile,
  workspacePath,
  refreshToken = 0,
  onSelectTurn,
  onSelectFile,
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

  const isWorkingMode = selectedTurnId === "__working__";
  const activeTurn = useMemo(
    () => turns.find(t => t.id === selectedTurnId) ?? turns.at(-1) ?? null,
    [turns, selectedTurnId],
  );
  const repoPath = activeTurn?.summary.repo_root || workspacePath || "";

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
    if (isWorkingMode) {
      void loadWorkingTree();
    }
  }, [isWorkingMode, loadWorkingTree, refreshToken, localRefresh]);

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
    if (!effectiveFile) {
      setDiffData(null);
      setDiffError(null);
      return;
    }
    if (!isWorkingMode && activeTurn && !activeTurn.summary.from_git) {
      // Inline fallback diff — rendered directly without calling Tauri git command.
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

  return (
    <aside className="turn-diff-drawer" aria-label="代码变更对比">
      <header className="turn-diff-drawer-header">
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

        <div className="turn-diff-drawer-tools">
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
          <button
            type="button"
            className={`turn-diff-tool-btn ${wrapLines ? "active" : ""}`}
            onClick={() => setWrapLines(w => !w)}
            title="切换自动换行"
          >
            换行
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => setLocalRefresh(v => v + 1)}
            title="刷新变更"
            aria-label="刷新变更"
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
            aria-label="关闭变更面板"
            title="关闭变更面板"
          >
            <Icon name="close" size={16} />
          </button>
        </div>
      </header>

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

      {diffError && (
        <div className="turn-diff-error" role="alert">
          <span>{diffError}</span>
        </div>
      )}

      <div className="turn-diff-viewer-wrap">
        {!isWorkingMode && activeTurn && !activeTurn.summary.from_git ? (
          effectiveFileObj?.inline_diff ? (
            <InlinePatchView
              filePath={effectiveFileObj.path}
              patch={effectiveFileObj.inline_diff}
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
