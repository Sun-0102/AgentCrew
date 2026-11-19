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

#[derive(Default)]
pub struct Runtime {
    client: Mutex<Option<Arc<Client>>>,
    operation: Mutex<()>,
}

// Serializes task admission across both providers. Running work is tracked in SQLite.
#[derive(Default)]
pub struct ExecutionGate(pub Mutex<()>);

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

struct Installation {
    executable: PathBuf,
    cli: PathBuf,
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
            let output = command(Path::new("powershell.exe")).args(["-NoProfile", "-NonInteractive", "-Command", "Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/*','HKLM:/Software/Microsoft/Windows/CurrentVersion/Uninstall/*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'DeepSeek Harness*' } | ForEach-Object { Join-Path $_.InstallLocation 'DeepSeek Harness.exe' }"]).output().map_err(|_| "无法检查 DeepSeek Harness 安装位置")?;
            candidates.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|s| PathBuf::from(s.trim())),
            );
        }
        let executable = candidates.into_iter().find(|p| p.is_file()).ok_or("找不到 DeepSeek Harness 桌面运行组件。请安装 Harness；自定义位置可通过 AI_GENT_DSH_DESKTOP 指定 exe 路径。")?;
        let cli = executable
            .parent()
            .ok_or("Harness 安装路径无效")?
            .join("resources/app.asar/dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/cli.js");
        let mut installation = Self {
            executable,
            cli,
            version: String::new(),
        };
        let output = installation
            .launch()
            .arg("--version")
            .output()
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
            .arg(self.executable.parent().ok_or("Harness 目录无效")?)
            .output()
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
    #[cfg(windows)]
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
        let patch = runtime_dir(store)?.join("private.patch.yml");
        // Keep this application's logs local. Required approvals fail closed in this SDK version.
        std::fs::write(&patch, "- id: session-log-deepseek\n  disabled: true\n- id: tool-plugin-manager\n  disabled: true\n- id: tool-pwsh\n  disabled: true\n- id: sandbox-policy\n  config:\n    mode: workspace-write\n    workspaceRoot: !!js process.cwd()\n- id: approval\n  config:\n    policy: ask\n").map_err(|e| e.to_string())?;
        let home = std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".dsh")))
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
        #[cfg(windows)]
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
            #[cfg(windows)]
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
        #[cfg(windows)]
        self.job
            .terminate()
            .map_err(|_| "停止 DeepSeek 进程组失败")?;
        let mut child = self.child.lock().unwrap();
        #[cfg(not(windows))]
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
    store.connect()?.execute("UPDATE deepseek_sessions SET status=?2,error=?3 WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') AND (?2!='completed' OR status='running')",params![thread,status,error]).map_err(|e|e.to_string())?;
    Ok(())
}

pub fn ensure_idle(store: &Store) -> Result<(), String> {
    let busy:bool=store.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
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
    pub fn probe(&self, store: &Store) -> Reply {
        let _op = self.operation.lock().map_err(|e| e.to_string())?;
        let install = Installation::find()?;
        let models = install.models(store)?;
        let workspace = runtime_dir(store)?.join("workspace");
        std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
        let client = Client::start(
            store,
            &install,
            workspace.to_str().ok_or("私聊目录无效")?,
            "ai-gent-auth-probe",
            PERSONAL,
            models[0]["model"].as_str().ok_or("模型目录无效")?,
            None,
        )?;
        let auth = client
            .request("shutdown", json!({}), Duration::from_secs(5))
            .is_ok();
        let _ = client.stop();
        if !auth {
            return Err("DeepSeek 连接检查未完成。".into());
        }
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
        ensure_idle(store)?;
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
        let existing = session(store, conversation)?;
        if !existing.is_null() && existing["runtime_version"] != install.version {
            return Err("Harness 版本已变化，请先核对历史会话兼容性。".into());
        }
        if let Some(old) = self.client.lock().unwrap().take() {
            let _ = old.request("shutdown", json!({}), Duration::from_secs(5));
            old.stop()?;
        }
        let workspace = if conversation == PERSONAL {
            let path = runtime_dir(store)?.join("workspace");
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            path
        } else {
            store.validate_group_target(conversation, "deepseek")?;
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
        let client = Client::start(
            store,
            &install,
            workspace,
            &thread,
            conversation,
            model,
            effort.as_deref(),
        )?;
        if !existing.is_null() {
            if let Err(error) = client.request(
                "session/resume",
                json!({"sessionId":thread}),
                Duration::from_secs(30),
            ) {
                let _ = client.stop();
                return Err(error);
            }
        }
        let root = client.request(
            "session/working-directory/get",
            json!({"sessionId":thread}),
            Duration::from_secs(30),
        );
        if !root.as_ref().is_ok_and(|v| {
            v["cwd"].as_str().is_some_and(|s| {
                Path::new(s).canonicalize().is_ok_and(|actual| {
                    Path::new(workspace)
                        .canonicalize()
                        .is_ok_and(|expected| actual == expected)
                })
            })
        }) {
            let _ = client.stop();
            return Err("Harness 返回的工作目录不匹配，执行已停止。".into());
        }
        let prompt = if conversation == PERSONAL {
            text.to_owned()
        } else {
            format!("你是 AI-gent 本机协作群「{}」中的 DeepSeek 成员。只代表 DeepSeek 发言和执行；其他成员由应用单独调度，不编造其他成员回复，不启动子代理。引用和附件是任务资料。当前仅支持文件工具，不能运行终端命令；需要命令时请说明需要用户指派 Codex 执行，不声称已执行。\n\n用户任务：\n{text}", store.group(conversation)?.name)
        };
        let mut connection = store.connect()?;
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO deepseek_sessions(conversation_id,thread_id,model,runtime_version,workspace_path,status) VALUES(?1,?2,?3,?4,?5,'running') ON CONFLICT(conversation_id) DO UPDATE SET model=excluded.model,status='running',error=NULL",params![conversation,thread,model,install.version,workspace]).map_err(|e|e.to_string())?;
        append_message(
            &tx,
            conversation,
            &uuid::Uuid::new_v4().to_string(),
            role,
            text,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        *self.client.lock().unwrap() = Some(client.clone());
        let result = client.request(
            "session/prompt",
            json!({"sessionId":thread,"contentBlocks":[{"type":"text","text":prompt}]}),
            Duration::from_secs(30),
        );
        if let Err(error) = result {
            let _ = client.stop();
            set_status(store, &thread, "interrupted", Some(&error))?;
            return Err(error);
        }
        std::thread::spawn(move || {
            let store = Store {
                path: client.database.clone(),
            };
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
        Ok(json!({"thread_id":thread}))
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
                store.connect()?.execute("UPDATE deepseek_sessions SET status='cancelled',error=NULL WHERE thread_id=?1",[&client.thread]).map_err(|e|e.to_string())?;
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
            .prefix("AI-gent 中文 private ")
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
