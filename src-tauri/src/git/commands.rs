use super::*;

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
