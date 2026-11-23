use crate::process::BoundedOutput;
use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

type Reply = Result<Value, String>;
const VERSION: &str = "0.2.1-rc.2";
pub const PERSONAL: &str = "deepseek-personal";
const PROFILE: &str = "ai-gent-sdk";
const RUNTIME_PATCH: &str = "- id: session-log-deepseek\n  disabled: true\n- id: sandbox-policy\n  config:\n    mode: workspace-write\n    workspaceRoot: !!js process.cwd()\n- id: approval\n  config:\n    policy: ask\n";

#[derive(Default)]
pub struct Runtime {
    client: Arc<Mutex<Option<Arc<Client>>>>,
    operation: Arc<Mutex<()>>,
}

// Serializes task admission across both providers. Running work is tracked in SQLite.
#[derive(Default)]
pub struct ExecutionGate(pub Mutex<()>);

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    #[cfg(unix)] {use std::os::unix::process::CommandExt;cmd.process_group(0);}
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    crate::process::apply_system_proxy(&mut cmd);
    cmd
}

struct Installation {
    executable: PathBuf,
    cli: PathBuf,
    resources: PathBuf,
    version: String,
}
impl Installation {
    fn find() -> Result<Self, String> {
        let mut candidates = Vec::new();
        if let Some(path) = std::env::var_os("AI_GENT_DSH_DESKTOP") {
            candidates.push(PathBuf::from(path));
        }
        #[cfg(windows)]
        {
            let output = command(Path::new("powershell.exe")).args(["-NoProfile", "-NonInteractive", "-Command", "Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/*','HKLM:/Software/Microsoft/Windows/CurrentVersion/Uninstall/*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'DeepSeek Harness*' } | ForEach-Object { Join-Path $_.InstallLocation 'DeepSeek Harness.exe' }"]).bounded_output().map_err(|_| "无法检查 DeepSeek Harness 安装位置")?;
            candidates.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|s| PathBuf::from(s.trim())),
            );
        }
        #[cfg(target_os="macos")]
        {
            let mut directories=vec![PathBuf::from("/Applications")];
            if let Some(home)=std::env::var_os("HOME") {directories.push(PathBuf::from(home).join("Applications"));}
            for directory in directories {
                if let Ok(entries)=std::fs::read_dir(directory) {
                    for entry in entries.flatten() {
                        let path=entry.path();
                        if path.extension().is_some_and(|s|s=="app") && path.file_name().is_some_and(|s|s.to_string_lossy().to_lowercase().contains("deepseek")) {
                            let output=command(Path::new("/usr/bin/plutil")).args(["-extract","CFBundleExecutable","raw","-o","-"]).arg(path.join("Contents/Info.plist")).bounded_output();
                            if let Ok(output)=output {if output.status.success() {let name=String::from_utf8_lossy(&output.stdout).trim().to_owned();if !name.contains('/') {candidates.push(path.join("Contents/MacOS").join(name));}}}
                        }
                    }
                }
            }
        }
        let executable = candidates.into_iter().find(|p| p.is_file()).ok_or("找不到 DeepSeek Harness 桌面运行组件。请安装 Harness；自定义位置可通过 AI_GENT_DSH_DESKTOP 指定运行文件。")?;
        let parent=executable.parent().ok_or("Harness 安装路径无效")?;
        let resources=if cfg!(target_os="macos") {parent.parent().ok_or("Harness 应用包无效")?.join("Resources")} else {parent.join("resources")};
        if !resources.join("app.asar").is_file() {return Err("Harness 安装布局不支持：缺少应用资源包。".into());}
        let cli=resources.join("app.asar/dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/cli.js");
        let mut installation = Self {
            executable,
            cli,
            resources,
            version: String::new(),
        };
        let output = installation
            .launch()
            .arg("--version")
            .bounded_output()
            .map_err(|_| "无法读取 Harness 版本")?;
        installation.version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || installation.version != VERSION {
            return Err(format!(
                "当前 Harness 版本为 {}，此适配器已验证版本为 {VERSION}。请先验证版本兼容性。",
                installation.version
            ));
        }
        Ok(installation)
    }
    fn launch(&self) -> Command {
        let mut cmd = command(&self.executable);
        cmd.env("ELECTRON_RUN_AS_NODE", "1")
            .arg("--expose-internals")
            .arg(&self.cli);
        cmd
    }
    fn models(&self, store: &Store) -> Reply {
        let script = runtime_dir(store)?.join("catalog.cjs");
        std::fs::write(&script, include_str!("deepseek_catalog.cjs")).map_err(|e| e.to_string())?;
        let output = command(&self.executable)
            .env("ELECTRON_RUN_AS_NODE", "1")
            .arg("--expose-internals")
            .arg(script)
            .arg(&self.resources)
            .bounded_output()
            .map_err(|_| "读取 Harness 模型目录失败")?;
        if !output.status.success() {
            return Err("读取已安装 Harness 模型目录失败。".into());
        }
        let models: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Harness 模型目录格式不兼容")?;
        if models.as_array().is_none_or(|m| m.is_empty()) {
            return Err("Harness 模型目录为空。".into());
        }
        Ok(models)
    }
}

fn runtime_dir(store: &Store) -> Result<PathBuf, String> {
    let dir = store
        .path
        .parent()
        .ok_or("数据目录无效")?
        .join("runtimes/deepseek");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

struct Client {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    next: AtomicU64,
    healthy: AtomicBool,
    database: PathBuf,
    thread: String,
    conversation: String,
    #[cfg(any(windows, unix))]
    job: crate::codex::ProcessJob,
}
impl Client {
    fn start(
        store: &Store,
        install: &Installation,
        workspace: &str,
        thread: &str,
        conversation: &str,
        model: &str,
        effort: Option<&str>,
    ) -> Result<Arc<Self>, String> {
        let catalog = thread.starts_with("ai-gent-skill-catalog-");
        let patch = runtime_dir(store)?.join(if catalog { "skills.patch.yml" } else { "private.patch.yml" });
        // Keep this application's logs local. Required approvals fail closed in this SDK version.
        let runtime_patch = if cfg!(windows) {
            let dir = runtime_dir(store)?;
            let plugin = dir.join("console-plugin.cjs");
            std::fs::write(dir.join("console.cjs"), include_str!("deepseek_console.cjs")).map_err(|e| e.to_string())?;
            std::fs::write(&plugin, include_str!("deepseek_console_plugin.cjs")).map_err(|e| e.to_string())?;
            format!("{RUNTIME_PATCH}- insert:\n    - id: ai-gent-windows-console\n      name: {}\n",serde_json::to_string(&plugin.to_string_lossy()).map_err(|e|e.to_string())?)
        } else { RUNTIME_PATCH.to_owned() };
        let runtime_patch = if catalog {
            let skills_plugin = runtime_dir(store)?.join("skills-plugin.cjs");
            std::fs::write(&skills_plugin, include_str!("deepseek_skills_plugin.cjs")).map_err(|e| e.to_string())?;
            format!("{runtime_patch}- insert:\n    - id: ai-gent-skills\n      name: {}\n", serde_json::to_string(&skills_plugin.to_string_lossy()).map_err(|e| e.to_string())?)
        } else { runtime_patch };
        std::fs::write(&patch, runtime_patch).map_err(|e| e.to_string())?;
        let home = std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(|p| PathBuf::from(p).join(".dsh")))
            .ok_or("找不到 Harness 配置目录")?;
        let mut cmd = install.launch();
        cmd.args(["--profile", PROFILE]);
        if !home
            .join("profiles")
            .join(PROFILE)
            .join("package.json")
            .is_file()
        {
            cmd.args(["--from-default-profile", "sdk"]);
        }
        let mut child = cmd
            .arg("--patch")
            .arg(patch)
            .env("DSH_PERMISSION_MODE", "workspace-write")
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "无法启动 DeepSeek Harness SDK")?;
        #[cfg(any(windows, unix))]
        let job = match crate::codex::ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("无法绑定 DeepSeek 后台进程组。".into());
            }
        };
        let input = child.stdin.take().ok_or("Harness stdin 不可用")?;
        let output = child.stdout.take().ok_or("Harness stdout 不可用")?;
        let stderr = child.stderr.take().ok_or("Harness stderr 不可用")?;
        let client = Arc::new(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            database: store.path.clone(),
            thread: thread.into(),
            conversation: conversation.into(),
            #[cfg(any(windows, unix))]
            job,
        });
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
        });
        let weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Some(client) = weak.upgrade() else { break };
                let result = line
                    .map_err(|_| "Harness 协议读取失败".to_owned())
                    .and_then(|s| {
                        serde_json::from_str::<Value>(&s)
                            .map_err(|_| "Harness 协议格式不兼容".into())
                    })
                    .and_then(|v| client.receive(v));
                if let Err(error) = result {
                    client.disconnect(&error);
                    break;
                }
            }
            if let Some(client) = weak.upgrade() {
                client.disconnect("DeepSeek 连接已断开，未完成任务的结果未知。");
            }
        });
        let mut params = json!({"cwd":workspace,"provider":"deepseek-official","model":model});
        if let Some(effort) = effort {
            params["reasoningEffort"] = json!(effort);
        }
        let init = client.request("initialize", params, Duration::from_secs(45));
        match init {
            Ok(v) if v["serverInfo"]["name"] == "deepseek-harness-sdk-runtime" => Ok(client),
            _ => {
                let _ = client.stop();
                Err("DeepSeek SDK 初始化失败。请检查 Harness 的 API 凭证、模型服务及版本；原始认证诊断不写入聊天记录。".into())
            }
        }
    }
    fn write(&self, value: Value) -> Result<(), String> {
        let mut input = self.input.lock().map_err(|e| e.to_string())?;
        writeln!(input, "{value}")
            .and_then(|_| input.flush())
            .map_err(|_| "DeepSeek 协议发送失败".into())
    }
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        if !self.healthy.load(Ordering::SeqCst) {
            return Err("DeepSeek 后台已断开。".into());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let result = self
            .write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .and_then(|_| {
                rx.recv_timeout(timeout)
                    .map_err(|_| format!("DeepSeek {method} 未确认，执行结果未知。"))?
            });
        self.pending.lock().unwrap().remove(&id);
        result
    }
    fn receive(&self, value: Value) -> Result<(), String> {
        if !self.healthy.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(method) = value["method"].as_str() {
            if value.get("id").is_some() {
                return self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32601,"message":"Unsupported host request; approval not granted"}}));
            }
            let p = &value["params"];
            if p["sessionId"] != self.thread || method != "session.event" {
                return Ok(());
            }
            let event = &p["event"];
            let data = &event["data"];
            let store = Store {
                path: self.database.clone(),
            };
            let connection = store.connect()?;
            // Resume replays old events. Only the current running turn may append UI records.
            let active: bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM deepseek_sessions WHERE thread_id=?1 AND status='running')",[&self.thread],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !active {
                return Ok(());
            }
            match event["type"].as_str().unwrap_or("") {
                "assistant/message" | "tool/result" => {
                    let message = &data["message"];
                    let role = if event["type"] == "tool/result" {
                        "tool"
                    } else {
                        "assistant"
                    };
                    let text = message_text(message);
                    if !text.is_empty() {
                        let id = message["id"].as_str().ok_or("DeepSeek 消息缺少标识")?;
                        append_message(&connection, &self.conversation, id, role, &text)?;
                    }
                    if let Some(blocks) = message["content"].as_array() {
                        for block in blocks.iter().filter(|b| b["type"] == "tool-call") {
                            let id = block["id"].as_str().ok_or("DeepSeek 工具缺少标识")?;
                            append_message(
                                &connection,
                                &self.conversation,
                                &format!("call:{id}"),
                                "tool",
                                &format!(
                                    "工具开始：{}\n{}",
                                    block["name"].as_str().unwrap_or("未知工具"),
                                    block["arguments"].as_str().unwrap_or("")
                                ),
                            )?;
                        }
                    }
                }
                "turn/end" => {
                    let reason = data["reason"]["kind"].as_str().unwrap_or("unknown");
                    if reason != "completed" {
                        set_status(
                            &store,
                            &self.thread,
                            "failed",
                            Some(&format!("DeepSeek 执行结束：{reason}。请核对执行记录。")),
                        )?;
                    }
                }
                _ => {}
            }
        } else if let Some(id) = value["id"].as_u64() {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                // Provider errors can contain request details. Keep raw diagnostics out of SQLite/UI.
                let result = if value.get("error").is_some() {
                    Err("DeepSeek 请求失败。请检查 Harness 模型服务、额度和工具权限。".into())
                } else {
                    value
                        .get("result")
                        .cloned()
                        .ok_or("DeepSeek 响应缺少结果".into())
                };
                let _ = tx.send(result);
            }
        } else {
            return Err("DeepSeek 协议响应格式不兼容".into());
        }
        Ok(())
    }
    fn disconnect(&self, error: &str) {
        if self.healthy.swap(false, Ordering::SeqCst) {
            let _ = self.stop();
            let _ = set_status(
                &Store {
                    path: self.database.clone(),
                },
                &self.thread,
                "interrupted",
                Some(error),
            );
        }
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(error.into()));
        }
    }
    fn stop(&self) -> Result<(), String> {
        self.healthy.store(false, Ordering::SeqCst);
        #[cfg(any(windows, unix))]
        self.job
            .terminate()
            .map_err(|_| "停止 DeepSeek 进程组失败")?;
        let mut child = self.child.lock().unwrap();
        #[cfg(not(any(windows, unix)))]
        child.kill().map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn append_message(
    connection: &rusqlite::Connection,
    conversation: &str,
    id: &str,
    role: &str,
    text: &str,
) -> Result<(), String> {
    if conversation == PERSONAL {
        connection.execute("INSERT INTO deepseek_messages(item_id,role,text) VALUES(?1,?2,?3) ON CONFLICT(item_id) DO NOTHING", params![id,role,text]).map_err(|e|e.to_string())?;
    } else {
        connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,?3,?4,'deepseek') ON CONFLICT(conversation_id,item_id) DO NOTHING", params![conversation,format!("deepseek:{id}"),role,text]).map_err(|e|e.to_string())?;
    }
    Ok(())
}

fn message_text(message: &Value) -> String {
    message["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
pub fn session(store: &Store, conversation: &str) -> Reply {
    store.connect()?.query_row("SELECT thread_id,model,runtime_version,workspace_path,status,error FROM deepseek_sessions WHERE conversation_id=?1",[conversation],|r|Ok(json!({"thread_id":r.get::<_,String>(0)?,"model":r.get::<_,String>(1)?,"runtime_version":r.get::<_,String>(2)?,"workspace_path":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"error":r.get::<_,Option<String>>(5)?}))).optional().map(|s|s.unwrap_or(Value::Null)).map_err(|e|e.to_string())
}
fn set_status(
    store: &Store,
    thread: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let mut connection = store.connect()?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    let conversation: Option<String> = tx.query_row("SELECT conversation_id FROM deepseek_sessions WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') AND (?2!='completed' OR status='running')", params![thread, status], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    if let Some(conversation) = conversation.filter(|_| matches!(status, "completed" | "cancelled" | "failed" | "interrupted")) {
        append_message(&tx, &conversation, &uuid::Uuid::new_v4().to_string(), "task_end", status)?;
    }
    tx.execute("UPDATE deepseek_sessions SET status=?2,error=?3 WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') AND (?2!='completed' OR status='running')",params![thread,status,error]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn ensure_idle(store: &Store) -> Result<(), String> {
    let busy:bool=store.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM gemini_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if busy {
        Err("已有任务正在执行，请先完成或取消。".into())
    } else {
        Ok(())
    }
}
pub fn recover(store: &Store) -> Result<(), String> {
    store.connect()?.execute("UPDATE deepseek_sessions SET status='interrupted',error='程序已重启，上次执行结果未知；请核对文件后继续。' WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')",[]).map_err(|e|e.to_string())?;
    Ok(())
}

impl Runtime {
    pub(crate) fn skills(&self, store: &Store, workspace: &Path, names: &[String]) -> Reply {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let install = Installation::find()?;
        let models = install.models(store)?;
        let model = models.as_array().and_then(|items| items.first()).and_then(|item| item["model"].as_str()).ok_or("Harness 模型目录为空")?;
        std::fs::create_dir_all(workspace).map_err(|e| e.to_string())?;
        let root = workspace.canonicalize().map_err(|e| e.to_string())?;
        let root = root.to_str().ok_or("技能工作目录路径无效")?;
        let root = if let Some(unc) = root.strip_prefix(r"\\?\UNC\") { format!(r"\\{unc}") } else { root.strip_prefix(r"\\?\").unwrap_or(root).to_owned() };
        let thread = format!("ai-gent-skill-catalog-{}", uuid::Uuid::new_v4());
        // No prompt or Agent is created: the catalog only resolves the default preset scope.
        let client = Client::start(store, &install, &root, &thread, &thread, model, None)?;
        let result = client.request("ai-gent/skills", json!({"sessionId":thread,"names":names}), Duration::from_secs(45));
        let _ = client.request("shutdown", json!({}), Duration::from_secs(5));
        let stopped = client.stop();
        let result = result?;
        stopped?;
        Ok(result)
    }

    pub fn probe(&self, store: &Store) -> Reply {
        let _op = self.operation.lock().map_err(|e| e.to_string())?;
        let install = Installation::find()?;
        let models = install.models(store)?;
        Ok(
            json!({"version":install.version,"executable":install.executable,"auth":"Harness 配置（发送时验证认证）","requires_auth":true,"models":models,"sidebar_warning":null,"catalog_source":"已安装 Harness 内置目录","capabilities":{"streaming":"message-events","resume":true,"cancel":"process-group","approval":false}}),
        )
    }
    pub fn snapshot(&self, store: &Store) -> Reply {
        let connection = store.connect()?;
        let mut stmt = connection
            .prepare("SELECT item_id,role,text,created_at FROM deepseek_messages ORDER BY sequence")
            .map_err(|e| e.to_string())?;
        let messages=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?,"text":r.get::<_,String>(2)?,"created_at":r.get::<_,String>(3)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        Ok(json!({"session":session(store, PERSONAL)?,"messages":messages,"approvals":[]}))
    }
    pub fn send(&self, store: &Store, model: &str, text: &str) -> Reply {
        self.send_to(store, PERSONAL, model, text)
    }
    pub fn send_to(&self, store: &Store, conversation: &str, model: &str, text: &str) -> Reply {
        self.send_recorded(store, conversation, model, text, "user")
    }
    pub(crate) fn send_recorded(
        &self,
        store: &Store,
        conversation: &str,
        model: &str,
        text: &str,
        role: &str,
    ) -> Reply {
        let _op = self.operation.lock().map_err(|e| e.to_string())?;
        if text.trim().is_empty() || text.len() > 100000 {
            return Err("消息为空或过长。".into());
        }
        // Group assignments have one slot per provider; private sends retain global admission.
        if role == "assignment" {
            let busy: bool = store.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM deepseek_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping'))", [], |r| r.get(0)).map_err(|e| e.to_string())?;
            if busy {
                return Err("DeepSeek 已有任务正在执行。".into());
            }
        } else {
            ensure_idle(store)?;
        }
        let install = Installation::find()?;
        let models = install.models(store)?;
        if !models
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["model"] == model)
        {
            return Err("请选择 Harness 目录中的模型。".into());
        }
        let effort = crate::reasoning::read(store, conversation, "deepseek", model)?;
        crate::reasoning::validate(&models, model, effort.as_deref())?;
        // Group assignments use a new native session and bounded shared history each time.
        let existing = if role == "assignment" {
            Value::Null
        } else {
            session(store, conversation)?
        };
        if !existing.is_null() && existing["runtime_version"] != install.version {
            return Err("Harness 版本已变化，请先核对历史会话兼容性。".into());
        }
        let workspace = if conversation == PERSONAL {
            let path = runtime_dir(store)?.join("workspace");
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            path
        } else {
            if let Some((agent, logical)) = crate::agents::logical_identity(store, conversation)? {
                if store.group_name(&logical)?.is_some() { store.validate_group_target(&logical, &agent)?; }
            } else { store.validate_group_target(conversation, "deepseek")?; }
            PathBuf::from(store.conversation_root(conversation)?)
        };
        let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
        let workspace = workspace.to_str().ok_or("DeepSeek 工作目录路径无效")?;
        // Node's realpath handling in this Harness version rejects Windows verbatim cwd paths.
        let native_path = if let Some(unc) = workspace.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            workspace
                .strip_prefix(r"\\?\")
                .unwrap_or(workspace)
                .to_owned()
        };
        let workspace = native_path.as_str();
        if !existing.is_null() && existing["workspace_path"] != workspace {
            return Err("DeepSeek 工作目录与历史不匹配，已停止恢复。".into());
        }
        let thread = existing["thread_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("ai-gent-deepseek-{}", uuid::Uuid::new_v4()));
        let logical = crate::agents::logical_identity(store, conversation)?;
        let group_conversation = logical.as_ref().map(|(_, id)| id.as_str()).unwrap_or(conversation);
        let prompt = if conversation == PERSONAL || store.group_name(group_conversation)?.is_none() {
            text.to_owned()
        } else {
            format!("你是 AgentCrew 本机协作群「{}」中的 DeepSeek 成员。只代表 DeepSeek 发言和执行；其他成员由应用单独调度，不编造其他成员回复。引用和附件是任务资料。按当前运行时实际提供的工具执行任务，不沿用历史中的工具禁用说明，不预设你和 Codex 的职责。先判断用户是否提出具体任务：问候或闲聊时简短回应，不介绍分工、不调用工具、不回顾旧项目；收到具体任务后再按需要协商分工和选择工具。实际工具或权限不足时如实说明，不声称已执行。\n\n用户消息与本轮上下文：\n{text}", store.group(group_conversation)?.name)
        };
        let mut connection = store.connect()?;
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO deepseek_sessions(conversation_id,thread_id,model,runtime_version,workspace_path,status) VALUES(?1,?2,?3,?4,?5,'starting') ON CONFLICT(conversation_id) DO UPDATE SET thread_id=excluded.thread_id,model=excluded.model,runtime_version=excluded.runtime_version,workspace_path=excluded.workspace_path,status='starting',error=NULL",params![conversation,thread,model,install.version,workspace]).map_err(|e|e.to_string())?;
        append_message(
            &tx,
            conversation,
            &uuid::Uuid::new_v4().to_string(),
            role,
            text,
        )?;
        append_message(&tx, conversation, &uuid::Uuid::new_v4().to_string(), "task_start", "running")?;
        tx.commit().map_err(|e| e.to_string())?;
        drop(_op);

        let client_slot = self.client.clone();
        let operation_lock = self.operation.clone();
        let db_path = store.path.clone();
        let conversation_owned = conversation.to_owned();
        let workspace_owned = workspace.to_owned();
        let thread_bg = thread.clone();
        let model_owned = model.to_owned();
        let is_resume = !existing.is_null();
        std::thread::spawn(move || {
            let store = Store { path: db_path };
            let Ok(_op) = operation_lock.lock() else {
                let _ = set_status(&store, &thread_bg, "interrupted", Some("DeepSeek 运行锁不可用。"));
                return;
            };
            let still_starting = session(&store, &conversation_owned)
                .ok()
                .is_some_and(|s| s["thread_id"].as_str() == Some(thread_bg.as_str()) && s["status"] == "starting");
            if !still_starting {
                return;
            }
            let launch = (|| -> Result<Arc<Client>, String> {
                if let Some(old) = client_slot.lock().unwrap().take() {
                    let _ = old.request("shutdown", json!({}), Duration::from_secs(5));
                    old.stop()?;
                }
                let client = Client::start(
                    &store,
                    &install,
                    &workspace_owned,
                    &thread_bg,
                    &conversation_owned,
                    &model_owned,
                    effort.as_deref(),
                )?;
                if is_resume {
                    if let Err(error) = client.request(
                        "session/resume",
                        json!({"sessionId":thread_bg}),
                        Duration::from_secs(30),
                    ) {
                        let _ = client.stop();
                        return Err(error);
                    }
                }
                let root = client.request(
                    "session/working-directory/get",
                    json!({"sessionId":thread_bg}),
                    Duration::from_secs(30),
                );
                if !root.as_ref().is_ok_and(|v| {
                    v["cwd"].as_str().is_some_and(|s| {
                        Path::new(s).canonicalize().is_ok_and(|actual| {
                            Path::new(&workspace_owned)
                                .canonicalize()
                                .is_ok_and(|expected| actual == expected)
                        })
                    })
                }) {
                    let _ = client.stop();
                    return Err("Harness 返回的工作目录不匹配，执行已停止。".into());
                }
                store
                    .connect()?
                    .execute(
                        "UPDATE deepseek_sessions SET status='running' WHERE thread_id=?1 AND status='starting'",
                        params![thread_bg],
                    )
                    .map_err(|e| e.to_string())?;
                *client_slot.lock().unwrap() = Some(client.clone());
                if let Err(error) = client.request(
                    "session/prompt",
                    json!({"sessionId":thread_bg,"contentBlocks":[{"type":"text","text":prompt}]}),
                    Duration::from_secs(30),
                ) {
                    let _ = client.stop();
                    return Err(error);
                }
                Ok(client)
            })();
            let client = match launch {
                Ok(client) => client,
                Err(error) => {
                    let _ = set_status(&store, &thread_bg, "interrupted", Some(&error));
                    return;
                }
            };
            drop(_op);

            match client.request(
                "session/wait",
                json!({"sessionId":client.thread}),
                Duration::from_secs(3600),
            ) {
                Ok(_) if client.healthy.load(Ordering::SeqCst) => {
                    let _ = set_status(&store, &client.thread, "completed", None);
                }
                Ok(_) => {}
                Err(error) => {
                    if client.healthy.load(Ordering::SeqCst) {
                        let _ = client.stop();
                        let _ = set_status(&store, &client.thread, "interrupted", Some(&error));
                    }
                }
            }
        });
        Ok(json!({"thread_id":thread, "status":"starting"}))
    }
    pub fn cancel(&self, store: &Store) -> Reply {
        self.cancel_in(store, PERSONAL)
    }
    pub fn cancel_in(&self, store: &Store, conversation: &str) -> Reply {
        let _op = self.operation.lock().map_err(|e| e.to_string())?;
        let current = session(store, conversation)?;
        if !matches!(
            current["status"].as_str(),
            Some("running" | "starting" | "waiting_approval")
        ) {
            return Err("当前没有正在执行的 DeepSeek 任务。".into());
        }
        let mut slot = self.client.lock().map_err(|e| e.to_string())?;
        if !slot.as_ref().is_some_and(|client| {
            client.conversation == conversation && current["thread_id"] == client.thread
        }) {
            return Err("当前会话的 DeepSeek 进程已断开，请核对执行记录。".into());
        }
        let client = slot.take().ok_or("DeepSeek 进程已断开")?;
        drop(slot);
        set_status(store, &client.thread, "stopping", None)?;
        // No interrupt RPC in the pinned SDK. Stop our complete process group and confirm exit.
        match client.stop() {
            Ok(()) => {
                set_status(store, &client.thread, "cancelled", None)?;
                Ok(json!({"cancelled":true}))
            }
            Err(error) => {
                set_status(store, &client.thread, "interrupted", Some(&error))?;
                Err(error)
            }
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut slot) = self.client.lock() {
            if let Some(client) = slot.take() {
                let _ = client.request("shutdown", json!({}), Duration::from_secs(2));
                let _ = client.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn confined_shell(temp: &Path, script: &str) -> (Command, PathBuf) {
        let install = Installation::find().unwrap();
        let workspace = temp.join("中文 工作区");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = temp.join("console.cjs");
        std::fs::write(&bootstrap, include_str!("deepseek_console.cjs")).unwrap();
        let runner = install.executable.parent().unwrap().join("resources/app.asar/dsh/node_modules/@deepseek-ai/dsh-sandbox-windows-acl/lib/runner.js");
        let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut cmd = command(&install.executable);
        cmd.env("ELECTRON_RUN_AS_NODE","1")
            .args(["--expose-internals","--require"]).arg(bootstrap).arg(runner)
            .arg("--workspace").arg(&workspace).arg("--temp").arg(temp)
            .args(["--mode","workspace-write","--"]).arg(shell)
            .args(["-NoLogo","-NoProfile","-NonInteractive","-Command",script])
            .current_dir(&workspace).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        (cmd,workspace)
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Runs the installed Windows sandbox without a model; verifies workspace writes and outside-write denial"]
    fn installed_deepseek_terminal_sandbox_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside-denied.txt");
        let escaped = outside.to_string_lossy().replace('\'',"''");
        let script = format!("$ErrorActionPreference='Stop'; Set-Content -LiteralPath 'inside.txt' -Value '中文写入成功' -Encoding UTF8; Write-Output 'INSIDE_OK'; try {{ Set-Content -LiteralPath '{escaped}' -Value 'denied' -ErrorAction Stop; Write-Output 'OUTSIDE_ALLOWED' }} catch [System.UnauthorizedAccessException] {{ Write-Output 'OUTSIDE_DENIED' }}");
        let (mut cmd,workspace) = confined_shell(temp.path(),&script);
        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(),"{} / {}",stdout,String::from_utf8_lossy(&output.stderr));
        assert!(stdout.contains("INSIDE_OK") && stdout.contains("OUTSIDE_DENIED"),"{stdout}");
        assert!(!outside.exists());
        let content = std::fs::read_to_string(workspace.join("inside.txt")).unwrap();
        assert_eq!(content.trim_start_matches('\u{feff}').trim(),"中文写入成功");
        println!("Confined shell: Chinese workspace write succeeded; outside write was denied.");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Runs and cancels a confined PowerShell process tree in a temporary workspace; no model calls"]
    fn installed_deepseek_terminal_process_group_cancel() {
        use windows_sys::Win32::{Foundation::CloseHandle,System::Threading::{OpenProcess,WaitForSingleObject}};
        let temp = tempfile::tempdir().unwrap();
        let (mut cmd,workspace) = confined_shell(temp.path(),"Set-Content -LiteralPath 'shell.pid' -Value $PID; Start-Sleep -Seconds 30");
        let mut child = cmd.spawn().unwrap();
        let job = crate::codex::ProcessJob::attach(&child).unwrap();
        let deadline = std::time::Instant::now()+Duration::from_secs(10);
        let pid_path = workspace.join("shell.pid");
        while !pid_path.exists() {
            assert!(child.try_wait().unwrap().is_none(),"shell stopped before starting");
            assert!(std::time::Instant::now()<deadline,"shell did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        let pid: u32 = std::fs::read_to_string(pid_path).unwrap().trim().parse().unwrap();
        // SYNCHRONIZE lets the test wait for this exact process, even after its PID is reused.
        let handle = unsafe {OpenProcess(0x00100000,0,pid)};
        assert!(!handle.is_null());
        job.terminate().unwrap();
        let exited = unsafe {WaitForSingleObject(handle,3000)};
        unsafe {CloseHandle(handle)};
        assert_eq!(exited,0,"PowerShell survived cancellation");
        assert!(!child.wait().unwrap().success());
        println!("Cancelling the owned process group terminated the confined PowerShell child.");
    }

    #[test]
    fn runtime_patch_keeps_default_tools_and_preserves_workspace_permissions() {
        assert!(!RUNTIME_PATCH.contains("tool-pwsh"));
        assert!(!RUNTIME_PATCH.contains("tool-plugin-manager"));
        assert!(RUNTIME_PATCH.contains("mode: workspace-write"));
        assert!(RUNTIME_PATCH.contains("policy: ask"));
    }

    #[test]
    #[ignore = "Uses one real Harness turn to verify an ordinary group greeting does not invoke tools or assign roles"]
    fn installed_deepseek_group_greeting() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let group = store.create_group_in("greeting",vec!["codex".into(),"deepseek".into()],None,&temp.path().join("groups")).unwrap();
        let runtime = Runtime::default();
        let result = (|| -> Result<(),String> {
            let probe = runtime.probe(&store)?;
            let model = probe["models"][0]["model"].as_str().ok_or("model missing")?;
            runtime.send_to(&store,&group.id,model,"你们好")?;
            let deadline = std::time::Instant::now()+Duration::from_secs(90);
            while session(&store,&group.id)?["status"].as_str().is_some_and(crate::codex::active) {
                if std::time::Instant::now()>deadline {return Err("greeting timed out".into());}
                std::thread::sleep(Duration::from_millis(100));
            }
            let connection = store.connect()?;
            let tools: u32 = connection.query_row("SELECT COUNT(*) FROM codex_messages WHERE conversation_id=?1 AND role='tool'",[&group.id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if tools != 0 {return Err("greeting unexpectedly invoked tools".into());}
            let reply: String = connection.query_row("SELECT text FROM codex_messages WHERE conversation_id=?1 AND role='assistant' ORDER BY sequence DESC LIMIT 1",[&group.id],|r|r.get(0)).map_err(|e|e.to_string())?;
            println!("DeepSeek greeting: {reply}");
            if reply.chars().count()>160 || ["分工","不能运行终端","仅支持文件","当前工作区"].iter().any(|word|reply.contains(word)) {return Err("greeting unnecessarily explained roles or workspace".into());}
            Ok(())
        })();
        runtime.shutdown();
        result.unwrap();
    }

    #[test]
    #[ignore = "Uses a real Harness model turn to check PowerShell tool availability and execution"]
    fn installed_deepseek_enabled_powershell() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let runtime = Runtime::default();
        let result = (|| -> Result<(),String> {
            let probe = runtime.probe(&store)?;
            let model = probe["models"][0]["model"].as_str().ok_or("model missing")?;
            runtime.send(&store,model,"请使用当前已提供的 pwsh 工具执行且只执行 Write-Output AI_GENT_PWSH_OK。不要创建文件、不要安装插件、不要调用其他工具。请如实报告工具是否执行成功；失败则报告错误，不要换工具或请求提权。")?;
            let data = wait(&runtime,&store);
            let messages = data["messages"].as_array().ok_or("messages missing")?;
            let calls = messages.iter().filter(|m|m["role"] == "tool" && m["text"].as_str().is_some_and(|t|t.starts_with("工具开始：pwsh"))).count();
            if calls == 0 { return Err("PowerShell tool was not called".into()); }
            let results: Vec<_> = messages.iter().filter(|m|m["role"] == "tool" && !m["id"].as_str().unwrap_or("").starts_with("call:")).collect();
            println!("PowerShell tool called; result: {}",serde_json::to_string(&results).unwrap());
            if !results.iter().any(|m|m["text"].as_str().is_some_and(|t|t.contains("AI_GENT_PWSH_OK") && !t.contains("3221225794") && !t.contains("0xC0000142"))) {
                return Err("PowerShell was enabled but command execution did not return the marker".into());
            }
            Ok(())
        })();
        runtime.shutdown();
        result.unwrap();
    }
    fn wait(runtime: &Runtime, store: &Store) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(150);
        loop {
            let value = runtime.snapshot(store).unwrap();
            if !matches!(
                value["session"]["status"].as_str(),
                Some("starting" | "running" | "stopping")
            ) {
                assert_eq!(value["session"]["status"], "completed", "{value}");
                return value;
            }
            assert!(std::time::Instant::now() < deadline, "DeepSeek timed out");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[test]
    #[ignore = "Uses two short Harness model turns to verify reasoning off/max and resume"]
    fn installed_deepseek_reasoning_selection() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let model = probe["models"][0]["model"].as_str().unwrap();
        let mut thread = Value::Null;
        for effort in ["off", "max"] {
            crate::reasoning::save(&store, PERSONAL, "deepseek", model, Some(effort)).unwrap();
            runtime
                .send(&store, model, "不要用工具或子代理。只回复 REASONING_OK。")
                .unwrap();
            let result = wait(&runtime, &store);
            if !thread.is_null() {
                assert_eq!(result["session"]["thread_id"], thread);
            }
            thread = result["session"]["thread_id"].clone();
            assert!(result["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|m| m["role"] == "assistant")
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .contains("REASONING_OK"));
            println!("DeepSeek explicit effort {effort} completed");
        }
        runtime.shutdown();
    }

    #[test]
    #[ignore = "Uses real Harness API calls in a temporary private workspace"]
    fn installed_deepseek_private_resume_and_cancel() {
        let temp = tempfile::Builder::new()
            .prefix("AgentCrew 中文 private ")
            .tempdir()
            .unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let model = probe["models"][0]["model"].as_str().unwrap();
        let marker = uuid::Uuid::new_v4().to_string();
        runtime.send(&store,model,&format!("这是隔离的接入测试。请记住暗号 {marker}，使用 write 工具在当前目录创建 marker.txt，内容仅为该暗号。不要创建目标或子代理。最后只回复该暗号。")).unwrap();
        let value = wait(&runtime, &store);
        let thread = value["session"]["thread_id"].clone();
        let workspace = PathBuf::from(value["session"]["workspace_path"].as_str().unwrap());
        assert_eq!(
            std::fs::read_to_string(workspace.join("marker.txt"))
                .unwrap()
                .trim(),
            marker
        );
        assert!(value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "assistant" && m["text"].as_str().unwrap().contains(&marker)));
        runtime.shutdown();
        drop(runtime);
        let runtime = Runtime::default();
        runtime
            .send(
                &store,
                model,
                "不要用工具，不要读取文件。只回复我上一轮给你的暗号。",
            )
            .unwrap();
        let value = wait(&runtime, &store);
        assert_eq!(value["session"]["thread_id"], thread);
        assert!(value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&marker));
        assert!(store.list_projects().unwrap().is_empty());
        // Use the other installed catalog entry to verify per-turn model selection on resume.
        let next_model = probe["models"][1]["model"].as_str().unwrap();
        runtime
            .send(&store, next_model, "不要使用工具。只回复 MODEL_SWITCH_OK。")
            .unwrap();
        let value = wait(&runtime, &store);
        assert_eq!(value["session"]["model"], next_model);
        assert!(value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("MODEL_SWITCH_OK"));
        runtime.send(&store,model,"这是取消生成测试。不要使用工具、子代理或目标。请从1写到10000，每行一个数字，不要省略。").unwrap();
        let pid = runtime
            .client
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .child
            .lock()
            .unwrap()
            .id();
        std::thread::sleep(Duration::from_millis(200));
        runtime.cancel(&store).unwrap();
        assert_eq!(
            runtime.snapshot(&store).unwrap()["session"]["status"],
            "cancelled"
        );
        #[cfg(windows)]
        {
            let output = command(Path::new("tasklist.exe"))
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()
                .unwrap();
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")),
                "Harness process survived cancellation"
            );
        }
        runtime
            .send(&store, model, "不要用工具。只回复最开始的暗号。")
            .unwrap();
        let value = wait(&runtime, &store);
        assert!(value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&marker));
        runtime.shutdown();
        println!("DeepSeek private write, restart resume, model switch, generation cancel, process exit and post-cancel resume passed");
    }
    #[test]
    fn terminal_status_persists_one_end_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        store.connect().unwrap().execute("INSERT INTO deepseek_sessions(conversation_id,thread_id,model,runtime_version,workspace_path,status) VALUES('deepseek-personal','deepseek','model','test','workspace','running')",[]).unwrap();
        set_status(&store, "deepseek", "completed", None).unwrap();
        set_status(&store, "deepseek", "completed", None).unwrap();
        let result = Runtime::default().snapshot(&store).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "task_end");
        assert_eq!(messages[0]["text"], "completed");
        assert!(!messages[0]["created_at"].as_str().unwrap().is_empty());
    }

    #[test]
    fn deepseek_records_are_separate_and_restart_is_conservative() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        store.connect().unwrap().execute("INSERT INTO deepseek_sessions(conversation_id,thread_id,model,runtime_version,workspace_path,status) VALUES('deepseek-personal','deepseek','model','test','workspace','running')",[]).unwrap();
        assert!(ensure_idle(&store).is_err());
        recover(&store).unwrap();
        assert_eq!(session(&store, PERSONAL).unwrap()["status"], "interrupted");
        assert!(ensure_idle(&store).is_ok());
        let client_text = message_text(
            &json!({"content":[{"type":"reasoning","text":"hidden"},{"type":"text","text":"answer"},{"type":"tool-call","name":"write"}]}),
        );
        assert_eq!(client_text, "answer");
        assert!(Runtime::default().snapshot(&store).unwrap()["messages"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
