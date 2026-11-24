import { invoke } from "@tauri-apps/api/core";

export interface GitFileChange {
  path: string;
  old_path: string | null;
  staged_status: string;
  unstaged_status: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
  additions: number;
  deletions: number;
}

export interface GitWorktreeInfo {
  path: string;
  head: string;
  branch: string;
  is_main: boolean;
  is_current: boolean;
  detached: boolean;
}

export interface GitCommitInfo {
  hash: string;
  short_hash: string;
  subject: string;
  author: string;
  relative_time: string;
}

export interface GitCommitFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

export interface GitOverview {
  is_repo: boolean;
  requested_path: string;
  repo_root: string;
  branch: string;
  head_short: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  gh_available: boolean;
  remote_url: string | null;
  branches: string[];
  files: GitFileChange[];
  worktrees: GitWorktreeInfo[];
  total_commits: number;
  recent_commits: GitCommitInfo[];
}

export interface GitFileDiff {
  file_path: string;
  old_contents: string;
  new_contents: string;
  patch: string;
  is_binary: boolean;
  truncated: boolean;
  additions: number;
  deletions: number;
}

export interface GitActionResult {
  ok: boolean;
  message: string;
  url: string | null;
}

export type DiffMode = "all" | "working" | "staged";

export const gitOverview = (path: string) =>
  invoke<GitOverview>("git_overview", { path });

export const gitFileDiff = (path: string, filePath: string, mode: DiffMode = "all") =>
  invoke<GitFileDiff>("git_file_diff", { path, filePath, mode });

export const gitStage = (path: string, files: string[], all = false) =>
  invoke<GitActionResult>("git_stage", { path, files, all });

export const gitUnstage = (path: string, files: string[], all = false) =>
  invoke<GitActionResult>("git_unstage", { path, files, all });

export const gitDiscard = (path: string, files: string[], untracked: string[]) =>
  invoke<GitActionResult>("git_discard", { path, files, untracked });

export const gitCommit = (path: string, message: string, stageAll = false) =>
  invoke<GitActionResult>("git_commit", { path, message, stageAll });

export const gitPush = (path: string, setUpstream = true) =>
  invoke<GitActionResult>("git_push", { path, setUpstream });

export const gitCheckoutBranch = (path: string, branch: string) =>
  invoke<GitActionResult>("git_checkout_branch", { path, branch });

export const gitCreatePr = (path: string, title: string, body: string, draft = false) =>
  invoke<GitActionResult>("git_create_pr", { path, title, body, draft });

export const gitWorktreeCreate = (path: string, branchName: string, baseBranch?: string) =>
  invoke<GitWorktreeInfo>("git_worktree_create", {
    path,
    branchName,
    baseBranch: baseBranch || null,
  });

export const gitWorktreeRemove = (path: string, worktreePath: string, force = false) =>
  invoke<GitActionResult>("git_worktree_remove", { path, worktreePath, force });

export const gitCommits = (path: string, skip = 0, limit = 50, query?: string) =>
  invoke<GitCommitInfo[]>("git_commits", {
    path,
    skip,
    limit,
    query: query || null,
  });

export const gitCommitFiles = (path: string, hash: string) =>
  invoke<GitCommitFile[]>("git_commit_files", { path, hash });

export const gitCommitFileDiff = (path: string, hash: string, filePath: string) =>
  invoke<GitFileDiff>("git_commit_file_diff", { path, hash, filePath });

export const gitOpenUrl = (url: string) =>
  invoke<void>("git_open_url", { url });
