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

#[derive(Default)]
pub struct Runtime {
    client: Mutex<Option<Arc<Client>>>,
    operation: Mutex<()>,
    cached_probe: Mutex<Option<Value>>,
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
    next: AtomicU64,
    healthy: AtomicBool,
    recording: AtomicBool,
    database: PathBuf,
    root: String,
    conversation: String,
    session_id: Mutex<String>,
    turn_id: Mutex<String>,
    message_id: Mutex<Option<String>>,
    capabilities: Mutex<Value>,
    current_model: Mutex<String>,
    cached_models: Mutex<Option<Vec<Value>>>,
    #[cfg(any(windows, unix))]
    job: codex::ProcessJob,
}

impl Client {
    fn start(
        store: &Store,
        install: &Installation,
        root: &str,
        conversation: &str,
    ) -> Result<Arc<Self>, String> {
        let mut cmd = match &install.launch {
            Launch::Agy(bin) => {
                let mut c = command(bin);
                c.arg("acp");
                c
            }
            Launch::GeminiCli { node, entry } => {
                let mut c = command(node);
                c.arg(entry).args(["--acp", "--approval-mode", "default"]);
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
        let client = Arc::new(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            recording: AtomicBool::new(false),
            database: store.path.clone(),
            root: root.into(),
            conversation: conversation.into(),
            session_id: Mutex::new(String::new()),
            turn_id: Mutex::new(String::new()),
            message_id: Mutex::new(None),
            capabilities: Mutex::new(Value::Null),
            current_model: Mutex::new(String::new()),
            cached_models: Mutex::new(None),
            #[cfg(any(windows, unix))]
            job,
        });
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
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
                client.disconnect("Gemini 连接已断开，未完成执行的结果未知。");
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
        let update = &p["update"];
        if !self.recording.load(Ordering::SeqCst)
            || p["sessionId"].as_str()
                != Some(self.session_id.lock().map_err(|e| e.to_string())?.as_str())
        {
            return Ok(());
        }
        let store = Store {
            path: self.database.clone(),
        };
        let turn = self.turn_id.lock().map_err(|e| e.to_string())?.clone();
        let connection = store.connect()?;
        let active:bool=connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id=?1 AND turn_id=?2 AND status IN {ACTIVE})"),params![self.conversation,turn],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !active {
            return Ok(());
        }
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") if update["content"]["type"] == "text" => {
                let text = update["content"]["text"].as_str().unwrap_or("");
                let mut item = self.message_id.lock().map_err(|e| e.to_string())?;
                let id = item.get_or_insert_with(|| format!("gemini:{}", uuid::Uuid::new_v4()));
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'assistant',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text||excluded.text",params![self.conversation,id.as_str(),text]).map_err(|e|e.to_string())?;
            }
            Some("tool_call" | "tool_call_update") => {
                *self.message_id.lock().map_err(|e| e.to_string())? = None;
                let id = format!(
                    "gemini:{turn}:{}",
                    update["toolCallId"]
                        .as_str()
                        .ok_or("Gemini 工具事件缺少标识")?
                );
                let text = serde_json::to_string(update).map_err(|e| e.to_string())?;
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'tool',?3,'gemini') ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text",params![self.conversation,id.as_str(),text]).map_err(|e|e.to_string())?;
            }
            Some("current_mode_update") if update["currentModeId"] != "default" => {
                return Err("Gemini 权限模式发生变化，已停止执行。".into());
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
        if info["modes"]["currentModeId"] != "default" {
            return Err("Gemini 未采用逐次工具审批模式，未启动任务。".into());
        }
        Ok(info)
    }
    fn stop(&self) -> Result<(), String> {
        self.recording.store(false, Ordering::SeqCst);
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
fn set_status(
    store: &Store,
    conversation: &str,
    turn: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let mut connection = store.connect()?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let changed=tx.execute(&format!("UPDATE gemini_sessions SET status=?3,error=?4 WHERE conversation_id=?1 AND turn_id=?2 AND status IN {ACTIVE}"),params![conversation,turn,status,error]).map_err(|e|e.to_string())?;
    if changed > 0 && matches!(status, "completed" | "failed" | "interrupted" | "cancelled") {
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'task_end',?3,'gemini')",params![conversation,format!("gemini:end:{turn}"),status]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
pub fn recover(store: &Store) -> Result<(), String> {
    store.connect()?.execute(&format!("UPDATE gemini_sessions SET status='interrupted',error='应用已重启，上次 Gemini 执行结果未知；请核对后处理。' WHERE status IN {ACTIVE}"),[]).map_err(|e|e.to_string())?;
    Ok(())
}

impl Runtime {
    pub fn probe(&self, store: &Store) -> Reply {
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
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        if text.trim().is_empty() || text.len() > 100_000 || model.is_empty() {
            return Err("请选择 Gemini 模型，并填写有效消息。".into());
        }
        let busy: bool = store
            .connect()?
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE status IN {ACTIVE})"),
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if busy {
            return Err("Gemini 已有任务正在执行，请等待或取消。".into());
        }
        let root = PathBuf::from(store.conversation_root(conversation)?)
            .canonicalize()
            .map_err(|e| format!("Gemini 工作目录不可用：{e}"))?;
        let root = root.to_str().ok_or("Gemini 工作目录无效")?;
        let install = Installation::find()?;
        let previous = session(store, conversation)?;
        if !previous.is_null()
            && (previous["runtime_version"] != install.version
                || previous["workspace_path"] != root)
        {
            return Err("Gemini 运行版本或工作目录与历史不一致，未恢复错误会话。".into());
        }

        // Reuse the live ACP process when workspace and conversation match.
        let existing_client = self.client.lock().map_err(|e| e.to_string())?.clone();
        let reusable = existing_client.filter(|c| {
            c.healthy.load(Ordering::SeqCst)
                && c.root == root
                && c.conversation == conversation
                && role != "assignment"
                && !c.session_id.lock().map(|s| s.is_empty()).unwrap_or(true)
        });

        let (client, id) = if let Some(client) = reusable {
            let id = client.session_id.lock().map_err(|e| e.to_string())?.clone();
            if let Some(cached) = client.cached_models.lock().map_err(|e| e.to_string())?.as_ref() {
                if !cached.iter().any(|item| item["model"] == model) {
                    return Err("Gemini 实际目录中没有所选模型，未切换到其他模型。".into());
                }
            }
            let current = client.current_model.lock().map_err(|e| e.to_string())?.clone();
            if current != model {
                client.request(
                    "session/set_model",
                    json!({"sessionId":id,"modelId":model}),
                    Duration::from_secs(15),
                )?;
                *client.current_model.lock().map_err(|e| e.to_string())? = model.to_owned();
            }
            (client, id)
        } else {
            if let Some(old) = self.client.lock().map_err(|e| e.to_string())?.take() {
                let _ = old.stop();
            }
            let previous_id = if role == "assignment" {
                None
            } else {
                previous["thread_id"].as_str()
            };
            let client = Client::start(store, &install, root, conversation)?;
            let setup = (|| -> Reply {
                let info = client.metadata(root, previous_id)?;
                let available = models(&info)?;
                if !available.iter().any(|item| item["model"] == model) {
                    return Err("Gemini 实际目录中没有所选模型，未切换到其他模型。".into());
                }
                *client.cached_models.lock().map_err(|e| e.to_string())? = Some(available);
                let id = client.session_id.lock().map_err(|e| e.to_string())?.clone();
                let current_model = info["models"]["currentModelId"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                if current_model != model {
                    client.request(
                        "session/set_model",
                        json!({"sessionId":id,"modelId":model}),
                        Duration::from_secs(15),
                    )?;
                }
                *client.current_model.lock().map_err(|e| e.to_string())? = model.to_owned();
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
        let turn = uuid::Uuid::new_v4().to_string();
        *client.turn_id.lock().map_err(|e| e.to_string())? = turn.clone();
        let mut connection = store.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO gemini_sessions(conversation_id,thread_id,turn_id,model,runtime_version,workspace_path,status) VALUES(?1,?2,?3,?4,?5,?6,'starting') ON CONFLICT(conversation_id) DO UPDATE SET thread_id=excluded.thread_id,turn_id=excluded.turn_id,model=excluded.model,runtime_version=excluded.runtime_version,workspace_path=excluded.workspace_path,status='starting',error=NULL",params![conversation,id,turn,model,install.version,root]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'gemini')",params![conversation,format!("gemini:input:{turn}"),role,text]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'task_start','running','gemini')",params![conversation,format!("gemini:start:{turn}")]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        set_status(store, conversation, &turn, "running", None)?;
        client.recording.store(true, Ordering::SeqCst);
        // Save the prompt correlation before writing; completion comes only from its real RPC response.
        let request_id = client.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = mpsc::channel();
        client
            .pending
            .lock()
            .map_err(|e| e.to_string())?
            .insert(request_id, send);
        *self.client.lock().map_err(|e| e.to_string())? = Some(client.clone());
        if let Err(error)=client.write(json!({"jsonrpc":"2.0","id":request_id,"method":"session/prompt","params":{"sessionId":id,"prompt":[{"type":"text","text":text}]}})) {let _=client.stop();return Err(error);}
        std::thread::spawn(move || {
            let outcome = receive.recv_timeout(Duration::from_secs(24 * 60 * 60));
            let store = Store {
                path: client.database.clone(),
            };
            let (status, reason): (&str, Option<String>) = match outcome {
                Ok(Ok(result)) if result["stopReason"] == "end_turn" => ("completed", None),
                Ok(Ok(result)) if result["stopReason"] == "cancelled" => ("cancelled", None),
                Ok(Ok(result)) => (
                    "failed",
                    Some(format!(
                        "Gemini 本轮终态：{}；未标为正常完成。",
                        result["stopReason"]
                    )),
                ),
                Ok(Err(error)) => ("failed", Some(error)),
                _ => (
                    "interrupted",
                    Some("Gemini 执行未得到可确认的完成结果，请核对文件后处理。".into()),
                ),
            };
            let _ = set_status(
                &store,
                &client.conversation,
                &turn,
                status,
                reason.as_deref(),
            );
            client.recording.store(false, Ordering::SeqCst);
            client
                .approvals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            if status != "completed" {
                let _ = client.stop();
            }
        });
        Ok(json!({"thread_id":id}))
    }
    pub fn approvals(&self, conversation: &str) -> Result<Vec<Value>, String> {
        let slot = self.client.lock().map_err(|e| e.to_string())?;
        let Some(client) = slot.as_ref().filter(|c| c.conversation == conversation) else {
            return Ok(Vec::new());
        };
        let requests = client.approvals.lock().map_err(|e| e.to_string())?;
        Ok(requests.iter().map(|(id,request)|json!({"id":id,"method":"session/request_permission","command":request["params"]["toolCall"]["title"],"reason":serde_json::to_string(&request["params"]["toolCall"]).unwrap_or_default(),"cwd":null})).collect())
    }
    pub fn approve(&self, store: &Store, conversation: &str, id: &str, allow: bool) -> Reply {
        let slot = self.client.lock().map_err(|e| e.to_string())?;
        let client = slot
            .as_ref()
            .filter(|c| c.conversation == conversation && c.recording.load(Ordering::SeqCst))
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
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let slot = self.client.lock().map_err(|e| e.to_string())?;
        let client = slot
            .as_ref()
            .filter(|c| c.conversation == conversation)
            .cloned()
            .ok_or("Gemini 当前会话进程已断开；不能确认已取消。")?;
        drop(slot);
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
        if terminal == "cancelled" {
            Ok(json!({"cancelled":true}))
        } else if terminal == "completed" {
            Ok(json!({"cancelled":false,"completed":true}))
        } else {
            Err("Gemini 未确认取消终态，已清理应用管理的进程组；执行结果仍需核对。".into())
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut slot) = self.client.lock() {
            if let Some(client) = slot.take() {
                let _ = client.stop();
            }
        }
    }
}
