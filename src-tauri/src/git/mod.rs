use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

const MAX_FILE_BYTES: usize = 512 * 1024;
const MAX_PATCH_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub staged_status: String,
    pub unstaged_status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitWorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub is_main: bool,
    pub is_current: bool,
    pub detached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    pub author: String,
    pub relative_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOverview {
    pub is_repo: bool,
    pub requested_path: String,
    pub repo_root: String,
    pub branch: String,
    pub head_short: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub gh_available: bool,
    pub remote_url: Option<String>,
    pub branches: Vec<String>,
    pub files: Vec<GitFileChange>,
    pub worktrees: Vec<GitWorktreeInfo>,
    pub total_commits: u32,
    pub recent_commits: Vec<GitCommitInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileDiff {
    pub file_path: String,
    pub old_contents: String,
    pub new_contents: String,
    pub patch: String,
    pub is_binary: bool,
    pub truncated: bool,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitActionResult {
    pub ok: bool,
    pub message: String,
    pub url: Option<String>,
}

fn configure_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
}

fn run_git_bytes(root: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    configure_command(&mut command);
    let output = command
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotepath=false",
            "-C",
            root,
        ])
        .args(args)
        .output()
        .map_err(|e| format!("无法执行 git：{e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            format!("git {} 执行失败", args.first().copied().unwrap_or(""))
        } else {
            detail
        });
    }
    Ok(output.stdout)
}

fn run_git(root: &str, args: &[&str]) -> Result<String, String> {
    let bytes = run_git_bytes(root, args)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn check_gh_available() -> bool {
    let mut command = Command::new("gh");
    configure_command(&mut command);
    command
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn resolve_repo_root(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("请先选择一个本地项目目录".into());
    }
    if !Path::new(trimmed).exists() {
        return Err(format!("目录不存在：{trimmed}"));
    }
    let out = run_git(trimmed, &["rev-parse", "--show-toplevel"])?;
    let root = out.trim().to_owned();
    if root.is_empty() {
        return Err("当前目录不是 Git 仓库".into());
    }
    Ok(root)
}

fn ensure_safe_relative_path(rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err("不允许绝对路径".into());
    }
    for comp in p.components() {
        if matches!(comp, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err("非法文件路径".into());
        }
    }
    Ok(p.to_path_buf())
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(8192);
    bytes[..sample_len].contains(&0)
}

fn parse_numstat(output: &str) -> HashMap<String, (u32, u32)> {
    let mut map: HashMap<String, (u32, u32)> = HashMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let add = parts[0].parse::<u32>().unwrap_or(0);
        let del = parts[1].parse::<u32>().unwrap_or(0);
        let path = parts[2..].join("\t");
        let entry = map.entry(path).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(add);
        entry.1 = entry.1.saturating_add(del);
    }
    map
}

fn parse_status_z(bytes: &[u8], stats: &HashMap<String, (u32, u32)>, repo_root: &str) -> Vec<GitFileChange> {
    let mut files = Vec::new();
    let entries: Vec<&[u8]> = bytes.split(|&b| b == 0).collect();
    let mut i = 0;
    while i < entries.len() {
        let raw = entries[i];
        i += 1;
        if raw.len() < 4 {
            continue;
        }
        let x = raw[0] as char;
        let y = raw[1] as char;
        if raw[2] != b' ' {
            continue;
        }
        let path = String::from_utf8_lossy(&raw[3..]).into_owned();
        let mut old_path = None;
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            if i < entries.len() && !entries[i].is_empty() {
                old_path = Some(String::from_utf8_lossy(entries[i]).into_owned());
                i += 1;
            }
        }
        let untracked = x == '?' && y == '?';
        let staged = !untracked && x != ' ' && x != '!';
        let unstaged = untracked || (y != ' ' && y != '!');
        let (mut additions, deletions) = stats.get(&path).copied().unwrap_or((0, 0));
        if untracked && additions == 0 {
            if let Ok(rel) = ensure_safe_relative_path(&path) {
                let full = Path::new(repo_root).join(rel);
                if let Ok(meta) = fs::metadata(&full) {
                    if meta.is_file() && meta.len() <= MAX_FILE_BYTES as u64 {
                        if let Ok(data) = fs::read(&full) {
                            if !is_probably_binary(&data) {
                                additions = String::from_utf8_lossy(&data).lines().count() as u32;
                            }
                        }
                    }
                }
            }
        }
        files.push(GitFileChange {
            path,
            old_path,
            staged_status: if untracked { " ".into() } else { x.to_string() },
            unstaged_status: if untracked { "?".into() } else { y.to_string() },
            staged,
            unstaged,
            untracked,
            additions,
            deletions,
        });
    }
    files
}

fn parse_worktrees(output: &str, current_root: &str) -> Vec<GitWorktreeInfo> {
    let canonical_current = fs::canonicalize(current_root)
        .unwrap_or_else(|_| PathBuf::from(current_root));
    let mut list = Vec::new();
    let mut cur_path = String::new();
    let mut cur_head = String::new();
    let mut cur_branch = String::new();
    let mut cur_detached = false;
    let mut idx = 0usize;

    let flush = |list: &mut Vec<GitWorktreeInfo>,
                 idx: &mut usize,
                 path: &mut String,
                 head: &mut String,
                 branch: &mut String,
                 detached: &mut bool| {
        if path.is_empty() {
            return;
        }
        let canon = fs::canonicalize(&*path).unwrap_or_else(|_| PathBuf::from(&*path));
        let short_head = head.chars().take(7).collect::<String>();
        let branch_display = if !branch.is_empty() {
            branch
                .strip_prefix("refs/heads/")
                .unwrap_or(branch)
                .to_owned()
        } else if *detached {
            format!("detached@{short_head}")
        } else {
            "(bare)".to_owned()
        };
        list.push(GitWorktreeInfo {
            path: path.clone(),
            head: short_head,
            branch: branch_display,
            is_main: *idx == 0,
            is_current: canon == canonical_current,
            detached: *detached,
        });
        *idx += 1;
        path.clear();
        head.clear();
        branch.clear();
        *detached = false;
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(
                &mut list,
                &mut idx,
                &mut cur_path,
                &mut cur_head,
                &mut cur_branch,
                &mut cur_detached,
            );
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("worktree ") {
            cur_path = rest.to_owned();
        } else if let Some(rest) = trimmed.strip_prefix("HEAD ") {
            cur_head = rest.to_owned();
        } else if let Some(rest) = trimmed.strip_prefix("branch ") {
            cur_branch = rest.to_owned();
        } else if trimmed == "detached" {
            cur_detached = true;
        }
    }
    flush(
        &mut list,
        &mut idx,
        &mut cur_path,
        &mut cur_head,
        &mut cur_branch,
        &mut cur_detached,
    );
    list
}

fn parse_recent_commits(output: &str) -> Vec<GitCommitInfo> {
    let mut commits = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\x1f').collect();
        if parts.len() < 5 {
            continue;
        }
        commits.push(GitCommitInfo {
            hash: parts[0].to_owned(),
            short_hash: parts[1].to_owned(),
            subject: parts[2].to_owned(),
            author: parts[3].to_owned(),
            relative_time: parts[4].to_owned(),
        });
    }
    commits
}

fn remote_to_web_url(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches(".git");
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        return Some(trimmed.to_owned());
    }
    if let Some(rest) = trimmed.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return Some(format!("https://{host}/{path}"));
        }
    }
    if let Some(rest) = trimmed.strip_prefix("ssh://git@") {
        return Some(format!("https://{rest}"));
    }
    None
}

fn sanitize_branch_dir(branch: &str) -> String {
    let mut out = String::new();
    for ch in branch.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let cleaned = out.trim_matches('-').to_owned();
    if cleaned.is_empty() {
        "worktree".to_owned()
    } else {
        cleaned
    }
}

fn overview_sync(path: String) -> Result<GitOverview, String> {
    let repo_root = match resolve_repo_root(&path) {
        Ok(root) => root,
        Err(_) => {
            return Ok(GitOverview {
                is_repo: false,
                requested_path: path,
                repo_root: String::new(),
                branch: String::new(),
                head_short: String::new(),
                upstream: None,
                ahead: 0,
                behind: 0,
                gh_available: check_gh_available(),
                remote_url: None,
                branches: Vec::new(),
                files: Vec::new(),
                worktrees: Vec::new(),
                total_commits: 0,
                recent_commits: Vec::new(),
            });
        }
    };

    let branch = run_git(&repo_root, &["symbolic-ref", "--short", "HEAD"])
        .map(|s| s.trim().to_owned())
        .or_else(|_| {
            run_git(&repo_root, &["rev-parse", "--short", "HEAD"])
                .map(|s| format!("detached@{}", s.trim()))
        })
        .unwrap_or_else(|_| "HEAD".to_owned());

    let head_short = run_git(&repo_root, &["rev-parse", "--short", "HEAD"])
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    let upstream = run_git(
        &repo_root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .map(|s| s.trim().to_owned())
    .filter(|s| !s.is_empty());

    let (ahead, behind) = if let Some(ref up) = upstream {
        let range = format!("{up}...HEAD");
        run_git(&repo_root, &["rev-list", "--left-right", "--count", &range])
            .ok()
            .and_then(|s| {
                let mut parts = s.split_whitespace();
                let b = parts.next()?.parse::<u32>().ok()?;
                let a = parts.next()?.parse::<u32>().ok()?;
                Some((a, b))
            })
            .unwrap_or((0, 0))
    } else {
        (0, 0)
    };

    let remote_url = run_git(&repo_root, &["remote", "get-url", "origin"])
        .ok()
        .and_then(|u| remote_to_web_url(&u));

    let mut stats = parse_numstat(
        &run_git(&repo_root, &["diff", "--numstat"]).unwrap_or_default(),
    );
    for (k, (a, d)) in parse_numstat(
        &run_git(&repo_root, &["diff", "--cached", "--numstat"]).unwrap_or_default(),
    ) {
        let entry = stats.entry(k).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(a);
        entry.1 = entry.1.saturating_add(d);
    }

    let status_bytes = run_git_bytes(&repo_root, &["status", "--porcelain=v1", "-z", "-uall"])
        .unwrap_or_default();
    let files = parse_status_z(&status_bytes, &stats, &repo_root);

    let worktrees = run_git(&repo_root, &["worktree", "list", "--porcelain"])
        .map(|out| parse_worktrees(&out, &repo_root))
        .unwrap_or_default();

    let branches = run_git(
        &repo_root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map(|out| {
        out.lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    })
    .unwrap_or_default();

    let total_commits = run_git(&repo_root, &["rev-list", "--count", "HEAD"])
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);

    let recent_commits = run_git(
        &repo_root,
        &[
            "log",
            "-n",
            "50",
            "--pretty=format:%H%x1f%h%x1f%s%x1f%an%x1f%cr",
        ],
    )
    .map(|out| parse_recent_commits(&out))
    .unwrap_or_default();

    Ok(GitOverview {
        is_repo: true,
        requested_path: path,
        repo_root,
        branch,
        head_short,
        upstream,
        ahead,
        behind,
        gh_available: check_gh_available(),
        remote_url,
        branches,
        files,
        worktrees,
        total_commits,
        recent_commits,
    })
}

fn read_git_object(repo_root: &str, spec: &str) -> (String, bool, bool) {
    match run_git_bytes(repo_root, &["show", spec]) {
        Ok(bytes) => {
            if is_probably_binary(&bytes) {
                return (String::new(), true, false);
            }
            if bytes.len() > MAX_FILE_BYTES {
                let text = String::from_utf8_lossy(&bytes[..MAX_FILE_BYTES]).into_owned();
                return (text, false, true);
            }
            (String::from_utf8_lossy(&bytes).into_owned(), false, false)
        }
        Err(_) => (String::new(), false, false),
    }
}

fn read_working_file(repo_root: &str, rel_path: &str) -> (String, bool, bool) {
    let Ok(rel) = ensure_safe_relative_path(rel_path) else {
        return (String::new(), false, false);
    };
    let full = Path::new(repo_root).join(rel);
    let Ok(bytes) = fs::read(&full) else {
        return (String::new(), false, false);
    };
    if is_probably_binary(&bytes) {
        return (String::new(), true, false);
    }
    if bytes.len() > MAX_FILE_BYTES {
        let text = String::from_utf8_lossy(&bytes[..MAX_FILE_BYTES]).into_owned();
        return (text, false, true);
    }
    (String::from_utf8_lossy(&bytes).into_owned(), false, false)
}

fn file_diff_sync(path: String, file_path: String, mode: String) -> Result<GitFileDiff, String> {
    let repo_root = resolve_repo_root(&path)?;
    ensure_safe_relative_path(&file_path)?;

    let (old_contents, new_contents, old_bin, new_bin, old_trunc, new_trunc, patch) =
        match mode.as_str() {
            "staged" => {
                let (old_c, ob, ot) = read_git_object(&repo_root, &format!("HEAD:{file_path}"));
                let (new_c, nb, nt) = read_git_object(&repo_root, &format!(":{file_path}"));
                let patch = run_git(
                    &repo_root,
                    &["diff", "--cached", "--no-ext-diff", "--no-textconv", "--", &file_path],
                )
                .unwrap_or_default();
                (old_c, new_c, ob, nb, ot, nt, patch)
            }
            "working" => {
                let (old_c, ob, ot) = read_git_object(&repo_root, &format!(":{file_path}"));
                let (new_c, nb, nt) = read_working_file(&repo_root, &file_path);
                let patch = run_git(
                    &repo_root,
                    &["diff", "--no-ext-diff", "--no-textconv", "--", &file_path],
                )
                .unwrap_or_default();
                (old_c, new_c, ob, nb, ot, nt, patch)
            }
            _ => {
                let (old_c, ob, ot) = read_git_object(&repo_root, &format!("HEAD:{file_path}"));
                let (new_c, nb, nt) = read_working_file(&repo_root, &file_path);
                let patch = run_git(
                    &repo_root,
                    &["diff", "--no-ext-diff", "--no-textconv", "HEAD", "--", &file_path],
                )
                .or_else(|_| {
                    run_git(
                        &repo_root,
                        &["diff", "--no-ext-diff", "--no-textconv", "--", &file_path],
                    )
                })
                .unwrap_or_default();
                (old_c, new_c, ob, nb, ot, nt, patch)
            }
        };

    let mut additions = 0u32;
    let mut deletions = 0u32;
    if !patch.is_empty() {
        for line in patch.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
    } else if old_contents.is_empty() && !new_contents.is_empty() {
        additions = new_contents.lines().count() as u32;
    }

    let truncated_patch = if patch.len() > MAX_PATCH_BYTES {
        patch.chars().take(MAX_PATCH_BYTES).collect()
    } else {
        patch
    };

    Ok(GitFileDiff {
        file_path,
        old_contents,
        new_contents,
        patch: truncated_patch,
        is_binary: old_bin || new_bin,
        truncated: old_trunc || new_trunc,
        additions,
        deletions,
    })
}

mod commands;
mod turn_diff;

pub use commands::*;
pub use turn_diff::*;
