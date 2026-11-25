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

#[tauri::command]
pub async fn git_overview(path: String) -> Result<GitOverview, String> {
    tauri::async_runtime::spawn_blocking(move || overview_sync(path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_file_diff(
    path: String,
    file_path: String,
    mode: Option<String>,
) -> Result<GitFileDiff, String> {
    let mode = mode.unwrap_or_else(|| "all".to_owned());
    tauri::async_runtime::spawn_blocking(move || file_diff_sync(path, file_path, mode))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_stage(
    path: String,
    files: Vec<String>,
    all: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        if all.unwrap_or(false) {
            run_git(&repo_root, &["add", "-A"])?;
            return Ok(GitActionResult {
                ok: true,
                message: "已暂存全部变更".into(),
                url: None,
            });
        }
        if files.is_empty() {
            return Err("未指定要暂存的文件".into());
        }
        for f in &files {
            ensure_safe_relative_path(f)?;
        }
        let mut args = vec!["add", "--"];
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        run_git(&repo_root, &args)?;
        Ok(GitActionResult {
            ok: true,
            message: format!("已暂存 {} 个文件", files.len()),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_unstage(
    path: String,
    files: Vec<String>,
    all: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let has_head = run_git(&repo_root, &["rev-parse", "--verify", "HEAD"]).is_ok();
        if all.unwrap_or(false) {
            if has_head {
                run_git(&repo_root, &["reset", "HEAD"])?;
            } else {
                run_git(&repo_root, &["rm", "-r", "--cached", "."])?;
            }
            return Ok(GitActionResult {
                ok: true,
                message: "已取消全部暂存".into(),
                url: None,
            });
        }
        if files.is_empty() {
            return Err("未指定要取消暂存的文件".into());
        }
        for f in &files {
            ensure_safe_relative_path(f)?;
        }
        let mut args = if has_head {
            vec!["reset", "HEAD", "--"]
        } else {
            vec!["rm", "--cached", "--"]
        };
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        run_git(&repo_root, &args)?;
        Ok(GitActionResult {
            ok: true,
            message: format!("已取消暂存 {} 个文件", files.len()),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_discard(
    path: String,
    files: Vec<String>,
    untracked: Vec<String>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        if !files.is_empty() {
            for f in &files {
                ensure_safe_relative_path(f)?;
            }
            let mut args = vec!["checkout", "--"];
            let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
            args.extend(refs);
            run_git(&repo_root, &args)?;
        }
        let canon_root = fs::canonicalize(&repo_root).unwrap_or_else(|_| PathBuf::from(&repo_root));
        for f in &untracked {
            let rel = ensure_safe_relative_path(f)?;
            let full = canon_root.join(rel);
            if full.starts_with(&canon_root) && full.exists() {
                if full.is_dir() {
                    fs::remove_dir_all(&full).map_err(|e| format!("删除目录失败：{e}"))?;
                } else {
                    fs::remove_file(&full).map_err(|e| format!("删除文件失败：{e}"))?;
                }
            }
        }
        Ok(GitActionResult {
            ok: true,
            message: "已放弃所选变更".into(),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_commit(
    path: String,
    message: String,
    stage_all: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let msg = message.trim();
        if msg.is_empty() {
            return Err("请输入提交说明（Commit Message）".into());
        }
        if stage_all.unwrap_or(false) {
            run_git(&repo_root, &["add", "-A"])?;
        }
        let out = run_git(&repo_root, &["commit", "-m", msg])?;
        let summary = out.lines().next().unwrap_or("提交成功").trim().to_owned();
        Ok(GitActionResult {
            ok: true,
            message: summary,
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_push(
    path: String,
    set_upstream: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let branch = run_git(&repo_root, &["symbolic-ref", "--short", "HEAD"])
            .map(|s| s.trim().to_owned())
            .map_err(|_| "当前处于 detached HEAD 状态，无法直接推送".to_owned())?;
        let has_upstream = run_git(
            &repo_root,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .is_ok();
        if !has_upstream && set_upstream.unwrap_or(true) {
            run_git(&repo_root, &["push", "-u", "origin", &branch])?;
        } else {
            run_git(&repo_root, &["push"])?;
        }
        Ok(GitActionResult {
            ok: true,
            message: format!("已推送分支 {branch}"),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_checkout_branch(
    path: String,
    branch: String,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let b = branch.trim();
        if b.is_empty() {
            return Err("分支名不能为空".into());
        }
        run_git(&repo_root, &["checkout", b])?;
        Ok(GitActionResult {
            ok: true,
            message: format!("已切换到分支 {b}"),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_create_pr(
    path: String,
    title: String,
    body: String,
    draft: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let branch = run_git(&repo_root, &["symbolic-ref", "--short", "HEAD"])
            .map(|s| s.trim().to_owned())
            .map_err(|_| "当前处于 detached HEAD，无法创建 PR".to_owned())?;

        if check_gh_available() {
            let t = title.trim();
            if t.is_empty() {
                return Err("请输入 PR 标题".into());
            }
            let mut command = Command::new("gh");
            configure_command(&mut command);
            command.current_dir(&repo_root);
            command.args(["pr", "create", "--title", t, "--body", body.trim()]);
            if draft.unwrap_or(false) {
                command.arg("--draft");
            }
            let output = command.output().map_err(|e| format!("无法执行 gh：{e}"))?;
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                let url = stdout
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("http://") || l.starts_with("https://"))
                    .map(|s| s.to_owned());
                return Ok(GitActionResult {
                    ok: true,
                    message: "PR 创建成功".into(),
                    url,
                });
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if stderr.is_empty() {
                "gh pr create 执行失败，请确认已执行 gh auth login 并已推送当前分支".into()
            } else {
                stderr
            });
        }

        let remote = run_git(&repo_root, &["remote", "get-url", "origin"])
            .ok()
            .and_then(|u| remote_to_web_url(&u))
            .ok_or_else(|| "未检测到 gh CLI，且无法解析 origin 远程地址".to_owned())?;
        let compare_url = if remote.contains("gitlab") {
            format!("{remote}/-/merge_requests/new?merge_request[source_branch]={branch}")
        } else {
            format!("{remote}/compare/{branch}?expand=1")
        };
        Ok(GitActionResult {
            ok: true,
            message: "已生成网页 PR 对比链接（未检测到 gh CLI）".into(),
            url: Some(compare_url),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_worktree_create(
    app: tauri::AppHandle,
    path: String,
    branch_name: String,
    base_branch: Option<String>,
) -> Result<GitWorktreeInfo, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let branch = branch_name.trim();
        if branch.is_empty() {
            return Err("请输入新分支名称".into());
        }
        let repo_name = Path::new(&repo_root)
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_branch_dir)
            .unwrap_or_else(|| "repo".to_owned());
        let dir_name = sanitize_branch_dir(branch);
        let worktrees_base = data_dir.join("worktrees").join(repo_name);
        fs::create_dir_all(&worktrees_base)
            .map_err(|e| format!("无法创建 worktrees 目录：{e}"))?;
        let target_path = worktrees_base.join(&dir_name);
        if target_path.exists() {
            return Err(format!(
                "目标 Worktree 目录已存在：{}",
                target_path.display()
            ));
        }
        let target_str = target_path.to_string_lossy().into_owned();

        let branch_exists = run_git(
            &repo_root,
            &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
        )
        .is_ok();

        if branch_exists {
            run_git(&repo_root, &["worktree", "add", &target_str, branch])?;
        } else if let Some(base) = base_branch.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            run_git(&repo_root, &["worktree", "add", "-b", branch, &target_str, base])?;
        } else {
            run_git(&repo_root, &["worktree", "add", "-b", branch, &target_str])?;
        }

        let head = run_git(&target_str, &["rev-parse", "--short", "HEAD"])
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();

        Ok(GitWorktreeInfo {
            path: target_str,
            head,
            branch: branch.to_owned(),
            is_main: false,
            is_current: false,
            detached: false,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_worktree_remove(
    path: String,
    worktree_path: String,
    force: Option<bool>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        if force.unwrap_or(false) {
            run_git(&repo_root, &["worktree", "remove", "--force", &worktree_path])?;
        } else {
            run_git(&repo_root, &["worktree", "remove", &worktree_path])?;
        }
        let _ = run_git(&repo_root, &["worktree", "prune"]);
        Ok(GitActionResult {
            ok: true,
            message: "Worktree 已移除".into(),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_commits(
    path: String,
    skip: Option<u32>,
    limit: Option<u32>,
    query: Option<String>,
) -> Result<Vec<GitCommitInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let skip_val = skip.unwrap_or(0);
        let limit_val = match limit.unwrap_or(50) {
            0 => 5000,
            n => n.min(5000),
        };
        let skip_arg = format!("--skip={skip_val}");
        let limit_arg = format!("-n{limit_val}");
        let mut args = vec![
            "log",
            &skip_arg,
            &limit_arg,
            "--pretty=format:%H%x1f%h%x1f%s%x1f%an%x1f%cr",
        ];
        let grep_arg;
        if let Some(q) = query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            grep_arg = format!("--grep={q}");
            args.push("-i");
            args.push(&grep_arg);
        }
        let out = run_git(&repo_root, &args).unwrap_or_default();
        Ok(parse_recent_commits(&out))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_commit_files(
    path: String,
    hash: String,
) -> Result<Vec<GitCommitFile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let h = hash.trim();
        if h.is_empty() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("无效的提交哈希".into());
        }
        let numstat = run_git(
            &repo_root,
            &["diff-tree", "--no-commit-id", "--numstat", "-r", "--root", h],
        )
        .unwrap_or_default();
        let stats = parse_numstat(&numstat);

        let name_status = run_git(
            &repo_root,
            &["diff-tree", "--no-commit-id", "--name-status", "-r", "--root", h],
        )
        .unwrap_or_default();
        let mut files = Vec::new();
        for line in name_status.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                continue;
            }
            let raw_status = parts[0].trim();
            let status = raw_status.chars().next().unwrap_or('M').to_string();
            let file_path = if parts.len() >= 3 && (status == "R" || status == "C") {
                parts[2].to_owned()
            } else {
                parts[1].to_owned()
            };
            let (additions, deletions) = stats.get(&file_path).copied().unwrap_or((0, 0));
            files.push(GitCommitFile {
                path: file_path,
                status,
                additions,
                deletions,
            });
        }
        Ok(files)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_commit_file_diff(
    path: String,
    hash: String,
    file_path: String,
) -> Result<GitFileDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let h = hash.trim();
        if h.is_empty() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("无效的提交哈希".into());
        }
        ensure_safe_relative_path(&file_path)?;
        let has_parent = run_git(&repo_root, &["rev-parse", "--verify", &format!("{h}^")]).is_ok();
        let (old_contents, old_bin, old_trunc) = if has_parent {
            read_git_object(&repo_root, &format!("{h}^:{file_path}"))
        } else {
            (String::new(), false, false)
        };
        let (new_contents, new_bin, new_trunc) =
            read_git_object(&repo_root, &format!("{h}:{file_path}"));
        let patch = if has_parent {
            run_git(
                &repo_root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    &format!("{h}^"),
                    h,
                    "--",
                    &file_path,
                ],
            )
            .unwrap_or_default()
        } else {
            run_git(
                &repo_root,
                &[
                    "show",
                    "--format=",
                    "--no-ext-diff",
                    "--no-textconv",
                    h,
                    "--",
                    &file_path,
                ],
            )
            .unwrap_or_default()
        };
        let mut additions = 0u32;
        let mut deletions = 0u32;
        for line in patch.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
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
    })
    .await
    .map_err(|e| e.to_string())?
}

fn try_cli_commit_message(repo_root: &str, prompt: &str) -> Option<(String, &'static str)> {
    let candidates: &[(&str, &[&str], &str)] = &[
        ("claude", &["-p", prompt, "--output-format", "text"], "Claude"),
        ("codex", &["exec", prompt], "Codex"),
        ("gemini", &["-p", prompt], "Gemini"),
    ];
    for (bin, args, label) in candidates {
        let mut command = Command::new(bin);
        configure_command(&mut command);
        crate::process::apply_system_proxy(&mut command);
        command.current_dir(repo_root).args(*args);
        if let Ok(output) = command.output() {
            if output.status.success() {
                let raw = String::from_utf8_lossy(&output.stdout);
                let cleaned = raw
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !l.starts_with("```"))
                    .unwrap_or("")
                    .trim_matches('`')
                    .trim_matches('"')
                    .trim()
                    .to_owned();
                if !cleaned.is_empty() && cleaned.len() <= 160 {
                    return Some((cleaned, *label));
                }
            }
        }
    }
    None
}

fn fallback_commit_message(files: &[GitFileChange]) -> String {
    if files.is_empty() {
        return "chore: update workspace".into();
    }
    let all_docs = files
        .iter()
        .all(|f| f.path.ends_with(".md") || f.path.starts_with("docs/"));
    let all_tests = files
        .iter()
        .all(|f| f.path.contains("test") || f.path.starts_with("tests/"));
    let all_styles = files.iter().all(|f| f.path.ends_with(".css"));
    let kind = if all_docs {
        "docs"
    } else if all_tests {
        "test"
    } else if all_styles {
        "style"
    } else if files.iter().any(|f| f.untracked || f.staged_status == "A") {
        "feat"
    } else {
        "chore"
    };

    let scope = files
        .first()
        .and_then(|f| {
            let parts: Vec<&str> = f.path.split('/').collect();
            if parts.len() >= 2 {
                Some(parts[parts.len() - 2])
            } else {
                None
            }
        })
        .unwrap_or("");

    let names: Vec<&str> = files
        .iter()
        .take(3)
        .map(|f| f.path.rsplit('/').next().unwrap_or(f.path.as_str()))
        .collect();
    let suffix = if files.len() > 3 {
        format!(" and {} more files", files.len() - 3)
    } else {
        String::new()
    };
    if scope.is_empty() {
        format!("{kind}: update {}{suffix}", names.join(", "))
    } else {
        format!("{kind}({scope}): update {}{suffix}", names.join(", "))
    }
}

#[tauri::command]
pub async fn git_generate_commit_message(path: String) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let staged_stat = run_git(&repo_root, &["diff", "--cached", "--stat"]).unwrap_or_default();
        let (summary, diff_snippet) = if !staged_stat.trim().is_empty() {
            let diff = run_git(&repo_root, &["diff", "--cached", "--no-ext-diff"]).unwrap_or_default();
            (staged_stat, diff)
        } else {
            let status = run_git(&repo_root, &["status", "--short"]).unwrap_or_default();
            if status.trim().is_empty() {
                return Err("当前工作区没有变更，无法生成提交说明".into());
            }
            let diff = run_git(&repo_root, &["diff", "HEAD", "--no-ext-diff"])
                .or_else(|_| run_git(&repo_root, &["diff", "--no-ext-diff"]))
                .unwrap_or_default();
            (status, diff)
        };

        let clipped_diff: String = diff_snippet.chars().take(10_000).collect();
        let prompt = format!(
            "Generate a single concise Conventional Commit subject line (max 72 chars, e.g. feat(scope): description) for the following git changes. Output ONLY the commit message line with no quotes, markdown, or explanation.\n\nSummary:\n{summary}\n\nDiff:\n{clipped_diff}"
        );

        if let Some((msg, provider)) = try_cli_commit_message(&repo_root, &prompt) {
            return Ok(GitActionResult {
                ok: true,
                message: msg,
                url: Some(provider.to_owned()),
            });
        }

        let status_bytes =
            run_git_bytes(&repo_root, &["status", "--porcelain=v1", "-z", "-uall"])
                .unwrap_or_default();
        let files = parse_status_z(&status_bytes, &HashMap::new(), &repo_root);
        Ok(GitActionResult {
            ok: true,
            message: fallback_commit_message(&files),
            url: Some("本地摘要".to_owned()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim().to_owned();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("仅支持打开 http/https 链接".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let status = Command::new("open").arg(&trimmed).status();
        #[cfg(target_os = "windows")]
        let status = Command::new("cmd").args(["/C", "start", "", &trimmed]).status();
        #[cfg(all(unix, not(target_os = "macos")))]
        let status = Command::new("xdg-open").arg(&trimmed).status();
        status
            .map_err(|e| format!("无法打开浏览器：{e}"))
            .and_then(|s| {
                if s.success() {
                    Ok(())
                } else {
                    Err("打开浏览器失败".into())
                }
            })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnDiffFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnDiffSummary {
    pub repo_root: String,
    pub before_tree: String,
    pub after_tree: String,
    pub files: Vec<TurnDiffFile>,
    pub total_additions: u32,
    pub total_deletions: u32,
}

struct TempIndexGuard(PathBuf);

impl Drop for TempIndexGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let lock = self.0.with_extension("lock");
        let _ = fs::remove_file(&lock);
    }
}

fn run_git_with_index(root: &str, index_path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    configure_command(&mut command);
    let output = command
        .env("GIT_INDEX_FILE", index_path)
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn capture_worktree_tree(repo_root: &str) -> Result<String, String> {
    let temp_index = std::env::temp_dir().join(format!(
        "agentcrew-turn-idx-{}",
        uuid::Uuid::new_v4()
    ));
    let _guard = TempIndexGuard(temp_index.clone());
    // Seed from existing index if present so git add -A is fast; otherwise start fresh.
    let _ = run_git_with_index(repo_root, &temp_index, &["read-tree", "HEAD"]);
    run_git_with_index(repo_root, &temp_index, &["add", "-A"])?;
    let tree = run_git_with_index(repo_root, &temp_index, &["write-tree"])?;
    let trimmed = tree.trim().to_owned();
    if trimmed.is_empty() {
        return Err("无法生成工作区树快照".into());
    }
    Ok(trimmed)
}

fn compute_turn_diff_summary(
    repo_root: &str,
    before_tree: &str,
    after_tree: &str,
) -> Result<TurnDiffSummary, String> {
    let numstat = run_git(
        repo_root,
        &["diff", "--numstat", "--no-renames", before_tree, after_tree],
    )?;
    let name_status = run_git(
        repo_root,
        &["diff", "--name-status", "--no-renames", before_tree, after_tree],
    )?;

    let mut status_map: HashMap<String, String> = HashMap::new();
    for line in name_status.lines() {
        let mut parts = line.splitn(2, '\t');
        if let (Some(st), Some(path)) = (parts.next(), parts.next()) {
            let code = st.trim().chars().next().unwrap_or('M').to_string();
            status_map.insert(path.trim().to_owned(), code);
        }
    }

    let mut files = Vec::new();
    let mut total_additions = 0u32;
    let mut total_deletions = 0u32;

    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let additions = parts[0].parse::<u32>().unwrap_or(0);
        let deletions = parts[1].parse::<u32>().unwrap_or(0);
        let path = parts[2..].join("\t");
        let status = status_map
            .get(&path)
            .cloned()
            .unwrap_or_else(|| "M".to_owned());
        total_additions = total_additions.saturating_add(additions);
        total_deletions = total_deletions.saturating_add(deletions);
        files.push(TurnDiffFile {
            path,
            status,
            additions,
            deletions,
        });
    }

    Ok(TurnDiffSummary {
        repo_root: repo_root.to_owned(),
        before_tree: before_tree.to_owned(),
        after_tree: after_tree.to_owned(),
        files,
        total_additions,
        total_deletions,
    })
}

fn turn_baselines() -> &'static Mutex<HashMap<(String, String), (String, String)>> {
    static BASELINES: OnceLock<Mutex<HashMap<(String, String), (String, String)>>> =
        OnceLock::new();
    BASELINES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn begin_turn_snapshot(conversation_id: &str, agent: &str, workspace_path: &str) {
    let Ok(repo_root) = resolve_repo_root(workspace_path) else {
        return;
    };
    let Ok(before_tree) = capture_worktree_tree(&repo_root) else {
        return;
    };
    if let Ok(mut map) = turn_baselines().lock() {
        map.insert(
            (conversation_id.to_owned(), agent.to_owned()),
            (repo_root, before_tree),
        );
    }
}

pub fn finish_turn_snapshot(
    store: &crate::storage::Store,
    conversation_id: &str,
    agent: &str,
) {
    let baseline = turn_baselines()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&(conversation_id.to_owned(), agent.to_owned())));
    let Some((repo_root, before_tree)) = baseline else {
        return;
    };
    let Ok(after_tree) = capture_worktree_tree(&repo_root) else {
        return;
    };
    if before_tree == after_tree {
        return;
    }
    let Ok(summary) = compute_turn_diff_summary(&repo_root, &before_tree, &after_tree) else {
        return;
    };
    if summary.files.is_empty() {
        return;
    }
    let Ok(payload) = serde_json::to_string(&summary) else {
        return;
    };
    if let Ok(connection) = store.connect() {
        let item_id = format!("turn_diff:{}", uuid::Uuid::new_v4());
        let inserted = if agent == "deepseek" && conversation_id == "deepseek-personal" {
            connection.execute(
                "INSERT INTO deepseek_messages(item_id,role,text) VALUES(?1,'turn_diff',?2)",
                rusqlite::params![item_id, payload],
            )
        } else {
            connection.execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'turn_diff',?3,?4)",
                rusqlite::params![conversation_id, item_id, payload, agent],
            )
        };
        if inserted.is_ok() {
            let logical_cid = connection
                .query_row(
                    "SELECT logical_conversation_id FROM agent_conversations WHERE execution_conversation_id=?1",
                    [conversation_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| conversation_id.to_owned());
            crate::events::emit_conversation_sync(&logical_cid);
        }
    }
}

fn validate_tree_hash(hash: &str) -> Result<&str, String> {
    let trimmed = hash.trim();
    if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("无效的快照树哈希".into());
    }
    Ok(trimmed)
}

#[tauri::command]
pub async fn git_turn_file_diff(
    path: String,
    before_tree: String,
    after_tree: String,
    file_path: String,
) -> Result<GitFileDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        let before = validate_tree_hash(&before_tree)?;
        let after = validate_tree_hash(&after_tree)?;
        ensure_safe_relative_path(&file_path)?;

        let (old_contents, old_bin, old_trunc) =
            read_git_object(&repo_root, &format!("{before}:{file_path}"));
        let (new_contents, new_bin, new_trunc) =
            read_git_object(&repo_root, &format!("{after}:{file_path}"));
        let patch = run_git(
            &repo_root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                before,
                after,
                "--",
                &file_path,
            ],
        )
        .unwrap_or_default();

        let mut additions = 0u32;
        let mut deletions = 0u32;
        for line in patch.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
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
    })
    .await
    .map_err(|e| e.to_string())?
}

fn revert_files_to_tree(repo_root: &str, before_tree: &str, files: &[String]) -> Result<usize, String> {
    let before = validate_tree_hash(before_tree)?;
    let mut reverted = 0usize;
    for file in files {
        let rel = ensure_safe_relative_path(file)?;
        let full_path = Path::new(repo_root).join(&rel);
        let obj_spec = format!("{before}:{file}");
        let existed_before = run_git(repo_root, &["cat-file", "-e", &obj_spec]).is_ok();
        if existed_before {
            let bytes = run_git_bytes(repo_root, &["show", &obj_spec])?;
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
            }
            fs::write(&full_path, bytes).map_err(|e| format!("恢复文件 {file} 失败：{e}"))?;
            reverted += 1;
        } else if full_path.exists() {
            fs::remove_file(&full_path).map_err(|e| format!("删除新建文件 {file} 失败：{e}"))?;
            reverted += 1;
        }
    }
    Ok(reverted)
}

#[tauri::command]
pub async fn git_turn_revert(
    path: String,
    before_tree: String,
    files: Vec<String>,
) -> Result<GitActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_root = resolve_repo_root(&path)?;
        if files.is_empty() {
            return Err("没有指定要撤销的文件".into());
        }
        let count = revert_files_to_tree(&repo_root, &before_tree, &files)?;
        Ok(GitActionResult {
            ok: true,
            message: format!("已撤销 {count} 个文件的本轮修改"),
            url: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod turn_diff_tests {
    use super::*;

    #[test]
    fn turn_snapshot_diff_and_revert_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agentcrew-turn-diff-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let root = dir.to_str().unwrap();

        run_git(root, &["init"]).unwrap();
        run_git(root, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(root, &["config", "user.name", "Test"]).unwrap();
        fs::write(dir.join("a.txt"), "line1\nline2\n").unwrap();
        run_git(root, &["add", "a.txt"]).unwrap();
        run_git(root, &["commit", "-m", "init"]).unwrap();

        let before = capture_worktree_tree(root).unwrap();

        fs::write(dir.join("a.txt"), "line1\nline2-edited\nline3\n").unwrap();
        fs::write(dir.join("b.txt"), "brand new\n").unwrap();

        let after = capture_worktree_tree(root).unwrap();
        assert_ne!(before, after);

        let summary = compute_turn_diff_summary(root, &before, &after).unwrap();
        assert_eq!(summary.files.len(), 2);
        assert_eq!(summary.total_additions, 3);
        assert_eq!(summary.total_deletions, 1);

        let reverted = revert_files_to_tree(
            root,
            &before,
            &["a.txt".to_owned(), "b.txt".to_owned()],
        )
        .unwrap();
        assert_eq!(reverted, 2);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "line1\nline2\n");
        assert!(!dir.join("b.txt").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
