use crate::{codex, process::BoundedOutput, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

type Reply = Result<Value, String>;
const ACTIVE: &str = "('starting','running','waiting_approval','cancel_requested','stopping')";
const PROMPT_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const IDLE_SESSION_REAP_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PROMPT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const STDERR_TAIL_LIMIT: usize = 8192;
const BACKGROUND_TOOL_NOTICE_PREFIX: &str = "Gemini 本轮已返回，但仍有";

#[derive(Default)]
pub struct Runtime {
    clients: Arc<Mutex<HashMap<String, Arc<Client>>>>,
    operation: Arc<Mutex<()>>,
    cached_probe: Mutex<Option<Value>>,
    reaper_started: AtomicBool,
}

fn tool_status_is_terminal(status: Option<&str>) -> bool {
    let Some(raw) = status else {
        return false;
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "completed"
            | "succeeded"
            | "success"
            | "done"
            | "ok"
            | "failed"
            | "error"
            | "declined"
            | "cancelled"
            | "canceled"
            | "interrupted"
            | "denied"
    )
}

fn background_tools_notice(count: usize) -> String {
    format!(
        "{BACKGROUND_TOOL_NOTICE_PREFIX} {count} 条后台命令尚未结束；后台结果会自动更新，如需继续后续步骤请发送「继续」。"
    )
}

fn merge_tool_update(existing: Option<&str>, update: &Value) -> Result<String, String> {
    if let (Some(prev_raw), Some(patch)) = (existing, update.as_object()) {
        if let Ok(Value::Object(mut base)) = serde_json::from_str::<Value>(prev_raw) {
            for (key, value) in patch {
                if !value.is_null() {
                    base.insert(key.clone(), value.clone());
                }
            }
            return serde_json::to_string(&Value::Object(base)).map_err(|e| e.to_string());
        }
    }
    serde_json::to_string(update).map_err(|e| e.to_string())
}

fn append_stderr_tail(buf: &mut String, line: &str) {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(trimmed);
    if buf.len() > STDERR_TAIL_LIMIT {
        let mut cut = buf.len() - STDERR_TAIL_LIMIT;
        while cut < buf.len() && !buf.is_char_boundary(cut) {
            cut += 1;
        }
        buf.drain(..cut);
    }
}

fn sanitized_stderr_hint(tail: &str) -> Option<String> {
    tail.lines()
        .rev()
        .map(str::trim)
        .find(|line| {
            if line.is_empty() {
                return false;
            }
            let lower = line.to_ascii_lowercase();
            !(lower.contains("token")
                || lower.contains("api_key")
                || lower.contains("apikey")
                || lower.contains("authorization")
                || lower.contains("bearer ")
                || lower.contains("cookie")
                || lower.contains("secret")
                || lower.contains("password"))
        })
        .map(|line| {
            let mut out = String::new();
            for ch in line.chars().take(180) {
                out.push(ch);
            }
            out
        })
}

fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    crate::process::apply_system_proxy(&mut command);
    command
}

enum Launch {
    Agy(PathBuf),
    GeminiCli { node: PathBuf, entry: PathBuf },
}

struct Installation {
    launch: Launch,
    executable: PathBuf,
    version: String,
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
                    home.join(".gemini/antigravity/bin"),
                    home.join("Library/Application Support/Antigravity/bin"),
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
                for extra in [
                    home.join(".npm-global/bin"),
                    home.join(".volta/bin"),
                    home.join("Library/pnpm"),
                ] {
                    if extra.is_dir() {
                        paths.push(extra);
                    }
                }
            }
            for extra in [PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")] {
                if extra.is_dir() {
                    paths.push(extra);
                }
            }
        }
        // Prefer Antigravity CLI (`agy acp`) when installed.
        let agy_name = if cfg!(windows) { "agy.exe" } else { "agy" };
        for dir in &paths {
            let candidate = dir.join(agy_name);
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
            let version = String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_start_matches('v')
                .to_owned();
            if version.is_empty() {
                continue;
            }
            return Ok(Self {
                launch: Launch::Agy(executable.clone()),
                executable,
                version,
            });
        }
        let mut roots = Vec::new();
        // This is explicit local configuration, never a path supplied over HTTP.
        if let Some(root) = std::env::var_os("AI_GENT_GEMINI_PACKAGE") {
            roots.push(PathBuf::from(root));
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(roaming).join("npm/node_modules/@google/gemini-cli"));
        }
        for path in &paths {
            roots.push(path.join("node_modules/@google/gemini-cli"));
            roots.push(path.join("../lib/node_modules/@google/gemini-cli"));
            if let Ok(entry) = path.join("gemini").canonicalize() {
                for parent in entry.ancestors().skip(1) {
                    if parent.file_name().is_some_and(|name| name == "gemini-cli") {
                        roots.push(parent.to_owned());
                        break;
                    }
                }
            }
        }
        let node_name = if cfg!(windows) { "node.exe" } else { "node" };
        for root in roots {
            let Ok(root) = root.canonicalize() else {
                continue;
            };
            let manifest = root.join("package.json");
            if !manifest.metadata().is_ok_and(|m| m.len() < 1_000_000) {
                continue;
            }
            let Some(package) = std::fs::read(&manifest)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            else {
                continue;
            };
            if package["name"] != "@google/gemini-cli" {
                continue;
            }
            let Some(bin) = package["bin"]["gemini"]
                .as_str()
                .or_else(|| package["bin"].as_str())
            else {
                continue;
            };
            let Ok(entry) = root.join(bin).canonicalize() else {
                continue;
            };
            if !entry.starts_with(&root) || !entry.is_file() {
                continue;
            }
            let version = package["version"]
                .as_str()
                .ok_or("runtime_version_unsupported：Gemini CLI 缺少版本。")?
                .to_owned();
            semver::Version::parse(&version)
                .map_err(|_| "runtime_version_unsupported：Gemini CLI 版本无效。")?;
            let node=paths.iter().map(|p|p.join(node_name)).find(|p|p.is_file()).ok_or("runtime_not_found：Gemini CLI 需要现有 Node.js 运行组件，请自行安装并完成原厂登录。")?;
            let output = command(&node)
                .arg(&entry)
                .arg("--version")
                .bounded_output()
                .map_err(|_| "runtime_init_failed：无法读取 Gemini CLI 版本。")?;
            if !output.status.success()
                || !String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .trim_start_matches('v')
                    .eq(&version)
            {
                return Err(
                    "runtime_version_unsupported：Gemini CLI 运行版本与安装元数据不一致。".into(),
                );
            }
            return Ok(Self {
                executable: entry.clone(),
                launch: Launch::GeminiCli { node, entry },
                version,
            });
        }
        Err("runtime_not_found：未发现 Antigravity CLI (agy) 或 Gemini CLI。".into())
    }
}

struct Client {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    approvals: Mutex<HashMap<String, Value>>,
    running_tools: Mutex<HashMap<String, String>>,
    next: AtomicU64,
    healthy: AtomicBool,
    recording: AtomicBool,
    turn_active: AtomicBool,
    last_activity: Mutex<Instant>,
    stderr_tail: Arc<Mutex<String>>,
    database: PathBuf,
    root: String,
    conversation: String,
    session_id: Mutex<String>,
    turn_id: Mutex<String>,
    message_id: Mutex<Option<String>>,
    thought_id: Mutex<Option<String>>,
    capabilities: Mutex<Value>,
    current_model: Mutex<String>,
    cached_models: Mutex<Option<Vec<Value>>>,
    #[cfg(any(windows, unix))]
    job: codex::ProcessJob,
}

impl Client {
    fn touch_activity(&self) {
        if let Ok(mut guard) = self.last_activity.lock() {
            *guard = Instant::now();
        }
    }

    fn inactivity_elapsed(&self) -> Duration {
        self.last_activity
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    fn running_tools_for_turn(&self, turn: &str) -> usize {
        self.running_tools
            .lock()
            .map(|tools| tools.values().filter(|t| t.as_str() == turn).count())
            .unwrap_or(0)
    }

    fn has_any_running_tools(&self) -> bool {
        self.running_tools
            .lock()
            .map(|tools| !tools.is_empty())
            .unwrap_or(false)
    }

    fn is_idle_for_reap(&self) -> bool {
        !self.turn_active.load(Ordering::SeqCst)
            && !self.has_any_running_tools()
            && self.inactivity_elapsed() >= IDLE_SESSION_REAP_TIMEOUT
    }

    fn stderr_diagnostic(&self) -> Option<String> {
        let guard = self.stderr_tail.lock().ok()?;
        sanitized_stderr_hint(&guard)
    }

    fn start(
        store: &Store,
        install: &Installation,
        root: &str,
        conversation: &str,
    ) -> Result<Arc<Self>, String> {
        let mut cmd = match &install.launch {
            Launch::Agy(bin) => {
                let mut c = command(bin);
                c.args(["acp", "--dangerously-skip-permissions"]);
                c
            }
            Launch::GeminiCli { node, entry } => {
                let mut c = command(node);
                c.arg(entry).args(["--acp", "--approval-mode", "yolo"]);
                c
            }
        };
        let mut child = cmd
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "无法启动 Gemini / Antigravity ACP：请检查本机安装及权限。")?;
        #[cfg(any(windows, unix))]
        let job = match codex::ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let input = child.stdin.take().ok_or("Gemini stdin 不可用")?;
        let output = child.stdout.take().ok_or("Gemini stdout 不可用")?;
        let stderr = child.stderr.take().ok_or("Gemini stderr 不可用")?;
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let client = Arc::new(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            running_tools: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            recording: AtomicBool::new(false),
            turn_active: AtomicBool::new(false),
            last_activity: Mutex::new(Instant::now()),
            stderr_tail: stderr_tail.clone(),
            database: store.path.clone(),
            root: root.into(),
            conversation: conversation.into(),
            session_id: Mutex::new(String::new()),
            turn_id: Mutex::new(String::new()),
            message_id: Mutex::new(None),
            thought_id: Mutex::new(None),
            capabilities: Mutex::new(Value::Null),
            current_model: Mutex::new(String::new()),
            cached_models: Mutex::new(None),
            #[cfg(any(windows, unix))]
            job,
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Ok(mut buf) = stderr_tail.lock() {
                            append_stderr_tail(&mut buf, &line);
                        }
                    }
                }
            }
        });
        let weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                match (&mut reader).take(2_000_001).read_until(b'\n', &mut bytes) {
                    Ok(0) => break,
                    Ok(_) if bytes.len() <= 2_000_000 => {}
                    _ => break,
                }
                let Some(client) = weak.upgrade() else {
                    return;
                };
                let result = serde_json::from_slice::<Value>(&bytes)
                    .map_err(|_| "Gemini ACP 消息格式错误。".to_owned())
                    .and_then(|message| client.receive(message));
                if result.is_err() {
                    break;
                }
            }
            if let Some(client) = weak.upgrade() {
                let reason = match client.stderr_diagnostic() {
                    Some(hint) => format!("Gemini 连接已断开，未完成执行的结果未知。（诊断：{hint}）"),
                    None => "Gemini 连接已断开，未完成执行的结果未知。".to_owned(),
                };
                client.disconnect(&reason);
                let _ = client.stop();
            }
        });
        let result=client.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"ai-gent","version":env!("CARGO_PKG_VERSION")}}),Duration::from_secs(15));
        match result {
            Ok(info)
                if info["protocolVersion"] == 1
                    && info["agentCapabilities"]["loadSession"] == true
                    && match &install.launch {
                        Launch::Agy(_) => info["agentInfo"]["name"]
                            .as_str()
                            .is_some_and(|n| n.to_lowercase().contains("antigravity")),
                        Launch::GeminiCli { .. } => {
                            info["agentInfo"]["name"] == "gemini-cli"
                                && info["agentInfo"]["version"] == install.version
                        }
                    } =>
            {
                *client.capabilities.lock().map_err(|e| e.to_string())? =
                    info["agentCapabilities"].clone();
                Ok(client)
            }
            _ => {
                let _ = client.stop();
                Err("runtime_identity_mismatch：组件不是兼容的 Antigravity / Gemini ACP v1，或版本/身份不一致。未调用模型。".into())
            }
        }
    }
    fn write(&self, value: Value) -> Result<(), String> {
        let mut input = self.input.lock().map_err(|e| e.to_string())?;
        writeln!(input, "{value}")
            .and_then(|_| input.flush())
            .map_err(|_| "Gemini ACP 写入失败，执行结果未知。".into())
    }
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        if !self.healthy.load(Ordering::SeqCst) {
            return Err("Gemini ACP 已断开。".into());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id, send);
        let result = self
            .write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .and_then(|_| {
                receive
                    .recv_timeout(timeout)
                    .map_err(|_| format!("Gemini {method} 未确认；请核对执行状态。"))?
            });
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if result.is_err() {
            let _ = self.stop();
        }
        result
    }
    fn receive(&self, message: Value) -> Result<(), String> {
        self.touch_activity();
        if message["method"].is_null() {
            if let Some(id) = message["id"].as_u64() {
                if let Some(sender) = self.pending.lock().map_err(|e| e.to_string())?.remove(&id) {
                    // Raw RPC error data may contain authentication details. Do not expose it.
                    let result = if message.get("error").is_some() {
                        let detail = message["error"]["message"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase();
                        let code = message["error"]["code"].as_i64().unwrap_or(-32000);
                        let description = if detail.contains("quota")
                            || detail.contains("resource_exhausted")
                            || detail.contains("429")
                        {
                            "quota_exhausted：Gemini 原厂额度不足或限流，请在原厂处理后重试。"
                        } else if detail.contains("auth")
                            || detail.contains("api key")
                            || detail.contains("credential")
                            || detail.contains("login")
                        {
                            "authentication_failed：Gemini 原厂认证失败，请先在 Gemini CLI 完成登录或凭证配置。"
                        } else if matches!(code, -32601 | -32602) {
                            "runtime_version_unsupported：Gemini CLI 不支持当前 ACP 方法或字段，未自动更换组件或模型。"
                        } else {
                            "runtime_init_failed：Gemini RPC 返回错误，请核对原厂运行状态。"
                        };
                        Err(format!("{description}（RPC 代码 {code}；原始诊断未保存）"))
                    } else {
                        Ok(message["result"].clone())
                    };
                    let _ = sender.send(result);
                }
            }
            return Ok(());
        }
        let method = message["method"].as_str().unwrap_or("");
        let p = &message["params"];
        if message.get("id").is_some() {
            if method == "session/request_permission"
                && self.recording.load(Ordering::SeqCst)
                && p["sessionId"].as_str()
                    == Some(self.session_id.lock().map_err(|e| e.to_string())?.as_str())
            {
                if let Some(option) = p["options"]
                    .as_array()
                    .and_then(|items| {
                        items
                            .iter()
                            .find(|option| option["kind"] == "allow_always")
                            .or_else(|| items.iter().find(|option| option["kind"] == "allow_once"))
                    })
                    .and_then(|option| option["optionId"].as_str())
                {
                    return self.write(json!({
                        "jsonrpc": "2.0",
                        "id": message["id"],
                        "result": { "outcome": { "outcome": "selected", "optionId": option } }
                    }));
                }
                let key = uuid::Uuid::new_v4().to_string();
                self.approvals
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(key, message);
                set_status(
                    &Store {
                        path: self.database.clone(),
                    },
                    &self.conversation,
                    &self.turn_id.lock().map_err(|e| e.to_string())?,
                    "waiting_approval",
                    None,
                )?;
                return Ok(());
            }
            return self.write(json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Client capability not supported; request denied"}}));
        }
        if method != "session/update" {
            return Ok(());
        }
        if p["sessionId"].as_str()
            != Some(self.session_id.lock().map_err(|e| e.to_string())?.as_str())
        {
            return Ok(());
        }
        let update = &p["update"];
        let update_kind = update["sessionUpdate"].as_str();
        let store = Store {
            path: self.database.clone(),
        };
        let connection = store.connect()?;
        let logical_cid = logical_conversation(&connection, &self.conversation);

        // Accept late tool_call_update notifications for background tools that were still running
        // when `session/prompt` returned `end_turn` (common in `agy acp` background commands).
        if !self.recording.load(Ordering::SeqCst) {
            if matches!(update_kind, Some("tool_call" | "tool_call_update")) {
                if let Some(tool_call_id) = update["toolCallId"].as_str() {
                    let tracked_turn = self
                        .running_tools
                        .lock()
                        .map_err(|e| e.to_string())?
                        .get(tool_call_id)
                        .cloned();
                    if let Some(orig_turn) = tracked_turn {
                        let id = format!("gemini:{orig_turn}:{tool_call_id}");
                        let existing: Option<String> = connection
                            .query_row(
                                "SELECT text FROM codex_messages WHERE conversation_id=?1 AND item_id=?2",
                                params![self.conversation, id.as_str()],
                                |r| r.get(0),
                            )
                            .optional()
                            .map_err(|e| e.to_string())?;
                        let text = merge_tool_update(existing.as_deref(), update)?;
                        connection.execute(
                            "INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'tool',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text",
                            params![self.conversation, id.as_str(), &text],
                        ).map_err(|e| e.to_string())?;
                        crate::events::emit_delta(&logical_cid, id, "tool", Some("gemini"), "replace", text);
                        if tool_status_is_terminal(update["status"].as_str()) {
                            let remaining = {
                                let mut tools = self.running_tools.lock().map_err(|e| e.to_string())?;
                                tools.remove(tool_call_id);
                                tools.values().filter(|t| t.as_str() == orig_turn.as_str()).count()
                            };
                            let next_error = if remaining == 0 {
                                None
                            } else {
                                Some(background_tools_notice(remaining))
                            };
                            let _ = connection.execute(
                                "UPDATE gemini_sessions SET error=?3 WHERE conversation_id=?1 AND turn_id=?2 AND status='completed' AND error LIKE ?4",
                                params![
                                    self.conversation,
                                    orig_turn,
                                    next_error.as_deref(),
                                    format!("{BACKGROUND_TOOL_NOTICE_PREFIX}%")
                                ],
                            );
                        }
                        crate::events::emit_conversation_sync(&logical_cid);
                    }
                }
            }
            return Ok(());
        }

        let turn = self.turn_id.lock().map_err(|e| e.to_string())?.clone();
        let active:bool=connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id=?1 AND turn_id=?2 AND status IN {ACTIVE})"),params![self.conversation,turn],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !active {
            return Ok(());
        }
        match update_kind {
            Some("agent_thought_chunk") => {
                let text = update["content"]["text"]
                    .as_str()
                    .or_else(|| update["text"].as_str())
                    .unwrap_or("");
                if !text.is_empty() {
                    *self.message_id.lock().map_err(|e| e.to_string())? = None;
                    let mut item = self.thought_id.lock().map_err(|e| e.to_string())?;
                    let id = item.get_or_insert_with(|| format!("gemini:thought:{}", uuid::Uuid::new_v4()));
                    connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'thinking',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text||excluded.text",params![self.conversation,id.as_str(),text]).map_err(|e|e.to_string())?;
                    crate::events::emit_delta(&logical_cid, id.as_str(), "thinking", Some("gemini"), "append", text);
                }
            }
            Some("agent_message_chunk") if update["content"]["type"] == "text" => {
                *self.thought_id.lock().map_err(|e| e.to_string())? = None;
                let text = update["content"]["text"].as_str().unwrap_or("");
                let mut item = self.message_id.lock().map_err(|e| e.to_string())?;
                let id = item.get_or_insert_with(|| format!("gemini:{}", uuid::Uuid::new_v4()));
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'assistant',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text||excluded.text",params![self.conversation,id.as_str(),text]).map_err(|e|e.to_string())?;
                crate::events::emit_delta(&logical_cid, id.as_str(), "assistant", Some("gemini"), "append", text);
            }
            Some("tool_call" | "tool_call_update") => {
                *self.message_id.lock().map_err(|e| e.to_string())? = None;
                *self.thought_id.lock().map_err(|e| e.to_string())? = None;
                let tool_call_id = update["toolCallId"]
                    .as_str()
                    .ok_or("Gemini 工具事件缺少标识")?;
                {
                    let mut tools = self.running_tools.lock().map_err(|e| e.to_string())?;
                    if tool_status_is_terminal(update["status"].as_str()) {
                        tools.remove(tool_call_id);
                    } else {
                        tools.insert(tool_call_id.to_owned(), turn.clone());
                    }
                }
                let id = format!("gemini:{turn}:{tool_call_id}");
                let existing: Option<String> = connection
                    .query_row(
                        "SELECT text FROM codex_messages WHERE conversation_id=?1 AND item_id=?2",
                        params![self.conversation, id.as_str()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                let text = merge_tool_update(existing.as_deref(), update)?;
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'tool',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text",params![self.conversation,id.as_str(),&text]).map_err(|e|e.to_string())?;
                crate::events::emit_delta(&logical_cid, id, "tool", Some("gemini"), "replace", text);
            }
            Some("plan") => {
                *self.message_id.lock().map_err(|e| e.to_string())? = None;
                *self.thought_id.lock().map_err(|e| e.to_string())? = None;
                let id = format!("gemini:{turn}:plan");
                let text = serde_json::to_string(update).map_err(|e| e.to_string())?;
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'tool',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text",params![self.conversation,id.as_str(),&text]).map_err(|e|e.to_string())?;
                crate::events::emit_delta(&logical_cid, id, "tool", Some("gemini"), "replace", text);
            }
            _ => {}
        }
        Ok(())
    }
    fn metadata(&self, root: &str, previous: Option<&str>) -> Reply {
        if previous.is_some()
            && self.capabilities.lock().map_err(|e| e.to_string())?["loadSession"] != true
        {
            return Err("Gemini 组件未声明会话恢复能力。".into());
        }
        let (method, params) = if let Some(id) = previous {
            (
                "session/load",
                json!({"sessionId":id,"cwd":root,"mcpServers":[]}),
            )
        } else {
            ("session/new", json!({"cwd":root,"mcpServers":[]}))
        };
        // Metadata only: never send session/prompt while checking or listing models.
        let info = self.request(method, params, Duration::from_secs(20))?;
        let id = previous
            .or_else(|| info["sessionId"].as_str())
            .ok_or("Gemini 未返回会话 ID。")?;
        *self.session_id.lock().map_err(|e| e.to_string())? = id.to_owned();
        Ok(info)
    }
    fn stop(&self) -> Result<(), String> {
        self.recording.store(false, Ordering::SeqCst);
        self.turn_active.store(false, Ordering::SeqCst);
        #[cfg(any(windows, unix))]
        self.job.terminate()?;
        let mut child = self.child.lock().map_err(|e| e.to_string())?;
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            let _ = child.kill();
            child.wait().map_err(|e| e.to_string())?;
        }
        self.disconnect("Gemini 后台已停止，未确认的执行结果未知。");
        Ok(())
    }
    fn disconnect(&self, reason: &str) {
        if !self.healthy.swap(false, Ordering::SeqCst) {
            return;
        }
        self.recording.store(false, Ordering::SeqCst);
        self.turn_active.store(false, Ordering::SeqCst);
        let turn = self
            .turn_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let _ = set_status(
            &Store {
                path: self.database.clone(),
            },
            &self.conversation,
            &turn,
            "interrupted",
            Some(reason),
        );
        self.approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.running_tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        for (_, sender) in self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            let _ = sender.send(Err(reason.to_owned()));
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn models(info: &Value) -> Result<Vec<Value>, String> {
    let rows = info["models"]["availableModels"]
        .as_array()
        .ok_or("runtime_version_unsupported：Gemini ACP 未提供实际模型目录。")?;
    let mut result = Vec::new();
    for model in rows {
        let id = model["modelId"].as_str().ok_or("Gemini 模型标识无效")?;
        result.push(json!({"model":id,"displayName":model["name"].as_str().unwrap_or(id),"isDefault":info["models"]["currentModelId"]==id}));
    }
    if result.is_empty() {
        return Err("Gemini 返回的模型目录为空。".into());
    }
    Ok(result)
}
pub fn session(store: &Store, conversation: &str) -> Reply {
    store.connect()?.query_row("SELECT thread_id,turn_id,model,runtime_version,workspace_path,status,error FROM gemini_sessions WHERE conversation_id=?1",[conversation],|r|Ok(json!({"thread_id":r.get::<_,String>(0)?,"turn_id":r.get::<_,Option<String>>(1)?,"model":r.get::<_,String>(2)?,"runtime_version":r.get::<_,String>(3)?,"workspace_path":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?,"error":r.get::<_,Option<String>>(6)?}))).optional().map(|s|s.unwrap_or(Value::Null)).map_err(|e|e.to_string())
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
    let changed=tx.execute(&format!("UPDATE gemini_sessions SET status=?3,error=?4 WHERE conversation_id=?1 AND turn_id=?2 AND status IN {ACTIVE}"),params![conversation,turn,status,error]).map_err(|e|e.to_string())?;
    if changed > 0 && matches!(status, "completed" | "failed" | "interrupted" | "cancelled") {
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'task_end',?3,'gemini')",params![conversation,format!("gemini:end:{turn}"),status]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if changed > 0 {
        if matches!(status, "completed" | "failed" | "interrupted" | "cancelled") {
            crate::git::finish_turn_snapshot(store, conversation, "gemini");
        }
        crate::events::emit_conversation_sync(&logical_cid);
    }
    Ok(())
}
pub fn recover(store: &Store) -> Result<(), String> {
    store.connect()?.execute(&format!("UPDATE gemini_sessions SET status='interrupted',error='应用已重启，上次 Gemini 执行结果未知；请核对后处理。' WHERE status IN {ACTIVE}"),[]).map_err(|e|e.to_string())?;
    Ok(())
}

impl Runtime {
    fn ensure_reaper(&self) {
        if self.reaper_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let clients = self.clients.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let idle: Vec<Arc<Client>> = match clients.lock() {
                Ok(mut map) => {
                    let keys: Vec<String> = map
                        .iter()
                        .filter(|(_, client)| client.is_idle_for_reap())
                        .map(|(k, _)| k.clone())
                        .collect();
                    keys.into_iter().filter_map(|k| map.remove(&k)).collect()
                }
                Err(_) => Vec::new(),
            };
            for client in idle {
                let _ = client.stop();
            }
        });
    }

    pub fn probe(&self, store: &Store) -> Reply {
        self.ensure_reaper();
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = self.cached_probe.lock().map_err(|e| e.to_string())?.as_ref() {
            if cached["auth"].is_string() {
                return Ok(cached.clone());
            }
        }
        let install = Installation::find()?;
        let root = store
            .path
            .parent()
            .ok_or("数据目录无效")?
            .join("runtimes/gemini/inspection");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let client = Client::start(
            store,
            &install,
            root.to_str().ok_or("Gemini 工作目录无效")?,
            "",
        )?;
        let result: Reply = (|| {
            let caps = json!({"resume":client.capabilities.lock().map_err(|e|e.to_string())?["loadSession"]==true,"cancel":true,"approval":true,"tools":true,"workspace_isolation":false});
            let auth_label = match &install.launch {
                Launch::Agy(_) => "Antigravity (agy) 原厂登录",
                Launch::GeminiCli { .. } => "Gemini CLI 原厂登录（模型额度在真实执行时确认）",
            };
            match client.metadata(root.to_str().ok_or("Gemini 工作目录无效")?, None) {
                Ok(info) => {
                    let models = models(&info)?;
                    Ok(json!({"version":install.version,"executable":install.executable,"runtime_kind":"gemini-cli-acp","auth":auth_label,"requires_auth":true,"models":models,"sidebar_warning":null,"capabilities":caps}))
                }
                Err(err) if err.starts_with("authentication_failed") => Ok(
                    json!({"version":install.version,"executable":install.executable,"runtime_kind":"gemini-cli-acp","auth":Value::Null,"requires_auth":true,"models":[],"sidebar_warning":null,"capabilities":caps}),
                ),
                Err(err) => Err(err),
            }
        })();
        let stopped = client.stop();
        let result = result?;
        stopped?;
        if result["auth"].is_string() {
            *self.cached_probe.lock().map_err(|e| e.to_string())? = Some(result.clone());
        }
        Ok(result)
    }
    pub fn send_recorded(
        &self,
        store: &Store,
        conversation: &str,
        model: &str,
        text: &str,
        role: &str,
    ) -> Reply {
        self.ensure_reaper();
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        if text.trim().is_empty() || text.len() > 100_000 || model.is_empty() {
            return Err("请选择 Gemini 模型，并填写有效消息。".into());
        }
        let busy: bool = store
            .connect()?
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id=?1 AND status IN {ACTIVE})"),
                [conversation],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if busy {
            return Err("当前 Gemini 会话正在执行，请等待或取消。".into());
        }
        let root = PathBuf::from(store.conversation_root(conversation)?)
            .canonicalize()
            .map_err(|e| format!("Gemini 工作目录不可用：{e}"))?;
        let root = root.to_str().ok_or("Gemini 工作目录无效")?;
        let install = Installation::find()?;
        let previous = session(store, conversation)?;
        let existing_thread = if role == "assignment" {
            None
        } else {
            previous["thread_id"]
                .as_str()
                .filter(|id| !id.is_empty() && !id.starts_with("pending:"))
                .map(str::to_owned)
        };
        if existing_thread.is_some() && previous["workspace_path"] != root {
            return Err("Gemini 工作目录与历史不一致，未恢复错误会话。".into());
        }

        let turn = uuid::Uuid::new_v4().to_string();
        let placeholder_thread = existing_thread
            .clone()
            .unwrap_or_else(|| format!("pending:{turn}"));
        let mut connection = store.connect()?;
        let logical_cid = logical_conversation(&connection, conversation);
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO gemini_sessions(conversation_id,thread_id,turn_id,model,runtime_version,workspace_path,status) VALUES(?1,?2,?3,?4,?5,?6,'starting') ON CONFLICT(conversation_id) DO UPDATE SET thread_id=excluded.thread_id,turn_id=excluded.turn_id,model=excluded.model,runtime_version=excluded.runtime_version,workspace_path=excluded.workspace_path,status='starting',error=NULL",params![conversation,placeholder_thread,turn,model,install.version,root]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'gemini')",params![conversation,format!("gemini:input:{turn}"),role,text]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'task_start','running','gemini')",params![conversation,format!("gemini:start:{turn}")]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        crate::events::emit_conversation_sync(&logical_cid);
        drop(_operation);

        let client_slots = self.clients.clone();
        let operation_lock = self.operation.clone();
        let db_path = store.path.clone();
        let conversation_owned = conversation.to_owned();
        let model_owned = model.to_owned();
        let text_owned = text.to_owned();
        let role_owned = role.to_owned();
        let root_owned = root.to_owned();
        let turn_bg = turn.clone();
        std::thread::spawn(move || {
            let store = Store { path: db_path };
            let Ok(_operation) = operation_lock.lock() else {
                let _ = set_status(&store, &conversation_owned, &turn_bg, "failed", Some("Gemini 运行锁不可用。"));
                return;
            };
            let still_starting = session(&store, &conversation_owned)
                .ok()
                .is_some_and(|s| s["turn_id"].as_str() == Some(turn_bg.as_str()) && s["status"] == "starting");
            if !still_starting {
                return;
            }
            crate::git::begin_turn_snapshot(&conversation_owned, "gemini", &root_owned);

            let launch = (|| -> Result<(Arc<Client>, String, mpsc::Receiver<Reply>), String> {
                // Reuse the live ACP process when workspace and conversation match.
                let existing_client = client_slots
                    .lock()
                    .map_err(|e| e.to_string())?
                    .get(&conversation_owned)
                    .cloned();
                let reusable = existing_client.filter(|c| {
                    c.healthy.load(Ordering::SeqCst)
                        && c.root == root_owned
                        && c.conversation == conversation_owned
                        && role_owned != "assignment"
                        && !c.session_id.lock().map(|s| s.is_empty()).unwrap_or(true)
                });

                let (client, id) = if let Some(client) = reusable {
                    let id = client.session_id.lock().map_err(|e| e.to_string())?.clone();
                    if let Some(cached) = client.cached_models.lock().map_err(|e| e.to_string())?.as_ref() {
                        if !cached.iter().any(|item| item["model"] == model_owned) {
                            return Err("Gemini 实际目录中没有所选模型，未切换到其他模型。".into());
                        }
                    }
                    let current = client.current_model.lock().map_err(|e| e.to_string())?.clone();
                    if current != model_owned {
                        client.request(
                            "session/set_model",
                            json!({"sessionId":id,"modelId":model_owned}),
                            Duration::from_secs(15),
                        )?;
                        *client.current_model.lock().map_err(|e| e.to_string())? = model_owned.clone();
                    }
                    (client, id)
                } else {
                    if let Some(old) = client_slots
                        .lock()
                        .map_err(|e| e.to_string())?
                        .remove(&conversation_owned)
                    {
                        let _ = old.stop();
                    }
                    let previous_id = if role_owned == "assignment" {
                        None
                    } else {
                        existing_thread.as_deref()
                    };
                    let client = Client::start(&store, &install, &root_owned, &conversation_owned)?;
                    let setup = (|| -> Reply {
                        let info = client.metadata(&root_owned, previous_id)?;
                        let available = models(&info)?;
                        if !available.iter().any(|item| item["model"] == model_owned) {
                            return Err("Gemini 实际目录中没有所选模型，未切换到其他模型。".into());
                        }
                        *client.cached_models.lock().map_err(|e| e.to_string())? = Some(available);
                        let id = client.session_id.lock().map_err(|e| e.to_string())?.clone();
                        let current_model = info["models"]["currentModelId"]
                            .as_str()
                            .unwrap_or("")
                            .to_owned();
                        if current_model != model_owned {
                            client.request(
                                "session/set_model",
                                json!({"sessionId":id,"modelId":model_owned}),
                                Duration::from_secs(15),
                            )?;
                        }
                        *client.current_model.lock().map_err(|e| e.to_string())? = model_owned.clone();
                        Ok(json!(id))
                    })();
                    let id = match setup {
                        Ok(id) => id.as_str().ok_or("Gemini 会话无效")?.to_owned(),
                        Err(error) => {
                            let _ = client.stop();
                            return Err(error);
                        }
                    };
                    (client, id)
                };

                *client.turn_id.lock().map_err(|e| e.to_string())? = turn_bg.clone();
                *client.message_id.lock().map_err(|e| e.to_string())? = None;
                *client.thought_id.lock().map_err(|e| e.to_string())? = None;
                store
                    .connect()?
                    .execute(
                        "UPDATE gemini_sessions SET thread_id=?3,status='running' WHERE conversation_id=?1 AND turn_id=?2 AND status='starting'",
                        params![conversation_owned, turn_bg, id],
                    )
                    .map_err(|e| e.to_string())?;
                client.recording.store(true, Ordering::SeqCst);
                client.turn_active.store(true, Ordering::SeqCst);
                client.touch_activity();
                // Save the prompt correlation before writing; completion comes only from its real RPC response.
                let request_id = client.next.fetch_add(1, Ordering::Relaxed);
                let (send, receive) = mpsc::channel();
                client
                    .pending
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(request_id, send);
                client_slots
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(conversation_owned.clone(), client.clone());
                let mut prompt_blocks = vec![json!({"type":"text","text":text_owned})];
                for attachment_id in crate::attachments::extract_ids(&text_owned) {
                    if let Ok((mime, data)) = crate::attachments::read_base64(&store, &attachment_id) {
                        if mime.starts_with("image/") {
                            prompt_blocks.push(json!({"type":"image","data":data,"mimeType":mime}));
                        }
                    }
                }
                if let Err(error) = client.write(json!({"jsonrpc":"2.0","id":request_id,"method":"session/prompt","params":{"sessionId":id,"prompt":prompt_blocks}})) {
                    let _ = client.stop();
                    return Err(error);
                }
                Ok((client, id, receive))
            })();

            let (client, _id, receive) = match launch {
                Ok(parts) => parts,
                Err(error) => {
                    let _ = set_status(&store, &conversation_owned, &turn_bg, "failed", Some(&error));
                    return;
                }
            };
            drop(_operation);

            // Inactivity watchdog modeled after T3 Code's 15-minute prompt stall guard.
            let mut stalled = false;
            let outcome: Result<Reply, mpsc::RecvTimeoutError> = loop {
                match receive.recv_timeout(PROMPT_POLL_INTERVAL) {
                    Ok(reply) => break Ok(reply),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break Err(mpsc::RecvTimeoutError::Disconnected);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let waiting_approval = client
                            .approvals
                            .lock()
                            .map(|a| !a.is_empty())
                            .unwrap_or(false);
                        if !waiting_approval
                            && client.inactivity_elapsed() >= PROMPT_INACTIVITY_TIMEOUT
                        {
                            stalled = true;
                            break Err(mpsc::RecvTimeoutError::Timeout);
                        }
                    }
                }
            };
            let (usage_inp, usage_out, usage_cached) = match &outcome {
                Ok(Ok(result)) => {
                    let u = &result["usage"];
                    (
                        u["inputTokens"].as_i64().unwrap_or(0),
                        u["outputTokens"].as_i64().unwrap_or(0),
                        u["cachedReadTokens"].as_i64().unwrap_or(0),
                    )
                }
                _ => (0, 0, 0),
            };
            let (status, reason): (&str, Option<String>) = match outcome {
                Ok(Ok(result)) if result["stopReason"] == "end_turn" => {
                    let pending_bg = client.running_tools_for_turn(&turn_bg);
                    if pending_bg > 0 {
                        ("completed", Some(background_tools_notice(pending_bg)))
                    } else {
                        ("completed", None)
                    }
                }
                Ok(Ok(result)) if result["stopReason"] == "cancelled" => ("cancelled", None),
                Ok(Ok(result)) => (
                    "failed",
                    Some(format!(
                        "Gemini 本轮终态：{}；未标为正常完成。",
                        result["stopReason"]
                    )),
                ),
                Ok(Err(error)) => ("failed", Some(error)),
                Err(mpsc::RecvTimeoutError::Timeout) if stalled => (
                    "interrupted",
                    Some("Gemini 连续 15 分钟无任何输出，已自动停止当前轮次，请核对文件后继续。".into()),
                ),
                _ => (
                    "interrupted",
                    Some("Gemini 执行未得到可确认的完成结果，请核对文件后处理。".into()),
                ),
            };
            let usage_status = if reason
                .as_deref()
                .is_some_and(|r| r.contains("quota_exhausted"))
            {
                "quota_exhausted"
            } else {
                status
            };
            crate::usage::record(
                &store.path,
                "gemini",
                "gemini",
                &model_owned,
                &conversation_owned,
                usage_inp,
                usage_out,
                usage_cached,
                usage_status,
            );
            let _ = set_status(
                &store,
                &conversation_owned,
                &turn_bg,
                status,
                reason.as_deref(),
            );
            client.recording.store(false, Ordering::SeqCst);
            client.turn_active.store(false, Ordering::SeqCst);
            client.touch_activity();
            *client
                .message_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            *client
                .thought_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            client
                .approvals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            if status != "completed" {
                let _ = client.stop();
                if let Ok(mut map) = client_slots.lock() {
                    if map
                        .get(&conversation_owned)
                        .is_some_and(|current| Arc::ptr_eq(current, &client))
                    {
                        map.remove(&conversation_owned);
                    }
                }
            }
        });
        Ok(json!({"turn_id": turn, "status": "starting"}))
    }
    pub fn approvals(&self, conversation: &str) -> Result<Vec<Value>, String> {
        let slots = self.clients.lock().map_err(|e| e.to_string())?;
        let Some(client) = slots.get(conversation) else {
            return Ok(Vec::new());
        };
        let requests = client.approvals.lock().map_err(|e| e.to_string())?;
        Ok(requests.iter().map(|(id,request)|json!({"id":id,"method":"session/request_permission","command":request["params"]["toolCall"]["title"],"reason":serde_json::to_string(&request["params"]["toolCall"]).unwrap_or_default(),"cwd":null})).collect())
    }
    pub fn approve(&self, store: &Store, conversation: &str, id: &str, allow: bool) -> Reply {
        let slots = self.clients.lock().map_err(|e| e.to_string())?;
        let client = slots
            .get(conversation)
            .filter(|c| c.recording.load(Ordering::SeqCst))
            .ok_or("Gemini 审批已过期。")?;
        let mut requests = client.approvals.lock().map_err(|e| e.to_string())?;
        let request = requests.get(id).ok_or("Gemini 审批不存在或已处理。")?;
        let outcome = if allow {
            let option = request["params"]["options"]
                .as_array()
                .and_then(|items| items.iter().find(|option| option["kind"] == "allow_once"))
                .and_then(|option| option["optionId"].as_str())
                .ok_or("Gemini 未提供单次批准选项，未扩大授权。")?;
            json!({"outcome":"selected","optionId":option})
        } else {
            json!({"outcome":"cancelled"})
        };
        client.write(json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":outcome}}))?;
        requests.remove(id);
        if requests.is_empty() {
            set_status(
                store,
                conversation,
                &client.turn_id.lock().map_err(|e| e.to_string())?,
                "running",
                None,
            )?;
        }
        Ok(Value::Null)
    }
    pub fn cancel(&self, store: &Store, conversation: &str) -> Reply {
        self.ensure_reaper();
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let slots = self.clients.lock().map_err(|e| e.to_string())?;
        let client = slots
            .get(conversation)
            .cloned()
            .ok_or("Gemini 当前会话进程已断开；不能确认已取消。")?;
        drop(slots);
        let turn = client.turn_id.lock().map_err(|e| e.to_string())?.clone();
        if !session(store, conversation)?["status"]
            .as_str()
            .is_some_and(codex::active)
        {
            return Err("当前没有正在执行的 Gemini 任务。".into());
        }
        set_status(store, conversation, &turn, "cancel_requested", None)?;
        let id = client.session_id.lock().map_err(|e| e.to_string())?.clone();
        let _ = client
            .write(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":id}}));
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && session(store, conversation)?["status"]
                .as_str()
                .is_some_and(codex::active)
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        let terminal = session(store, conversation)?["status"].clone();
        client.stop()?;
        if let Ok(mut map) = self.clients.lock() {
            if map
                .get(conversation)
                .is_some_and(|current| Arc::ptr_eq(current, &client))
            {
                map.remove(conversation);
            }
        }
        if terminal == "cancelled" {
            Ok(json!({"cancelled":true}))
        } else if terminal == "completed" {
            Ok(json!({"cancelled":false,"completed":true}))
        } else {
            Err("Gemini 未确认取消终态，已清理应用管理的进程组；执行结果仍需核对。".into())
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut slots) = self.clients.lock() {
            for (_, client) in slots.drain() {
                let _ = client.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_terminal_classification() {
        assert!(!tool_status_is_terminal(None));
        assert!(!tool_status_is_terminal(Some("in_progress")));
        assert!(!tool_status_is_terminal(Some("pending")));
        assert!(!tool_status_is_terminal(Some("running")));
        assert!(tool_status_is_terminal(Some("completed")));
        assert!(tool_status_is_terminal(Some("FAILED")));
        assert!(tool_status_is_terminal(Some("cancelled")));
    }

    #[test]
    fn merge_tool_update_preserves_initial_fields() {
        let initial = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call_1",
            "kind": "execute",
            "title": "python3 cleanup.py",
            "status": "in_progress"
        })
        .to_string();
        let patch = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call_1",
            "status": "completed",
            "rawOutput": "done"
        });
        let merged_str = merge_tool_update(Some(&initial), &patch).unwrap();
        let merged: Value = serde_json::from_str(&merged_str).unwrap();
        assert_eq!(merged["title"], "python3 cleanup.py");
        assert_eq!(merged["kind"], "execute");
        assert_eq!(merged["status"], "completed");
        assert_eq!(merged["rawOutput"], "done");
    }

    #[test]
    fn stderr_tail_bounds_and_redacts_secrets() {
        let mut buf = String::new();
        for _ in 0..300 {
            append_stderr_tail(&mut buf, "some long stderr diagnostic line 0123456789abcdef");
        }
        assert!(buf.len() <= STDERR_TAIL_LIMIT);
        append_stderr_tail(&mut buf, "fatal: child exited unexpectedly");
        append_stderr_tail(&mut buf, "Authorization: Bearer secret-token-123");
        assert_eq!(
            sanitized_stderr_hint(&buf).as_deref(),
            Some("fatal: child exited unexpectedly")
        );
    }
}
