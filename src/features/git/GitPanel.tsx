import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import Icon from "../../components/Icon";
import Dialog from "../../components/Dialog";
import DiffViewer from "./DiffViewer";
import {
  gitCheckoutBranch,
  gitCommit,
  gitCommitFileDiff,
  gitCommitFiles,
  gitCommits,
  gitCreatePr,
  gitDiscard,
  gitFileDiff,
  gitGenerateCommitMessage,
  gitOpenUrl,
  gitOverview,
  gitPush,
  gitStage,
  gitUnstage,
  gitWorktreeCreate,
  gitWorktreeRemove,
  type DiffMode,
  type GitCommitFile,
  type GitCommitInfo,
  type GitFileChange,
  type GitFileDiff,
  type GitOverview,
} from "./api";
import type { Project } from "../projects/api";
import { desktop } from "../chats/api";
import "./git-panel.css";

interface Props {
  projects: Project[];
  initialPath?: string;
  onAddProjectPath?: (path: string) => Promise<Project | null>;
  onNotify: (message: string) => void;
  onClose?: () => void;
}

type SubTab = "changes" | "worktrees" | "history";

function splitFilePath(fullPath: string): { name: string; dir: string } {
  const idx = fullPath.lastIndexOf("/");
  if (idx === -1) return { name: fullPath, dir: "" };
  return { name: fullPath.slice(idx + 1), dir: fullPath.slice(0, idx) };
}

function statusClass(code: string): string {
  const c = code.trim().toUpperCase();
  if (c === "A") return "is-a";
  if (c === "?") return "is-q";
  if (c === "D") return "is-d";
  if (c === "R" || c === "C") return "is-r";
  return "is-m";
}

export default function GitPanel({
  projects,
  initialPath,
  onAddProjectPath,
  onNotify,
  onClose,
}: Props) {
  const [selectedPath, setSelectedPath] = useState<string>(
    () => initialPath || projects[0]?.primary_root || "",
  );
  const [overview, setOverview] = useState<GitOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [subTab, setSubTab] = useState<SubTab>("changes");

  const [fileQuery, setFileQuery] = useState("");
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [diffMode, setDiffMode] = useState<DiffMode>("all");
  const [diffStyle, setDiffStyle] = useState<"split" | "unified">("split");
  const [wrapLines, setWrapLines] = useState(false);
  const [diffData, setDiffData] = useState<GitFileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);

  const [commitMessage, setCommitMessage] = useState("");
  const [generatingCommit, setGeneratingCommit] = useState(false);
  const [acting, setActing] = useState(false);
  const [discardTarget, setDiscardTarget] = useState<GitFileChange | null>(null);

  const [prOpen, setPrOpen] = useState(false);
  const [prTitle, setPrTitle] = useState("");
  const [prBody, setPrBody] = useState("");
  const [prDraft, setPrDraft] = useState(false);
  const [lastPrUrl, setLastPrUrl] = useState<string | null>(null);

  const [wtBranch, setWtBranch] = useState("");
  const [wtBase, setWtBase] = useState("");
  const [wtCreating, setWtCreating] = useState(false);

  const [commits, setCommits] = useState<GitCommitInfo[]>([]);
  const [commitQuery, setCommitQuery] = useState("");
  const [commitsLoading, setCommitsLoading] = useState(false);
  const [selectedCommit, setSelectedCommit] = useState<GitCommitInfo | null>(null);
  const [commitFiles, setCommitFiles] = useState<GitCommitFile[]>([]);
  const [commitFilesLoading, setCommitFilesLoading] = useState(false);
  const [selectedCommitFile, setSelectedCommitFile] = useState<string | null>(null);
  const [commitDiffData, setCommitDiffData] = useState<GitFileDiff | null>(null);
  const [commitDiffLoading, setCommitDiffLoading] = useState(false);

  useEffect(() => {
    if (initialPath) {
      setSelectedPath(initialPath);
    } else if (!selectedPath && projects[0]?.primary_root) {
      setSelectedPath(projects[0].primary_root);
    }
  }, [initialPath, projects]);

  const loadOverview = useCallback(
    async (targetPath = selectedPath, silent = false) => {
      if (!desktop || !targetPath) {
        setOverview(null);
        return;
      }
      if (!silent) setLoading(true);
      setError(null);
      try {
        const data = await gitOverview(targetPath);
        setOverview(data);
        setCommits(data.recent_commits);
        setSelectedCommit(prev => {
          if (prev && data.recent_commits.some(c => c.hash === prev.hash)) return prev;
          return data.recent_commits[0] ?? null;
        });
        setSelectedFile(prev => {
          if (prev && data.files.some(f => f.path === prev)) return prev;
          return data.files[0]?.path ?? null;
        });
      } catch (cause) {
        setError(String(cause));
        setOverview(null);
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [selectedPath],
  );

  useEffect(() => {
    void loadOverview(selectedPath);
  }, [selectedPath, loadOverview]);

  useEffect(() => {
    if (!desktop || !overview?.is_repo || !selectedFile) {
      setDiffData(null);
      return;
    }
    let active = true;
    setDiffLoading(true);
    void gitFileDiff(overview.repo_root, selectedFile, diffMode)
      .then(result => {
        if (active) setDiffData(result);
      })
      .catch(cause => {
        if (active) {
          setError(String(cause));
          setDiffData(null);
        }
      })
      .finally(() => {
        if (active) setDiffLoading(false);
      });
    return () => {
      active = false;
    };
  }, [overview?.repo_root, overview?.is_repo, overview?.files, selectedFile, diffMode]);

  useEffect(() => {
    if (!desktop || !overview?.is_repo || subTab !== "history" || !selectedCommit) {
      return;
    }
    let active = true;
    setCommitFilesLoading(true);
    setCommitFiles([]);
    setSelectedCommitFile(null);
    setCommitDiffData(null);
    void gitCommitFiles(overview.repo_root, selectedCommit.hash)
      .then(list => {
        if (!active) return;
        setCommitFiles(list);
        setSelectedCommitFile(list[0]?.path ?? null);
      })
      .catch(cause => {
        if (active) setError(String(cause));
      })
      .finally(() => {
        if (active) setCommitFilesLoading(false);
      });
    return () => {
      active = false;
    };
  }, [overview?.repo_root, overview?.is_repo, subTab, selectedCommit?.hash]);

  useEffect(() => {
    if (
      !desktop ||
      !overview?.is_repo ||
      subTab !== "history" ||
      !selectedCommit ||
      !selectedCommitFile
    ) {
      setCommitDiffData(null);
      return;
    }
    let active = true;
    setCommitDiffLoading(true);
    void gitCommitFileDiff(overview.repo_root, selectedCommit.hash, selectedCommitFile)
      .then(result => {
        if (active) setCommitDiffData(result);
      })
      .catch(cause => {
        if (active) {
          setError(String(cause));
          setCommitDiffData(null);
        }
      })
      .finally(() => {
        if (active) setCommitDiffLoading(false);
      });
    return () => {
      active = false;
    };
  }, [overview?.repo_root, overview?.is_repo, subTab, selectedCommit?.hash, selectedCommitFile]);

  const filteredCommits = useMemo(() => {
    const q = commitQuery.trim().toLowerCase();
    if (!q) return commits;
    return commits.filter(
      c =>
        c.subject.toLowerCase().includes(q) ||
        c.short_hash.toLowerCase().includes(q) ||
        c.author.toLowerCase().includes(q),
    );
  }, [commits, commitQuery]);

  async function loadMoreCommits(loadAll = false) {
    if (!overview?.repo_root || commitsLoading) return;
    setCommitsLoading(true);
    setError(null);
    try {
      const more = await gitCommits(
        overview.repo_root,
        commits.length,
        loadAll ? 0 : 50,
      );
      if (more.length > 0) {
        setCommits(prev => {
          const seen = new Set(prev.map(c => c.hash));
          const merged = [...prev];
          for (const item of more) {
            if (!seen.has(item.hash)) {
              seen.add(item.hash);
              merged.push(item);
            }
          }
          return merged;
        });
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setCommitsLoading(false);
    }
  }

  const filteredFiles = useMemo(() => {
    const files = overview?.files ?? [];
    const q = fileQuery.trim().toLowerCase();
    if (!q) return files;
    return files.filter(f => f.path.toLowerCase().includes(q));
  }, [overview?.files, fileQuery]);

  const stagedFiles = useMemo(
    () => filteredFiles.filter(f => f.staged),
    [filteredFiles],
  );
  const unstagedFiles = useMemo(
    () => filteredFiles.filter(f => f.unstaged),
    [filteredFiles],
  );

  const totalStaged = (overview?.files ?? []).filter(f => f.staged).length;
  const totalUnstaged = (overview?.files ?? []).filter(f => f.unstaged).length;
  const totalChanged = overview?.files.length ?? 0;

  async function pickFolder() {
    if (!desktop) return;
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择 Git 仓库目录",
      });
      if (typeof picked !== "string") return;
      if (onAddProjectPath) {
        await onAddProjectPath(picked);
      }
      setSelectedPath(picked);
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function runAction(fn: () => Promise<string | void>) {
    setActing(true);
    setError(null);
    try {
      const msg = await fn();
      await loadOverview(selectedPath, true);
      if (msg) onNotify(msg);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setActing(false);
    }
  }

  async function handleGenerateCommit() {
    if (!overview?.repo_root || generatingCommit) return;
    setGeneratingCommit(true);
    setError(null);
    try {
      const res = await gitGenerateCommitMessage(overview.repo_root);
      setCommitMessage(res.message);
      onNotify(res.url ? `已由 ${res.url} 生成提交说明` : "已生成提交说明");
    } catch (cause) {
      setError(String(cause));
    } finally {
      setGeneratingCommit(false);
    }
  }

  async function handleCommit(stageAll: boolean) {
    if (!overview?.repo_root) return;
    if (!commitMessage.trim()) {
      setError("请先填写 Commit Message");
      return;
    }
    await runAction(async () => {
      const res = await gitCommit(overview.repo_root, commitMessage.trim(), stageAll);
      setCommitMessage("");
      return res.message;
    });
  }

  async function handlePush() {
    if (!overview?.repo_root) return;
    await runAction(async () => {
      const res = await gitPush(overview.repo_root, true);
      return res.message;
    });
  }

  async function handleCreatePr() {
    if (!overview?.repo_root) return;
    await runAction(async () => {
      const res = await gitCreatePr(
        overview.repo_root,
        prTitle.trim() || overview.recent_commits[0]?.subject || overview.branch,
        prBody,
        prDraft,
      );
      setPrOpen(false);
      if (res.url) {
        setLastPrUrl(res.url);
        await gitOpenUrl(res.url).catch(() => {});
      }
      return res.message;
    });
  }

  async function handleCreateWorktree() {
    if (!overview?.repo_root || !wtBranch.trim()) return;
    setWtCreating(true);
    setError(null);
    try {
      const created = await gitWorktreeCreate(
        overview.repo_root,
        wtBranch.trim(),
        wtBase || undefined,
      );
      setWtBranch("");
      setWtBase("");
      await loadOverview(selectedPath, true);
      onNotify(`已创建隔离 Worktree（${created.branch}）`);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setWtCreating(false);
    }
  }

  const repoOptions = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of projects) {
      map.set(p.primary_root, p.name);
    }
    if (selectedPath && !map.has(selectedPath)) {
      const name = selectedPath.split(/[\\/]/).filter(Boolean).pop() ?? selectedPath;
      map.set(selectedPath, name);
    }
    for (const wt of overview?.worktrees ?? []) {
      if (!map.has(wt.path)) {
        map.set(wt.path, `${wt.branch} (worktree)`);
      }
    }
    return [...map.entries()];
  }, [projects, selectedPath, overview?.worktrees]);

  return (
    <section className="git-panel" aria-label="Git 与代码审阅">
      <header className="git-panel-header">
        <div className="git-header-left">
          <div className="git-repo-picker">
            <Icon name="folder" size={15} />
            <select
              aria-label="选择仓库目录"
              value={selectedPath}
              onChange={e => setSelectedPath(e.target.value)}
            >
              {!repoOptions.length && <option value="">未选择本地项目</option>}
              {repoOptions.map(([path, label]) => (
                <option key={path} value={path}>
                  {label}
                </option>
              ))}
            </select>
          </div>
          <button
            type="button"
            className="git-header-btn"
            onClick={() => void pickFolder()}
            title="选择本地目录"
          >
            <Icon name="plus" size={14} />
            选择目录
          </button>

          {overview?.is_repo && (
            <>
              <div className="git-branch-pill" title="切换本地分支">
                <Icon name="gitBranch" size={13} />
                <select
                  aria-label="当前分支"
                  value={overview.branch}
                  disabled={acting}
                  onChange={e => {
                    const target = e.target.value;
                    if (target && target !== overview.branch) {
                      void runAction(async () => {
                        const res = await gitCheckoutBranch(overview.repo_root, target);
                        return res.message;
                      });
                    }
                  }}
                >
                  {!overview.branches.includes(overview.branch) && (
                    <option value={overview.branch}>{overview.branch}</option>
                  )}
                  {overview.branches.map(b => (
                    <option key={b} value={b}>
                      {b}
                    </option>
                  ))}
                </select>
              </div>
              <span className="git-sync-badge" title={overview.upstream ?? "尚未设置远程上游分支"}>
                <span>{overview.head_short || "init"}</span>
                {overview.upstream && (
                  <>
                    <span>↑{overview.ahead}</span>
                    <span>↓{overview.behind}</span>
                  </>
                )}
              </span>
            </>
          )}
        </div>

        <div className="git-header-right">
          {(subTab === "changes" || subTab === "history") && overview?.is_repo && (
            <>
              <div className="git-segmented" role="group" aria-label="Diff 视图模式">
                <button
                  type="button"
                  className={diffStyle === "split" ? "active" : ""}
                  onClick={() => setDiffStyle("split")}
                >
                  Split
                </button>
                <button
                  type="button"
                  className={diffStyle === "unified" ? "active" : ""}
                  onClick={() => setDiffStyle("unified")}
                >
                  Unified
                </button>
              </div>
              <button
                type="button"
                className={`git-header-btn ${wrapLines ? "active" : ""}`}
                onClick={() => setWrapLines(w => !w)}
              >
                换行
              </button>
            </>
          )}
          <button
            type="button"
            className="git-header-btn"
            disabled={loading || acting || !selectedPath}
            onClick={() => void loadOverview(selectedPath)}
            title="刷新仓库状态"
          >
            <Icon name="reset" size={14} />
            刷新
          </button>
          {onClose && (
            <button
              type="button"
              className="icon-button"
              aria-label="关闭代码审阅"
              onClick={onClose}
            >
              <Icon name="close" size={17} />
            </button>
          )}
        </div>
      </header>

      {overview?.is_repo && (
        <div className="git-tabs-bar">
          <div className="git-tabs" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "changes"}
              className={subTab === "changes" ? "active" : ""}
              onClick={() => setSubTab("changes")}
            >
              变更审阅
              <span className="git-tab-count">{totalChanged}</span>
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "worktrees"}
              className={subTab === "worktrees" ? "active" : ""}
              onClick={() => setSubTab("worktrees")}
            >
              Worktree 隔离
              <span className="git-tab-count">{overview.worktrees.length}</span>
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "history"}
              className={subTab === "history" ? "active" : ""}
              onClick={() => setSubTab("history")}
            >
              提交历史
              <span className="git-tab-count">{overview.total_commits}</span>
            </button>
          </div>

          {lastPrUrl && (
            <button
              type="button"
              className="git-header-btn active"
              onClick={() => void gitOpenUrl(lastPrUrl)}
            >
              <Icon name="external" size={13} />
              打开最近 PR
            </button>
          )}
        </div>
      )}

      {error && (
        <div className="git-error-bar" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭提示"
            onClick={() => setError(null)}
          >
            <Icon name="close" size={15} />
          </button>
        </div>
      )}

      {!desktop ? (
        <div className="git-diff-empty">
          <strong>请在桌面版中使用 Git 与代码审阅</strong>
          <p>浏览器预览模式无法直接读取本地 Git 仓库与 Worktree。</p>
        </div>
      ) : !selectedPath ? (
        <div className="git-diff-empty">
          <strong>还没有选择本地项目</strong>
          <p>点击上方「选择目录」打开任意 Git 仓库，即可可视化审阅代码 Diff、一键提交与管理 Worktree。</p>
          <button type="button" className="primary" onClick={() => void pickFolder()}>
            选择本地 Git 仓库
          </button>
        </div>
      ) : loading && !overview ? (
        <div className="git-diff-empty">
          <span className="small-spinner" />
          <span>正在读取仓库状态…</span>
        </div>
      ) : overview && !overview.is_repo ? (
        <div className="git-diff-empty">
          <strong>当前目录不是 Git 仓库</strong>
          <p>{selectedPath}</p>
          <button type="button" className="outline-button" onClick={() => void pickFolder()}>
            切换到其他目录
          </button>
        </div>
      ) : overview && subTab === "changes" ? (
        <div className="git-changes-body">
          <aside className="git-files-sidebar">
            <div className="git-files-toolbar">
              <input
                type="search"
                className="git-files-search"
                placeholder="筛选变更文件…"
                value={fileQuery}
                onChange={e => setFileQuery(e.target.value)}
              />
            </div>

            <div className="git-files-scroll">
              {totalChanged === 0 ? (
                <div className="git-clean-state">
                  工作区干净，暂无未提交变更。
                </div>
              ) : (
                <>
                  {stagedFiles.length > 0 && (
                    <div className="git-file-group">
                      <div className="git-file-group-header">
                        <span>已暂存 ({stagedFiles.length})</span>
                        <div className="git-file-group-actions">
                          <button
                            type="button"
                            className="git-mini-btn"
                            disabled={acting}
                            onClick={() =>
                              void runAction(async () => {
                                const res = await gitUnstage(overview.repo_root, [], true);
                                return res.message;
                              })
                            }
                          >
                            全部取消
                          </button>
                        </div>
                      </div>
                      {stagedFiles.map(file => {
                        const { name, dir } = splitFilePath(file.path);
                        const active = selectedFile === file.path && diffMode === "staged";
                        return (
                          <div
                            key={`staged:${file.path}`}
                            className={`git-file-row ${active || selectedFile === file.path ? "selected" : ""}`}
                            onClick={() => {
                              setSelectedFile(file.path);
                              setDiffMode("staged");
                            }}
                          >
                            <span className={`git-status-badge ${statusClass(file.staged_status)}`}>
                              {file.staged_status}
                            </span>
                            <div className="git-file-info">
                              <span className="git-file-name" title={file.path}>
                                {name}
                              </span>
                              {dir && <span className="git-file-dir">{dir}</span>}
                            </div>
                            <span className="git-file-stats">
                              {file.additions > 0 && <span className="git-stat-add">+{file.additions}</span>}
                              {file.deletions > 0 && <span className="git-stat-del">-{file.deletions}</span>}
                            </span>
                            <div
                              className="git-file-row-actions"
                              onClick={e => e.stopPropagation()}
                            >
                              <button
                                type="button"
                                className="git-mini-btn"
                                disabled={acting}
                                title="取消暂存"
                                onClick={() =>
                                  void runAction(async () => {
                                    await gitUnstage(overview.repo_root, [file.path], false);
                                  })
                                }
                              >
                                -
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}

                  {unstagedFiles.length > 0 && (
                    <div className="git-file-group">
                      <div className="git-file-group-header">
                        <span>未暂存变更 ({unstagedFiles.length})</span>
                        <div className="git-file-group-actions">
                          <button
                            type="button"
                            className="git-mini-btn"
                            disabled={acting}
                            onClick={() =>
                              void runAction(async () => {
                                const res = await gitStage(overview.repo_root, [], true);
                                return res.message;
                              })
                            }
                          >
                            全部暂存
                          </button>
                        </div>
                      </div>
                      {unstagedFiles.map(file => {
                        const { name, dir } = splitFilePath(file.path);
                        return (
                          <div
                            key={`unstaged:${file.path}`}
                            className={`git-file-row ${selectedFile === file.path ? "selected" : ""}`}
                            onClick={() => {
                              setSelectedFile(file.path);
                              setDiffMode("all");
                            }}
                          >
                            <span className={`git-status-badge ${statusClass(file.unstaged_status)}`}>
                              {file.unstaged_status}
                            </span>
                            <div className="git-file-info">
                              <span className="git-file-name" title={file.path}>
                                {name}
                              </span>
                              {dir && <span className="git-file-dir">{dir}</span>}
                            </div>
                            <span className="git-file-stats">
                              {file.additions > 0 && <span className="git-stat-add">+{file.additions}</span>}
                              {file.deletions > 0 && <span className="git-stat-del">-{file.deletions}</span>}
                            </span>
                            <div
                              className="git-file-row-actions"
                              onClick={e => e.stopPropagation()}
                            >
                              <button
                                type="button"
                                className="git-mini-btn"
                                disabled={acting}
                                title="暂存此文件"
                                onClick={() =>
                                  void runAction(async () => {
                                    await gitStage(overview.repo_root, [file.path], false);
                                  })
                                }
                              >
                                +
                              </button>
                              <button
                                type="button"
                                className="git-mini-btn danger"
                                disabled={acting}
                                title="放弃此文件修改"
                                onClick={() => setDiscardTarget(file)}
                              >
                                撤
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </>
              )}
            </div>

            <div className="git-commit-box">
              <div className="git-commit-label-row">
                <span>提交说明</span>
                <button
                  type="button"
                  className="git-mini-btn"
                  disabled={generatingCommit || acting || totalChanged === 0}
                  onClick={() => void handleGenerateCommit()}
                  title="根据当前变更自动生成 Commit Message"
                >
                  <Icon name="spark" size={12} />
                  {generatingCommit ? "生成中…" : "AI 生成"}
                </button>
              </div>
              <textarea
                rows={3}
                placeholder="输入提交说明（Commit Message）…"
                value={commitMessage}
                onChange={e => setCommitMessage(e.target.value)}
                onKeyDown={e => {
                  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                    e.preventDefault();
                    void handleCommit(totalStaged === 0);
                  }
                }}
              />
              <div className="git-commit-actions">
                <button
                  type="button"
                  className="primary"
                  disabled={acting || !commitMessage.trim() || totalChanged === 0}
                  onClick={() => void handleCommit(totalStaged === 0)}
                >
                  {totalStaged > 0
                    ? `提交已暂存 (${totalStaged})`
                    : `全部暂存并提交 (${totalUnstaged})`}
                </button>
              </div>
              <div className="git-commit-secondary">
                <button
                  type="button"
                  className="outline-button"
                  disabled={acting}
                  onClick={() => void handlePush()}
                >
                  Push{overview.ahead > 0 ? ` (${overview.ahead})` : ""}
                </button>
                <button
                  type="button"
                  className="outline-button"
                  disabled={acting}
                  onClick={() => {
                    setPrTitle(overview.recent_commits[0]?.subject ?? overview.branch);
                    setPrOpen(true);
                  }}
                >
                  {overview.gh_available ? "创建 PR (gh)" : "打开 PR 链接"}
                </button>
              </div>
            </div>
          </aside>

          <div className="git-diff-pane">
            {selectedFile && (
              <div className="git-diff-pane-bar">
                <span className="git-diff-filepath" title={selectedFile}>
                  {selectedFile}
                </span>
                <div className="git-segmented" role="group" aria-label="对比基线">
                  <button
                    type="button"
                    className={diffMode === "all" ? "active" : ""}
                    onClick={() => setDiffMode("all")}
                  >
                    全部 (HEAD)
                  </button>
                  <button
                    type="button"
                    className={diffMode === "working" ? "active" : ""}
                    onClick={() => setDiffMode("working")}
                  >
                    未暂存
                  </button>
                  <button
                    type="button"
                    className={diffMode === "staged" ? "active" : ""}
                    onClick={() => setDiffMode("staged")}
                  >
                    已暂存
                  </button>
                </div>
              </div>
            )}
            <DiffViewer
              diff={diffData}
              loading={diffLoading}
              diffStyle={diffStyle}
              wrap={wrapLines}
            />
          </div>
        </div>
      ) : overview && subTab === "worktrees" ? (
        <div className="git-subpage">
          <div className="git-card">
            <h3>新建隔离 Worktree</h3>
            <p>
              参考 T3 Code 的隔离机制，新建的 Worktree 会统一存放在应用数据目录的
              <code> worktrees/ </code>
              下，不会污染当前项目目录。创建后可一键加入工作空间，让不同会话或 Agent 并行改代码互不干扰。
            </p>
            <div className="git-wt-form">
              <input
                type="text"
                placeholder="新分支名，例如 feat/auth-refactor"
                value={wtBranch}
                onChange={e => setWtBranch(e.target.value)}
                onKeyDown={e => {
                  if (e.key === "Enter") void handleCreateWorktree();
                }}
              />
              <select
                aria-label="基于分支"
                value={wtBase}
                onChange={e => setWtBase(e.target.value)}
              >
                <option value="">基于当前 HEAD ({overview.branch})</option>
                {overview.branches.map(b => (
                  <option key={b} value={b}>
                    基于 {b}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="primary"
                disabled={wtCreating || !wtBranch.trim()}
                onClick={() => void handleCreateWorktree()}
              >
                {wtCreating ? "创建中…" : "创建 Worktree"}
              </button>
            </div>
          </div>

          <div className="git-card">
            <h3>当前仓库的 Worktree 列表 ({overview.worktrees.length})</h3>
            <div className="git-wt-list">
              {overview.worktrees.map(wt => (
                <div
                  key={wt.path}
                  className={`git-wt-item ${wt.is_current ? "is-current" : ""}`}
                >
                  <div className="git-wt-meta">
                    <strong>
                      <Icon name="gitBranch" size={14} />
                      {wt.branch}
                      {wt.is_main && <span className="git-badge">主工作区</span>}
                      {wt.is_current && <span className="git-badge accent">当前查看</span>}
                      <span className="git-badge">{wt.head}</span>
                    </strong>
                    <code title={wt.path}>{wt.path}</code>
                  </div>
                  <div className="git-wt-actions">
                    {!wt.is_current && (
                      <button
                        type="button"
                        className="outline-button"
                        onClick={async () => {
                          if (onAddProjectPath) {
                            await onAddProjectPath(wt.path);
                          }
                          setSelectedPath(wt.path);
                          setSubTab("changes");
                        }}
                      >
                        切换查看
                      </button>
                    )}
                    {!wt.is_main && (
                      <button
                        type="button"
                        className="outline-button"
                        disabled={acting}
                        onClick={() =>
                          void runAction(async () => {
                            const res = await gitWorktreeRemove(
                              overview.repo_root,
                              wt.path,
                              false,
                            );
                            return res.message;
                          })
                        }
                      >
                        移除
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      ) : overview && subTab === "history" ? (
        <div className="git-history-body">
          <aside className="git-files-sidebar">
            <div className="git-files-toolbar">
              <input
                type="search"
                className="git-files-search"
                placeholder="筛选提交信息、作者或哈希…"
                value={commitQuery}
                onChange={e => setCommitQuery(e.target.value)}
              />
            </div>
            <div className="git-files-scroll">
              {filteredCommits.length === 0 ? (
                <div className="git-clean-state">
                  {commits.length === 0 ? "当前仓库尚无提交记录。" : "没有匹配的提交记录。"}
                </div>
              ) : (
                filteredCommits.map(c => (
                  <button
                    type="button"
                    key={c.hash}
                    className={`git-history-row ${selectedCommit?.hash === c.hash ? "selected" : ""}`}
                    onClick={() => setSelectedCommit(c)}
                  >
                    <div className="git-history-row-top">
                      <span className="git-commit-hash">{c.short_hash}</span>
                      <span className="git-commit-meta">
                        {c.author} · {c.relative_time}
                      </span>
                    </div>
                    <span className="git-history-row-subject">{c.subject}</span>
                  </button>
                ))
              )}
            </div>
            <div className="git-history-footer">
              <div className="git-history-summary">
                已加载 {commits.length} / 共 {overview.total_commits} 条提交
              </div>
              {commits.length < overview.total_commits && (
                <div className="git-history-load-actions">
                  <button
                    type="button"
                    className="outline-button"
                    disabled={commitsLoading}
                    onClick={() => void loadMoreCommits(false)}
                  >
                    {commitsLoading ? "加载中…" : "加载更多 (+50)"}
                  </button>
                  <button
                    type="button"
                    className="outline-button"
                    disabled={commitsLoading}
                    onClick={() => void loadMoreCommits(true)}
                  >
                    加载全部
                  </button>
                </div>
              )}
            </div>
          </aside>

          <div className="git-diff-pane">
            {selectedCommit ? (
              <>
                <div className="git-commit-detail-header">
                  <div className="git-commit-detail-title">
                    <span className="git-commit-hash">{selectedCommit.short_hash}</span>
                    <strong>{selectedCommit.subject}</strong>
                  </div>
                  <div className="git-commit-detail-sub">
                    {selectedCommit.author} · {selectedCommit.relative_time} ·{" "}
                    {commitFilesLoading ? "正在读取变更文件…" : `共修改 ${commitFiles.length} 个文件`}
                  </div>
                </div>
                {commitFiles.length > 0 && (
                  <div className="git-commit-files-strip">
                    {commitFiles.map(file => {
                      const { name } = splitFilePath(file.path);
                      return (
                        <button
                          type="button"
                          key={file.path}
                          title={file.path}
                          className={`git-commit-file-pill ${selectedCommitFile === file.path ? "selected" : ""}`}
                          onClick={() => setSelectedCommitFile(file.path)}
                        >
                          <span className={`git-status-badge ${statusClass(file.status)}`}>
                            {file.status}
                          </span>
                          <span>{name}</span>
                          {(file.additions > 0 || file.deletions > 0) && (
                            <span className="git-file-stats">
                              {file.additions > 0 && (
                                <span className="git-stat-add">+{file.additions}</span>
                              )}
                              {file.deletions > 0 && (
                                <span className="git-stat-del">-{file.deletions}</span>
                              )}
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
                <DiffViewer
                  diff={commitDiffData}
                  loading={commitFilesLoading || commitDiffLoading}
                  diffStyle={diffStyle}
                  wrap={wrapLines}
                />
              </>
            ) : (
              <div className="git-diff-empty">
                <strong>选择左侧任意提交查看代码变更</strong>
                <p>可按文件查看该次提交的 Split / Unified 语法高亮 Diff。</p>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {discardTarget && overview && (
        <Dialog title="放弃文件修改" onClose={() => setDiscardTarget(null)}>
          <p>
            确定要放弃对 <code>{discardTarget.path}</code> 的未暂存修改吗？
            {discardTarget.untracked
              ? "该文件为未跟踪的新文件，放弃后将直接从磁盘删除，无法恢复。"
              : "该操作会将工作区文件还原到暂存区/HEAD 状态，无法恢复。"}
          </p>
          <div className="git-pr-actions">
            <button
              type="button"
              className="outline-button"
              onClick={() => setDiscardTarget(null)}
            >
              取消
            </button>
            <button
              type="button"
              className="primary"
              disabled={acting}
              onClick={() => {
                const target = discardTarget;
                setDiscardTarget(null);
                void runAction(async () => {
                  const res = await gitDiscard(
                    overview.repo_root,
                    target.untracked ? [] : [target.path],
                    target.untracked ? [target.path] : [],
                  );
                  return res.message;
                });
              }}
            >
              确认放弃
            </button>
          </div>
        </Dialog>
      )}

      {prOpen && overview && (
        <Dialog
          title={overview.gh_available ? "创建 Pull Request (gh CLI)" : "打开网页创建 PR"}
          onClose={() => setPrOpen(false)}
        >
          <div className="git-pr-form">
            <p style={{ margin: 0, fontSize: 12.5, color: "var(--muted)" }}>
              {overview.gh_available
                ? `将基于当前分支 ${overview.branch} 调用本机 gh CLI 创建 Pull Request（请确保分支已 Push）。`
                : `本机未检测到 gh CLI，将为你生成 ${overview.branch} 的网页 Compare 链接并在浏览器打开。`}
            </p>
            {overview.gh_available && (
              <>
                <label>
                  PR 标题
                  <input
                    type="text"
                    value={prTitle}
                    onChange={e => setPrTitle(e.target.value)}
                    placeholder="输入 Pull Request 标题"
                  />
                </label>
                <label>
                  PR 描述
                  <textarea
                    rows={4}
                    value={prBody}
                    onChange={e => setPrBody(e.target.value)}
                    placeholder="简述本次改动内容与验证方式…"
                  />
                </label>
                <label className="git-pr-checkbox">
                  <input
                    type="checkbox"
                    checked={prDraft}
                    onChange={e => setPrDraft(e.target.checked)}
                  />
                  创建为 Draft PR（草稿）
                </label>
              </>
            )}
            <div className="git-pr-actions">
              <button
                type="button"
                className="outline-button"
                onClick={() => setPrOpen(false)}
              >
                取消
              </button>
              <button
                type="button"
                className="primary"
                disabled={acting || (overview.gh_available && !prTitle.trim())}
                onClick={() => void handleCreatePr()}
              >
                {overview.gh_available ? "创建 PR" : "在浏览器打开"}
              </button>
            </div>
          </div>
        </Dialog>
      )}
    </section>
  );
}
