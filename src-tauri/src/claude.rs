use crate::{process::BoundedOutput, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

type Reply = Result<Value, String>;
const ACTIVE: &str = "('starting','running','waiting_approval','cancel_requested','stopping')";

#[derive(Default)]
pub struct Runtime {
    running_pids: Arc<Mutex<HashMap<String, u32>>>,
    operation: Arc<Mutex<()>>,
    cached_probe: Mutex<Option<Value>>,
}

struct Installation {
    executable: PathBuf,
    version: String,
}

fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    crate::process::apply_system_proxy(&mut cmd);
    cmd
}

impl Installation {
    fn find() -> Result<Self, String> {
        let mut paths = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default();

        #[cfg(unix)]
        {
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                for extra in [
                    home.join(".local/bin"),
                    home.join(".claude/local"),
                    home.join(".npm-global/bin"),
                    home.join(".volta/bin"),
                    home.join("Library/pnpm"),
                ] {
                    if extra.is_dir() {
                        paths.push(extra);
                    }
                }
                let nvm_root = home.join(".nvm/versions/node");
                if let Ok(entries) = std::fs::read_dir(&nvm_root) {
                    let mut nvm_bins: Vec<PathBuf> = entries
                        .flatten()
                        .map(|entry| entry.path().join("bin"))
                        .filter(|p| p.is_dir())
                        .collect();
                    nvm_bins.sort();
                    nvm_bins.reverse();
                    paths.extend(nvm_bins);
                }
            }
            for extra in [PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")] {
                if extra.is_dir() {
                    paths.push(extra);
                }
            }
        }

        #[cfg(windows)]
        {
            if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
                paths.push(roaming.join("npm"));
            }
            if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                paths.push(local.join("Programs/claude"));
            }
        }

        let bin_names: &[&str] = if cfg!(windows) {
            &["claude.exe", "claude.cmd"]
        } else {
            &["claude"]
        };

        for dir in &paths {
            for name in bin_names {
                let candidate = dir.join(name);
                if !candidate.is_file() {
                    continue;
                }
                let executable = candidate.canonicalize().unwrap_or(candidate);
                let Ok(output) = command(&executable).arg("--version").bounded_output() else {
                    continue;
                };
                if !output.status.success() {
                    continue;
                }
                let raw = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                let version = raw
                    .split_whitespace()
                    .next()
                    .unwrap_or(&raw)
                    .trim_start_matches('v')
                    .to_owned();
                if version.is_empty() {
                    continue;
                }
                return Ok(Self {
                    executable,
                    version,
                });
            }
        }

        Err("runtime_not_found：未在本机检测到 Claude Code CLI（claude），请先安装并完成 claude login。".into())
    }
}

fn default_models() -> Vec<Value> {
    vec![
        json!({"model": "sonnet", "displayName": "Claude Sonnet (默认)", "isDefault": true}),
        json!({"model": "opus", "displayName": "Claude Opus", "isDefault": false}),
        json!({"model": "haiku", "displayName": "Claude Haiku", "isDefault": false}),
        json!({"model": "claude-sonnet-4-5", "displayName": "Claude Sonnet 4.5", "isDefault": false}),
        json!({"model": "claude-opus-4-1", "displayName": "Claude Opus 4.1", "isDefault": false}),
    ]
}

fn logical_conversation(connection: &rusqlite::Connection, conversation: &str) -> String {
    if !conversation.starts_with("runtime:") {
        return conversation.to_owned();
    }
    connection
        .query_row(
            "SELECT conversation_id FROM agent_sessions WHERE runtime_conversation_id=?1",
            [conversation],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| conversation.to_owned())
}

pub fn session(store: &Store, conversation: &str) -> Reply {
    store
        .connect()?
        .query_row(
            "SELECT thread_id,turn_id,model,runtime_version,workspace_path,status,error FROM claude_sessions WHERE conversation_id=?1",
            [conversation],
            |r| {
                Ok(json!({
                    "thread_id": r.get::<_, String>(0)?,
                    "turn_id": r.get::<_, Option<String>>(1)?,
                    "model": r.get::<_, String>(2)?,
                    "runtime_version": r.get::<_, String>(3)?,
                    "workspace_path": r.get::<_, String>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "error": r.get::<_, Option<String>>(6)?
                }))
            },
        )
        .optional()
        .map(|s| s.unwrap_or(Value::Null))
        .map_err(|e| e.to_string())
}

pub fn recover(store: &Store) -> Result<(), String> {
    store
        .connect()?
        .execute(
            &format!(
                "UPDATE claude_sessions SET status='interrupted',error='应用已重启，上次 Claude Code 执行结果未知；请核对后处理。' WHERE status IN {ACTIVE}"
            ),
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn set_status(
    store: &Store,
    conversation: &str,
    turn: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let mut connection = store.connect()?;
    let logical_cid = logical_conversation(&connection, conversation);
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let changed = tx
        .execute(
            &format!("UPDATE claude_sessions SET status=?3,error=?4 WHERE conversation_id=?1 AND turn_id=?2 AND status IN {ACTIVE}"),
            params![conversation, turn, status, error],
        )
        .map_err(|e| e.to_string())?;
    if changed > 0 && matches!(status, "completed" | "failed" | "interrupted" | "cancelled") {
        tx.execute(
            "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'task_end',?3,'claude')",
            params![conversation, format!("claude:end:{turn}"), status],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if changed > 0 {
        if matches!(status, "completed" | "failed" | "interrupted" | "cancelled") {
            crate::git::finish_turn_snapshot(store, conversation, "claude");
        }
        crate::events::emit_conversation_sync(&logical_cid);
    }
    Ok(())
}

fn save_message(
    store: &Store,
    conversation: &str,
    item_id: &str,
    role: &str,
    text: &str,
    append: bool,
) -> Result<(), String> {
    let connection = store.connect()?;
    let logical_cid = logical_conversation(&connection, conversation);
    if append {
        let updated = connection
            .execute(
                "UPDATE codex_messages SET text=text||?3 WHERE conversation_id=?1 AND item_id=?2",
                params![conversation, item_id, text],
            )
            .map_err(|e| e.to_string())?;
        if updated == 0 {
            connection
                .execute(
                    "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'claude')",
                    params![conversation, item_id, role, text],
                )
                .map_err(|e| e.to_string())?;
        }
        crate::events::emit_delta(&logical_cid, item_id, role, Some("claude"), "append", text);
    } else {
        connection
            .execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'claude')
                 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text",
                params![conversation, item_id, role, text],
            )
            .map_err(|e| e.to_string())?;
        crate::events::emit_conversation_sync(&logical_cid);
    }
    Ok(())
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Runtime {
    pub fn probe(&self, _store: &Store) -> Reply {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = self.cached_probe.lock().map_err(|e| e.to_string())?.as_ref() {
            if cached["auth"].is_string() {
                return Ok(cached.clone());
            }
        }
        let install = Installation::find()?;
        let caps = json!({
            "resume": true,
            "cancel": true,
            "approval": false,
            "tools": true,
            "streaming": true,
            "workspace_isolation": false
        });
        let result = json!({
            "version": install.version,
            "executable": install.executable,
            "runtime_kind": "claude-code-cli",
            "auth": "Claude Code CLI 原厂登录（额度以真实执行为准）",
            "requires_auth": true,
            "models": default_models(),
            "sidebar_warning": null,
            "capabilities": caps
        });
        *self.cached_probe.lock().map_err(|e| e.to_string())? = Some(result.clone());
        Ok(result)
    }

    #[allow(dead_code)]
    pub fn send_recorded(
        &self,
        store: &Store,
        conversation: &str,
        model: &str,
        text: &str,
        role: &str,
    ) -> Reply {
        self.send_recorded_with_context(store, conversation, model, text, role, None)
    }

    pub fn send_recorded_with_context(
        &self,
        store: &Store,
        conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Reply {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        if text.trim().is_empty() || text.len() > 100_000 || model.is_empty() {
            return Err("请选择 Claude 模型，并填写有效消息。".into());
        }
        let busy: bool = store
            .connect()?
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM claude_sessions WHERE conversation_id=?1 AND status IN {ACTIVE})"),
                [conversation],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if busy {
            return Err("当前 Claude Code 会话正在执行中，请等待完成或先取消。".into());
        }

        let install = Installation::find()?;
        let workspace = PathBuf::from(store.conversation_root(conversation)?)
            .canonicalize()
            .map_err(|e| format!("Claude Code 工作目录不可用：{e}"))?
            .to_str()
            .ok_or("Claude Code 工作目录无效")?
            .to_owned();
        let turn_id = uuid::Uuid::new_v4().to_string();

        let existing_thread: Option<String> = store
            .connect()?
            .query_row(
                "SELECT thread_id FROM claude_sessions WHERE conversation_id=?1",
                [conversation],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let resume_session_id = existing_thread
            .as_deref()
            .filter(|id| !id.starts_with("pending:"))
            .map(str::to_owned);

        let initial_thread = existing_thread.unwrap_or_else(|| format!("pending:{turn_id}"));

        {
            let mut connection = store.connect()?;
            let logical_cid = logical_conversation(&connection, conversation);
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO claude_sessions(conversation_id,thread_id,turn_id,model,runtime_version,workspace_path,status,error)
                 VALUES(?1,?2,?3,?4,?5,?6,'running',NULL)
                 ON CONFLICT(conversation_id) DO UPDATE SET
                   turn_id=excluded.turn_id,
                   model=excluded.model,
                   runtime_version=excluded.runtime_version,
                   workspace_path=excluded.workspace_path,
                   status='running',
                   error=NULL",
                params![
                    conversation,
                    initial_thread,
                    turn_id,
                    model,
                    install.version,
                    workspace
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'claude')",
                params![conversation, format!("claude:in:{turn_id}"), role, text],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            crate::events::emit_conversation_sync(&logical_cid);
        }

        let store_path = store.path.clone();
        let conv_owned = conversation.to_owned();
        let model_owned = model.to_owned();
        let text_owned = match extra_context.map(str::trim).filter(|s| !s.is_empty()) {
            Some(ctx) => format!("{ctx}\n\n---\n\n{text}"),
            None => text.to_owned(),
        };
        let turn_owned = turn_id.clone();
        let pids = self.running_pids.clone();

        std::thread::spawn(move || {
            let store = Store { path: store_path };
            crate::git::begin_turn_snapshot(&conv_owned, "claude", &workspace);
            let mut cmd = command(&install.executable);
            cmd.current_dir(&workspace)
                .args([
                    "-p",
                    &text_owned,
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--permission-mode",
                    "bypassPermissions",
                    "--model",
                    &model_owned,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if let Some(ref sid) = resume_session_id {
                cmd.args(["--resume", sid]);
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(err) => {
                    let _ = set_status(
                        &store,
                        &conv_owned,
                        &turn_owned,
                        "failed",
                        Some(&format!("启动 Claude Code 失败：{err}")),
                    );
                    return;
                }
            };

            let pid = child.id();
            if let Ok(mut map) = pids.lock() {
                map.insert(conv_owned.clone(), pid);
            }

            let msg_item = format!("claude:msg:{turn_owned}");
            let mut emitted_text = String::new();
            let mut had_error: Option<String> = None;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    let kind = val["type"].as_str().unwrap_or("");
                    if let Some(sid) = val["session_id"].as_str().filter(|s| !s.is_empty()) {
                        if let Ok(conn) = store.connect() {
                            let _ = conn.execute(
                                "UPDATE claude_sessions SET thread_id=?2 WHERE conversation_id=?1",
                                params![conv_owned, sid],
                            );
                        }
                    }

                    match kind {
                        "assistant" => {
                            if let Some(blocks) = val["message"]["content"].as_array() {
                                for (idx, block) in blocks.iter().enumerate() {
                                    match block["type"].as_str().unwrap_or("") {
                                        "text" => {
                                            if let Some(t) = block["text"].as_str() {
                                                let delta = if t.starts_with(&emitted_text) {
                                                    &t[emitted_text.len()..]
                                                } else {
                                                    t
                                                };
                                                if !delta.is_empty() {
                                                    emitted_text.push_str(delta);
                                                    let _ = save_message(
                                                        &store,
                                                        &conv_owned,
                                                        &msg_item,
                                                        "assistant",
                                                        delta,
                                                        true,
                                                    );
                                                }
                                            }
                                        }
                                        "thinking" => {
                                            if let Some(thinking) = block["thinking"].as_str() {
                                                if !thinking.trim().is_empty() {
                                                    let tid = format!("claude:think:{turn_owned}:{idx}");
                                                    let _ = save_message(
                                                        &store,
                                                        &conv_owned,
                                                        &tid,
                                                        "thinking",
                                                        thinking,
                                                        false,
                                                    );
                                                }
                                            }
                                        }
                                        "tool_use" => {
                                            let tool_id = block["id"]
                                                .as_str()
                                                .unwrap_or("tool");
                                            let name = block["name"].as_str().unwrap_or("tool");
                                            let input = &block["input"];
                                            let payload = json!({
                                                "tool": name,
                                                "input": input
                                            })
                                            .to_string();
                                            let tid = format!("claude:tool:{turn_owned}:{tool_id}");
                                            let _ = save_message(
                                                &store,
                                                &conv_owned,
                                                &tid,
                                                "tool",
                                                &payload,
                                                false,
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        "result" => {
                            if val["is_error"].as_bool().unwrap_or(false) {
                                had_error = val["result"]
                                    .as_str()
                                    .map(str::to_owned)
                                    .or_else(|| Some("Claude Code 执行失败".to_owned()));
                            } else if emitted_text.is_empty() {
                                if let Some(res_text) = val["result"].as_str() {
                                    if !res_text.trim().is_empty() {
                                        emitted_text.push_str(res_text);
                                        let _ = save_message(
                                            &store,
                                            &conv_owned,
                                            &msg_item,
                                            "assistant",
                                            res_text,
                                            true,
                                        );
                                    }
                                }
                            }
                            let u = &val["usage"];
                            let input_tokens = u["input_tokens"].as_i64().unwrap_or(0);
                            let output_tokens = u["output_tokens"].as_i64().unwrap_or(0);
                            let cache_read = u["cache_read_input_tokens"].as_i64().unwrap_or(0);
                            if input_tokens > 0 || output_tokens > 0 {
                                crate::usage::record(
                                    &store.path,
                                    "claude",
                                    "claude",
                                    &model_owned,
                                    &conv_owned,
                                    input_tokens,
                                    output_tokens,
                                    cache_read,
                                    if val["is_error"].as_bool().unwrap_or(false) {
                                        "failed"
                                    } else {
                                        "completed"
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }

            let status = child.wait();
            if let Ok(mut map) = pids.lock() {
                map.remove(&conv_owned);
            }

            if let Some(err_msg) = had_error {
                let _ = set_status(&store, &conv_owned, &turn_owned, "failed", Some(&err_msg));
            } else if status.map(|s| s.success()).unwrap_or(false) {
                let _ = set_status(&store, &conv_owned, &turn_owned, "completed", None);
            } else {
                let mut stderr_msg = String::new();
                if let Some(mut err_stream) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err_stream, &mut stderr_msg);
                }
                let detail = stderr_msg.trim();
                let msg = if detail.is_empty() {
                    "Claude Code 进程异常退出，请确认本机已完成 claude login。".to_owned()
                } else {
                    detail.chars().take(400).collect()
                };
                let _ = set_status(&store, &conv_owned, &turn_owned, "failed", Some(&msg));
            }
        });

        Ok(json!({
            "turn_id": turn_id,
            "status": "running"
        }))
    }

    pub fn cancel(&self, store: &Store, conversation: &str) -> Reply {
        if let Ok(mut map) = self.running_pids.lock() {
            if let Some(pid) = map.remove(conversation) {
                kill_pid(pid);
            }
        }
        let turn: Option<String> = store
            .connect()?
            .query_row(
                "SELECT turn_id FROM claude_sessions WHERE conversation_id=?1",
                [conversation],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        if let Some(t) = turn {
            let _ = set_status(store, conversation, &t, "cancelled", None);
        }
        Ok(json!({ "cancelled": true }))
    }
}
