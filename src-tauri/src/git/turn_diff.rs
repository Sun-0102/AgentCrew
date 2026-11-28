use super::*;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileContent {
    pub path: String,
    pub display_path: String,
    pub contents: String,
    pub is_binary: bool,
    pub truncated: bool,
}

fn find_file_in_workspace(root: &Path, target_name: &str) -> Option<PathBuf> {
    let normalized = target_name.replace('\\', "/");
    let needle = normalized.trim_start_matches("./");
    if let Ok(output) = run_git(
        root.to_str()?,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    ) {
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed == needle || trimmed.ends_with(&format!("/{needle}")) {
                let candidate = root.join(trimmed);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn strip_line_suffix(path: &str) -> &str {
    // Strip trailing :line or :line:col while preserving Windows drive letters like C:\...
    let mut end = path;
    for _ in 0..2 {
        if let Some((prefix, suffix)) = end.rsplit_once(':') {
            if !suffix.is_empty()
                && suffix.bytes().all(|b| b.is_ascii_digit())
                && prefix.len() > 1
            {
                end = prefix;
                continue;
            }
        }
        break;
    }
    end
}

#[tauri::command]
pub async fn workspace_read_file(
    workspace_path: Option<String>,
    file_path: String,
) -> Result<WorkspaceFileContent, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut raw = file_path.trim().to_owned();
        // Strip trailing #L123 fragment if present
        if let Some(idx) = raw.find('#') {
            raw.truncate(idx);
        }
        raw = strip_line_suffix(&raw).to_owned();
        raw = crate::local_files::clean_local_file_input(&raw);
        if raw.is_empty() {
            return Err("文件路径为空".into());
        }

        let ws_root = workspace_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let candidate = PathBuf::from(&raw);
        let resolved = if candidate.is_absolute() && candidate.is_file() {
            candidate
        } else if let Some(ref root) = ws_root {
            let direct = root.join(&raw);
            if direct.is_file() {
                direct
            } else if let Some(found) = find_file_in_workspace(root, &raw) {
                found
            } else {
                return Err(format!("找不到文件：{raw}"));
            }
        } else {
            return Err(format!("找不到文件：{raw}"));
        };

        let canonical = crate::storage::canonicalize_path(&resolved)
            .map_err(|e| format!("无法访问文件 {raw}：{e}"))?;
        if !canonical.is_file() {
            return Err(format!("目标不是文件：{raw}"));
        }

        let bytes = fs::read(&canonical).map_err(|e| format!("读取文件失败：{e}"))?;
        let display = ws_root
            .as_ref()
            .and_then(|r| crate::storage::canonicalize_path(r).ok())
            .and_then(|r| canonical.strip_prefix(&r).ok().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_else(|| canonical.to_string_lossy().into_owned());

        if is_probably_binary(&bytes) {
            return Ok(WorkspaceFileContent {
                path: canonical.to_string_lossy().into_owned(),
                display_path: display,
                contents: String::new(),
                is_binary: true,
                truncated: false,
            });
        }

        let truncated = bytes.len() > MAX_FILE_BYTES;
        let slice = if truncated {
            &bytes[..MAX_FILE_BYTES]
        } else {
            &bytes[..]
        };
        let contents = String::from_utf8_lossy(slice).into_owned();

        Ok(WorkspaceFileContent {
            path: canonical.to_string_lossy().into_owned(),
            display_path: display,
            contents,
            is_binary: false,
            truncated,
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
