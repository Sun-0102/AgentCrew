use crate::storage::Store;
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
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

#[derive(Default)]
pub struct Runtime {
    client: Mutex<Option<Arc<Client>>>,
    operation: Mutex<()>,
}

struct Client {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    approvals: Mutex<HashMap<String, Value>>,
    loaded: Mutex<HashSet<String>>,
    next_id: AtomicU64,
    healthy: AtomicBool,
    database: PathBuf,
    version: String,
    executable: PathBuf,
    #[cfg(windows)]
    job: ProcessJob,
}

#[cfg(windows)]
pub(crate) struct ProcessJob(usize);

#[cfg(windows)]
impl ProcessJob {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(format!(
                    "无法建立 Codex 进程组：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(format!(
                    "无法绑定 Codex 进程组：{}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(job)
        }
    }

    pub(crate) fn terminate(&self) -> Result<(), String> {
        if unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0 as _, 1) }
            == 0
        {
            return Err(format!(
                "停止 Codex 进程组失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}

fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn pick_codex(
    candidates: impl IntoIterator<Item = (PathBuf, String)>,
) -> Result<(PathBuf, String), String> {
    candidates
        .into_iter()
        .filter_map(|(path, version)| {
            let parsed = semver::Version::parse(version.strip_prefix("codex-cli ")?).ok()?;
            Some((parsed, path, version))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path, version)| (path, version))
        .ok_or_else(|| {
            "找不到可用的 Codex。请安装 Codex CLI，或确认桌面客户端已安装运行组件。".into()
        })
}

fn resolve_codex() -> Result<(PathBuf, String), String> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }) {
        paths.push(path);
    }
    // Desktop updates can install a newer runtime without updating PATH.
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if let Ok(entries) = std::fs::read_dir(
            PathBuf::from(local)
                .join("OpenAI")
                .join("Codex")
                .join("bin"),
        ) {
            paths.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path().join(name))
                    .filter(|path| path.is_file()),
            );
        }
    }
    paths.sort();
    paths.dedup();
    let candidates = paths.into_iter().filter_map(|path| {
        let output = command(&path).arg("--version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Some((path, version))
    });
    pick_codex(candidates)
}

impl Client {
    fn start(database: PathBuf) -> Result<Arc<Self>, String> {
        let (executable, version) = resolve_codex()?;
        let mut child = command(&executable)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 Codex App Server：{e}"))?;
        #[cfg(windows)]
        let job = match ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let input = child.stdin.take().ok_or("Codex stdin 不可用")?;
        let output = child.stdout.take().ok_or("Codex stdout 不可用")?;
        let stderr = child.stderr.take().ok_or("Codex stderr 不可用")?;
        let client = Arc::new(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            loaded: Mutex::new(HashSet::new()),
            next_id: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            database,
            version,
            executable,
            #[cfg(windows)]
            job,
        });
        // Drain diagnostics separately. Never put authentication diagnostics in the UI or database.
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
        });
        let weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Some(client) = weak.upgrade() else { break };
                let result = line.map_err(|e| e.to_string()).and_then(|line| {
                    serde_json::from_str::<Value>(&line).map_err(|e| e.to_string())
                });
                match result {
                    Ok(message) => {
                        if let Err(error) = client.receive(message) {
                            client.disconnected(&error);
                            break;
                        }
                    }
                    Err(_) => {
                        client.disconnected("Codex 协议读取失败，执行结果未知。");
                        break;
                    }
                }
            }
            if let Some(client) = weak.upgrade() {
                client.disconnected("Codex 连接已断开，未完成任务的结果未知。");
            }
        });
        client.request("initialize", json!({"clientInfo":{"name":"ai_gent","title":"AI-gent","version":env!("CARGO_PKG_VERSION")}}))?;
        client.write(json!({"method":"initialized"}))?;
        Ok(client)
    }

    fn write(&self, value: Value) -> Result<(), String> {
        let mut input = self.input.lock().map_err(|e| e.to_string())?;
        writeln!(input, "{value}")
            .and_then(|_| input.flush())
            .map_err(|e| format!("Codex 写入失败：{e}"))
    }

    fn request(&self, method: &str, params: Value) -> Reply {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, send);
        let result = self
            .write(json!({"id":id,"method":method,"params":params}))
            .and_then(|_| match receive.recv_timeout(Duration::from_secs(45)) {
                Ok(reply) => reply,
                Err(_) => {
                    let _ = self.stop();
                    Err(format!(
                        "Codex 请求超时：{method}；后台已停止，请核对已产生的文件。"
                    ))
                }
            });
        self.pending.lock().unwrap().remove(&id);
        result
    }

    fn receive(&self, message: Value) -> Result<(), String> {
        if message.get("method").is_none() {
            if let Some(id) = message["id"].as_u64() {
                if let Some(send) = self.pending.lock().unwrap().remove(&id) {
                    let reply = if message.get("error").is_some() {
                        Err(message["error"]["message"]
                            .as_str()
                            .unwrap_or("Codex 请求失败")
                            .to_owned())
                    } else {
                        Ok(message["result"].clone())
                    };
                    let _ = send.send(reply);
                }
            }
            return Ok(());
        }
        let method = message["method"].as_str().unwrap_or("");
        let p = &message["params"];
        if let Some(id) = message.get("id") {
            if matches!(
                method,
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
            ) {
                let connection = Store {
                    path: self.database.clone(),
                }
                .connect()?;
                let valid = connection.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE thread_id=?1 AND turn_id=?2 AND status IN ('running','waiting_approval'))", params![p["threadId"].as_str(),p["turnId"].as_str()], |row| row.get::<_,bool>(0)).map_err(|e|e.to_string())?;
                if !valid {
                    return self.write(json!({"id":id,"result":{"decision":"decline"}}));
                }
                self.approvals
                    .lock()
                    .unwrap()
                    .insert(uuid::Uuid::new_v4().to_string(), message.clone());
                self.update_status(
                    p["threadId"].as_str().unwrap_or(""),
                    "waiting_approval",
                    None,
                )?;
            } else {
                self.write(json!({"id":id,"error":{"code":-32601,"message":"AI-gent does not support this request"}}))?;
            }
            return Ok(());
        }
        let thread = p["threadId"].as_str().unwrap_or("");
        if thread.is_empty() {
            return Ok(());
        }
        let connection = Store {
            path: self.database.clone(),
        }
        .connect()?;
        let current = connection
            .query_row(
                "SELECT turn_id,status FROM codex_sessions WHERE thread_id=?1",
                [thread],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();
        if let Some((current_turn, status)) = current {
            let incoming = p["turnId"].as_str().or_else(|| p["turn"]["id"].as_str());
            if method == "turn/started" {
                if status != "starting" {
                    return Ok(());
                }
            } else if incoming.is_some() && incoming != current_turn.as_deref() {
                return Ok(());
            }
        }
        match method {
            "turn/started" => {
                connection.execute("UPDATE codex_sessions SET status='running', turn_id=?2, error=NULL WHERE thread_id=?1", params![thread, p["turn"]["id"].as_str()]).map_err(|e| e.to_string())?;
            }
            "item/agentMessage/delta" => {
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,'assistant',?3 FROM codex_sessions WHERE thread_id=?1 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text || excluded.text", params![thread, p["itemId"].as_str().unwrap_or(""), p["delta"].as_str().unwrap_or("")]).map_err(|e| e.to_string())?;
            }
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let kind = item["type"].as_str().unwrap_or("");
                let text = match kind {
                    "agentMessage" if method == "item/completed" => {
                        item["text"].as_str().unwrap_or("").to_owned()
                    }
                    "commandExecution" => format!(
                        "命令：{}\n状态：{}\n{}",
                        item["command"].as_str().unwrap_or(""),
                        item["status"].as_str().unwrap_or(""),
                        item["aggregatedOutput"].as_str().unwrap_or("")
                    ),
                    "fileChange" => format!("文件变更：{}", item["changes"]),
                    "imageGeneration" if method == "item/completed" => {
                        generated_image_message(&Store { path: self.database.clone() }, item)
                    }
                    _ => return Ok(()),
                };
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,?3,?4 FROM codex_sessions WHERE thread_id=?1 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text", params![thread, item["id"].as_str().unwrap_or(""), if matches!(kind, "agentMessage" | "imageGeneration") { "assistant" } else { "tool" }, text]).map_err(|e| e.to_string())?;
            }
            "turn/completed" => {
                let status = match p["turn"]["status"].as_str() {
                    Some("completed") => "completed",
                    Some("interrupted") => {
                        if current_status(&connection, thread).as_deref()
                            == Some("cancel_requested")
                        {
                            "stopping"
                        } else {
                            "cancelled"
                        }
                    }
                    _ => "failed",
                };
                self.update_status(thread, status, p["turn"]["error"]["message"].as_str())?;
                self.approvals
                    .lock()
                    .unwrap()
                    .retain(|_, value| value["params"]["threadId"] != thread);
                let root: Result<String,_> = connection.query_row("SELECT workspace_path FROM conversations c JOIN codex_sessions s ON c.id=s.conversation_id WHERE s.thread_id=?1",[thread],|row|row.get(0));
                if let Ok(root) = root {
                    if let Err(error) = crate::codex_sidebar::sync(&self.database, thread, &root) {
                        let _ = connection.execute(
                            "UPDATE codex_sessions SET error=COALESCE(error,?2) WHERE thread_id=?1",
                            params![thread, format!("任务已结束；同步官方最近列表失败：{error}")],
                        );
                    }
                }
            }
            "error" => {
                connection
                    .execute(
                        "UPDATE codex_sessions SET error=?2 WHERE thread_id=?1",
                        params![
                            thread,
                            p["error"]["message"].as_str().unwrap_or("Codex 执行出错")
                        ],
                    )
                    .map_err(|e| e.to_string())?;
            }
            _ => (),
        }
        Ok(())
    }

    fn update_status(&self, thread: &str, status: &str, error: Option<&str>) -> Result<(), String> {
        let mut connection = Store {
            path: self.database.clone(),
        }
        .connect()?;
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        if matches!(status, "completed" | "cancelled" | "failed" | "interrupted") {
            tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,'task_end',?3 FROM codex_sessions WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')", params![thread, uuid::Uuid::new_v4().to_string(), status]).map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE codex_sessions SET status=?2,error=?3 WHERE thread_id=?1",
            params![thread, status, error],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn disconnected(&self, error: &str) {
        if !self.healthy.swap(false, Ordering::SeqCst) {
            return;
        }
        for (_, send) in self.pending.lock().unwrap().drain() {
            let _ = send.send(Err(error.to_owned()));
        }
        for thread in self.loaded.lock().unwrap().iter() {
            if let Ok(connection) = (Store {
                path: self.database.clone(),
            })
            .connect()
            {
                let _ = connection.execute("UPDATE codex_sessions SET status='interrupted',error=?2 WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')", params![thread,error]);
            }
        }
        self.approvals.lock().unwrap().clear();
    }

    fn stop(&self) -> Result<(), String> {
        #[cfg(windows)]
        self.job.terminate()?;
        if let Ok(mut child) = self.child.lock() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn model_catalog(client: &Client) -> Result<Vec<Value>, String> {
    let mut models = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let page = client.request(
            "model/list",
            json!({"cursor":cursor,"limit":100,"includeHidden":false}),
        )?;
        models.extend(
            page["data"]
                .as_array()
                .ok_or("模型目录格式不兼容")?
                .iter()
                .cloned(),
        );
        let next = page["nextCursor"].clone();
        if next.is_null() {
            break;
        }
        if next == cursor {
            return Err("模型目录分页游标重复".into());
        }
        cursor = next;
    }
    Ok(models)
}

impl Runtime {
    fn client(&self, store: &Store) -> Result<Arc<Client>, String> {
        let mut slot = self.client.lock().map_err(|e| e.to_string())?;
        if let Some(client) = slot.as_ref() {
            if client.healthy.load(Ordering::SeqCst)
                && client
                    .child
                    .lock()
                    .unwrap()
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_none()
            {
                return Ok(client.clone());
            }
            client.stop()?;
            client.disconnected("Codex 后台连接已重建，上次执行结果未知。");
        }
        let client = Client::start(store.path.clone())?;
        *slot = Some(client.clone());
        Ok(client)
    }

    pub fn shutdown(&self) {
        if let Some(client) = self.client.lock().unwrap().take() {
            let _ = client.stop();
            client.disconnected("Codex 后台已停止。");
        }
    }

    pub fn sync_personal_sidebar(&self, store: &Store) -> Result<(), String> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let id = store.conversation_id(None)?;
        let Some(existing) = session(store, &id)? else {
            return Ok(());
        };
        if active(existing["status"].as_str().unwrap_or("")) {
            return Ok(());
        }
        let client = self.client(store)?;
        sync_personal_thread(&client, store, &existing)
    }

    pub fn probe(&self, store: &Store) -> Reply {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let client = self.client(store)?;
        let account = client.request("account/read", json!({"refreshToken":false}))?;
        let models = model_catalog(&client)?;
        let mut sidebar_warning = None;
        if let Some(existing) = session(store, "codex-personal")? {
            if !active(existing["status"].as_str().unwrap_or("")) {
                sidebar_warning = sync_personal_thread(&client, store, &existing).err();
            }
        }
        Ok(
            json!({"version":client.version,"executable":client.executable,"auth":account["account"]["type"],"requires_auth":account["requiresOpenaiAuth"],"models":models,"sidebar_warning":sidebar_warning}),
        )
    }

    pub fn send(&self, store: &Store, conversation_id: &str, model: &str, text: &str) -> Reply {
        self.send_recorded(store, conversation_id, model, text, "user")
    }

    pub(crate) fn send_recorded(
        &self,
        store: &Store,
        conversation_id: &str,
        model: &str,
        text: &str,
        role: &str,
    ) -> Reply {
        if text.trim().is_empty() || text.len() > 100_000 {
            return Err("消息必须为 1–100000 字节。".into());
        }
        if model.is_empty() {
            return Err("请先选择模型。".into());
        }
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let connection = store.connect()?;
        let root = store.conversation_root(conversation_id)?;
        if !std::path::Path::new(&root).is_dir() {
            return Err("会话工作目录已不存在".into());
        }
        let client = self.client(store)?;
        let effort = crate::reasoning::read(store, conversation_id, "codex", model)?;
        let tier = crate::speed::read(store, conversation_id, model)?;
        if effort.is_some() || tier.is_some() {
            let models = json!(model_catalog(&client)?);
            crate::reasoning::validate(&models, model, effort.as_deref())?;
            crate::speed::validate(&models, model, tier.as_deref())?;
        }
        let existing = session(store, conversation_id)?;
        let any_running: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[],|row|row.get(0)).map_err(|e|e.to_string())?;
        if any_running {
            return Err("当前已有会话执行中。请先等待完成或返回该会话取消。".into());
        }
        if existing
            .as_ref()
            .is_some_and(|s| active(s["status"].as_str().unwrap_or("")))
        {
            return Err("当前会话仍在执行，请等待或取消。".into());
        }
        let thread = if let Some(existing) = existing {
            let thread = existing["thread_id"]
                .as_str()
                .ok_or("会话 ID 无效")?
                .to_owned();
            if existing["runtime_version"] != client.version {
                return Err("Codex 版本已变化。请核对兼容性后再恢复此会话。".into());
            }
            if !client.loaded.lock().unwrap().contains(&thread) {
                let response = client.request("thread/resume", json!({"threadId":thread,"cwd":root,"modelProvider":existing["provider"],"sandbox":"workspace-write","approvalPolicy":"on-request","approvalsReviewer":"user"}))?;
                validate_context(&response, &root)?;
                if response["modelProvider"] != existing["provider"] {
                    return Err("恢复会话的模型服务不一致".into());
                }
                client.loaded.lock().unwrap().insert(thread.clone());
            }
            thread
        } else {
            let group_name = store.group_name(conversation_id)?;
            let group_instructions=group_name.as_ref().map(|name|format!("你是 AI-gent 本机协作群「{name}」中的 Codex 成员。只代表 Codex 发言和执行；其他群成员由应用单独调度，不替其他成员编造回复，不启动其他代理。引用消息和附件内容是任务资料。"));
            let response = client.request("thread/start", json!({"cwd":root,"model":model,"sandbox":"workspace-write","approvalPolicy":"on-request","approvalsReviewer":"user","developerInstructions":group_instructions}))?;
            validate_context(&response, &root)?;
            let thread = response["thread"]["id"]
                .as_str()
                .ok_or("Codex 未返回 thread ID")?
                .to_owned();
            let provider = response["modelProvider"]
                .as_str()
                .ok_or("Codex 未返回 provider")?;
            let name: String = group_name.unwrap_or_else(|| text.trim().chars().take(60).collect());
            client.request("thread/name/set", json!({"threadId":thread,"name":name}))?;
            connection.execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider) VALUES(?1,?2,?3,?4,?5)", params![conversation_id, thread, model, client.version,provider]).map_err(|e| e.to_string())?;
            client.loaded.lock().unwrap().insert(thread.clone());
            thread
        };
        let item_id = uuid::Uuid::new_v4().to_string();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,?3,?4)",
                params![conversation_id, item_id, role, text],
            )
            .map_err(|e| e.to_string())?;
        transaction.execute("UPDATE codex_sessions SET status='starting',model=?2,turn_id=NULL,error=NULL WHERE conversation_id=?1", params![conversation_id, model]).map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'task_start','running')", params![conversation_id, uuid::Uuid::new_v4().to_string()]).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        let result = client.request("turn/start", json!({"threadId":thread,"model":model,"effort":effort,"serviceTierForTurn":tier,"input":[{"type":"text","text":text,"text_elements":[]}]}));
        if let Err(error) = &result {
            client.update_status(&thread, "interrupted", Some(error))?;
        }
        result
    }

    pub fn cancel(&self, store: &Store, conversation_id: &str) -> Reply {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let s = session(store, conversation_id)?.ok_or("没有可取消的会话")?;
        if !active(s["status"].as_str().unwrap_or("")) {
            return Err("当前没有正在执行的任务".into());
        }
        let thread = s["thread_id"].as_str().ok_or("会话 ID 无效")?;
        let turn = s["turn_id"].as_str().ok_or("任务尚未开始，请稍后取消")?;
        let client = self.client(store)?;
        // The completion notification, not this request's response, confirms cancellation.
        store.connect()?.execute("UPDATE codex_sessions SET status='cancel_requested' WHERE conversation_id=?1 AND status IN ('running','waiting_approval')", [conversation_id]).map_err(|e| e.to_string())?;
        let result = client.request("turn/interrupt", json!({"threadId":thread,"turnId":turn}));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while result.is_ok() && std::time::Instant::now() < deadline {
            if session(store, conversation_id)?.is_some_and(|s| s["status"] == "stopping") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let confirmed = session(store, conversation_id)?.is_some_and(|s| s["status"] == "stopping");
        client.stop()?;
        client.disconnected("Codex 后台已停止。");
        client.update_status(
            thread,
            if confirmed {
                "cancelled"
            } else {
                "interrupted"
            },
            if confirmed {
                None
            } else {
                Some("后台已停止，但未收到取消终态；请核对已产生的文件。")
            },
        )?;
        result
    }

    pub fn approve(&self, store: &Store, conversation_id: &str, id: &str, allow: bool) -> Reply {
        let client = self.client(store)?;
        let s = session(store, conversation_id)?.ok_or("会话不存在")?;
        let mut approvals = client.approvals.lock().unwrap();
        let request = approvals.get(id).ok_or("审批已失效")?;
        if request["params"]["threadId"] != s["thread_id"]
            || request["params"]["turnId"] != s["turn_id"]
            || !active(s["status"].as_str().unwrap_or(""))
        {
            return Err("审批不属于当前会话".into());
        }
        client.write(json!({"id":request["id"],"result":{"decision":if allow { "accept" } else { "decline" }}}))?;
        approvals.remove(id);
        store.connect()?.execute("UPDATE codex_sessions SET status='running' WHERE conversation_id=?1 AND status='waiting_approval'", [conversation_id]).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    pub fn snapshot(&self, store: &Store, conversation_id: &str) -> Reply {
        let connection = store.connect()?;
        let mut statement = connection.prepare("SELECT item_id,role,text,created_at,agent FROM codex_messages WHERE conversation_id=?1 ORDER BY sequence").map_err(|e| e.to_string())?;
        let messages = statement.query_map([conversation_id], |row| Ok(json!({"id":row.get::<_,String>(0)?,"role":row.get::<_,String>(1)?,"text":row.get::<_,String>(2)?,"created_at":row.get::<_,Option<String>>(3)?,"agent":row.get::<_,String>(4)?}))).map_err(|e| e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?;
        let s = session(store, conversation_id)?;
        let mut approvals = Vec::new();
        if let Some(client) = self.client.lock().unwrap().as_ref() {
            for (id, request) in client.approvals.lock().unwrap().iter() {
                if s.as_ref()
                    .is_some_and(|s| request["params"]["threadId"] == s["thread_id"])
                {
                    let p = &request["params"];
                    approvals.push(json!({"id":id,"method":request["method"],"command":p["command"],"reason":p["reason"],"cwd":p["cwd"]}));
                }
            }
        }
        Ok(json!({"session":s,"messages":messages,"approvals":approvals}))
    }
}

fn sync_personal_thread(client: &Client, store: &Store, existing: &Value) -> Result<(), String> {
    let thread = existing["thread_id"].as_str().ok_or("私聊会话 ID 无效")?;
    if client.loaded.lock().unwrap().contains(thread) {
        return crate::codex_sidebar::sync(
            &store.path,
            thread,
            &store.conversation_root("codex-personal")?,
        );
    }
    if existing["runtime_version"] != client.version {
        return Err("Codex 版本已变化，请先核对会话兼容性。".into());
    }
    let root = store.conversation_root("codex-personal")?;
    let response = client.request("thread/resume",json!({"threadId":thread,"cwd":root,"modelProvider":existing["provider"],"sandbox":"workspace-write","approvalPolicy":"on-request","approvalsReviewer":"user"}))?;
    validate_context(&response, &root)?;
    if response["modelProvider"] != existing["provider"] {
        return Err("私聊模型服务不匹配".into());
    }
    let name = response["thread"]["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "AI-gent 私聊".into());
    // Updating the title also makes the changed cwd visible to Desktop's incremental catalog scan.
    client.request("thread/name/set", json!({"threadId":thread,"name":name}))?;
    crate::codex_sidebar::sync(&store.path, thread, &root)?;
    store.connect()?.execute("UPDATE codex_sessions SET error=NULL WHERE conversation_id='codex-personal' AND (error LIKE '同步官方最近列表失败：%' OR error LIKE '任务已结束；同步官方最近列表失败：%')",[]).map_err(|e|e.to_string())?;
    client.loaded.lock().unwrap().insert(thread.to_owned());
    Ok(())
}

fn session(store: &Store, conversation_id: &str) -> Result<Option<Value>, String> {
    use rusqlite::OptionalExtension;
    store.connect()?.query_row("SELECT thread_id,model,runtime_version,status,turn_id,error,provider FROM codex_sessions WHERE conversation_id=?1", [conversation_id], |row| Ok(json!({"thread_id":row.get::<_,String>(0)?,"model":row.get::<_,String>(1)?,"runtime_version":row.get::<_,String>(2)?,"status":row.get::<_,String>(3)?,"turn_id":row.get::<_,Option<String>>(4)?,"error":row.get::<_,Option<String>>(5)?,"provider":row.get::<_,String>(6)?}))).optional().map_err(|e| e.to_string())
}

pub(crate) fn active(status: &str) -> bool {
    matches!(
        status,
        "starting" | "running" | "waiting_approval" | "cancel_requested" | "stopping"
    )
}

fn validate_context(response: &Value, root: &str) -> Result<(), String> {
    let cwd = response["cwd"].as_str().ok_or("Codex 未返回执行目录")?;
    let actual = std::fs::canonicalize(cwd).map_err(|e| format!("无法核对 Codex 执行目录：{e}"))?;
    let expected = std::fs::canonicalize(root).map_err(|e| format!("无法核对项目目录：{e}"))?;
    if actual != expected {
        return Err("Codex 执行目录与所选项目不一致".into());
    }
    if response["sandbox"]["type"] != "workspaceWrite"
        || response["approvalPolicy"] != "on-request"
        || response["approvalsReviewer"] != "user"
    {
        return Err("Codex 未采用项目写入沙箱及用户审批，任务未发送。".into());
    }
    Ok(())
}

fn generated_image_message(store: &Store, item: &Value) -> String {
    if item["status"] != "completed" || !item["failure"].is_null() {
        return "图片生成未完成，请重试。".into();
    }
    if let Some(data) = item["result"].as_str() {
        return match crate::images::save(store, data) {
            Ok(url) => format!("![生成的图片]({url})"),
            Err(error) => format!("图片保存失败：{error}"),
        };
    }
    "图片生成工具未返回可显示的图片数据。".into()
}

fn current_status(connection: &rusqlite::Connection, thread: &str) -> Option<String> {
    connection
        .query_row(
            "SELECT status FROM codex_sessions WHERE thread_id=?1",
            [thread],
            |row| row.get(0),
        )
        .ok()
}

pub fn recover(store: &Store) -> Result<(), String> {
    store.connect()?.execute("UPDATE codex_sessions SET status='interrupted',error='程序已重启，上次执行结果未知；请核对文件后继续。' WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')", []).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Store, String, Client) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let project = store.add_project(dir.path().to_str().unwrap()).unwrap();
        store.connect().unwrap().execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider,status) VALUES(?1,'thread','model','test','test','starting')", [&project.id]).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let job = ProcessJob::attach(&child).unwrap();
        let client = Client {
            input: Mutex::new(child.stdin.take().unwrap()),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            loaded: Mutex::new(HashSet::from(["thread".to_owned()])),
            next_id: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            database: store.path.clone(),
            version: "test".into(),
            executable: PathBuf::from("test-codex"),
            #[cfg(windows)]
            job,
        };
        (dir, store, project.id, client)
    }

    #[test]
    #[ignore = "Uses two short model turns to verify explicit reasoning selection and resume"]
    fn installed_codex_reasoning_selection() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let project = store.add_project(temp.path().to_str().unwrap()).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let model = &probe["models"][0];
        let id = model["model"].as_str().unwrap();
        let choices = model["supportedReasoningEfforts"].as_array().unwrap();
        assert!(choices.len() >= 2);
        let mut thread = Value::Null;
        for choice in [choices.first().unwrap(), choices.last().unwrap()] {
            let effort = choice["reasoningEffort"].as_str().unwrap();
            crate::reasoning::save(&store, &project.id, "codex", id, Some(effort)).unwrap();
            runtime
                .send(
                    &store,
                    &project.id,
                    id,
                    "不要用工具或子代理。只回复 REASONING_OK。",
                )
                .unwrap();
            let result = wait(&runtime, &store, &project.id);
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
            println!("Codex explicit effort {effort} completed");
        }
        runtime.shutdown();
    }

    #[test]
    #[ignore = "Uses two short model turns to verify fast/standard speed with unchanged reasoning"]
    fn installed_codex_speed_selection() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let project = store.add_project(temp.path().to_str().unwrap()).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let model = probe["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| {
                m["serviceTiers"]
                    .as_array()
                    .is_some_and(|tiers| tiers.iter().any(|t| t["id"] == "priority"))
            })
            .expect("account must advertise Fast");
        let id = model["model"].as_str().unwrap();
        let effort = model["defaultReasoningEffort"].as_str().unwrap();
        crate::reasoning::save(&store, &project.id, "codex", id, Some(effort)).unwrap();
        let mut thread = Value::Null;
        for tier in ["priority", "default"] {
            crate::speed::save(&store, &project.id, id, Some(tier)).unwrap();
            runtime
                .send(
                    &store,
                    &project.id,
                    id,
                    "不要用工具或子代理。只回复 SPEED_OK。",
                )
                .unwrap();
            let result = wait(&runtime, &store, &project.id);
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
                .contains("SPEED_OK"));
            assert_eq!(
                crate::reasoning::read(&store, &project.id, "codex", id)
                    .unwrap()
                    .as_deref(),
                Some(effort)
            );
            println!("Codex speed {tier} completed; reasoning stays {effort}");
        }
        runtime.shutdown();
    }

    #[test]
    fn generated_images_are_saved_as_visible_replies() {
        let (_dir, store, project, client) = fixture();
        client.receive(json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"turn"}}})).unwrap();
        let image = json!({"id":"image","type":"imageGeneration","status":"completed","result":"iVBORw0KGgo=","failure":null});
        for method in ["item/started", "item/completed", "item/completed"] {
            client.receive(json!({"method":method,"params":{"threadId":"thread","turnId":"turn","item":image}})).unwrap();
        }
        let snapshot = Runtime::default().snapshot(&store, &project).unwrap();
        let messages = snapshot["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "assistant");
        let text = messages[0]["text"].as_str().unwrap();
        assert!(text.starts_with("![生成的图片](ai-gent-image:"));
        assert!(text.len() < 150);
        let id = text.split("ai-gent-image:").nth(1).unwrap().trim_end_matches(')');
        assert_eq!(crate::images::read(&store,id).unwrap(),"data:image/png;base64,iVBORw0KGgo=");
        assert!(!generated_image_message(&store, &json!({"status":"failed","result":"iVBORw0KGgo="})).contains("!["));
        for data in ["", "not an image", "iVBORw0KGgo=) injected"] {
            assert!(!generated_image_message(&store, &json!({"status":"completed","result":data})).contains("!["));
        }
    }

    #[test]
    fn newer_desktop_runtime_wins_over_old_path_and_invalid_versions() {
        let candidates = vec![
            (PathBuf::from("invalid"), "unrelated tool".into()),
            (
                PathBuf::from("desktop"),
                "codex-cli 0.169.0-alpha.11.1".into(),
            ),
            (PathBuf::from("path"), "codex-cli 0.147.0-alpha.6.6".into()),
        ];
        assert_eq!(pick_codex(candidates).unwrap().0, PathBuf::from("desktop"));
        assert_eq!(
            pick_codex([(PathBuf::from("cli"), "codex-cli 0.169.0".into())])
                .unwrap()
                .0,
            PathBuf::from("cli")
        );
        assert!(pick_codex(Vec::new()).is_err());
    }

    #[test]
    fn streamed_messages_terminal_states_and_late_events() {
        let (_dir, store, project, client) = fixture();
        client.receive(json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"turn"}}})).unwrap();
        for delta in ["你", "好"] {
            client.receive(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread","turnId":"turn","itemId":"message","delta":delta}})).unwrap();
        }
        client.receive(json!({"method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"id":"message","type":"agentMessage","text":"你好！"}}})).unwrap();
        let text: String = store
            .connect()
            .unwrap()
            .query_row("SELECT text FROM codex_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "你好！");
        client.receive(json!({"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"old-turn","status":"completed"}}})).unwrap();
        assert_eq!(
            session(&store, &project).unwrap().unwrap()["status"],
            "running"
        );
        client.receive(json!({"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"interrupted"}}})).unwrap();
        assert_eq!(
            session(&store, &project).unwrap().unwrap()["status"],
            "cancelled"
        );
        client.disconnected("disconnected");
        let end: (String, String) = store.connect().unwrap().query_row("SELECT text,created_at FROM codex_messages WHERE role='task_end'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(end.0, "cancelled");
        assert!(!end.1.is_empty());
        assert_eq!(
            session(&store, &project).unwrap().unwrap()["status"],
            "cancelled"
        );
    }

    #[test]
    fn rpc_ids_errors_approvals_and_restart_recovery() {
        let (_dir, store, project, client) = fixture();
        let (send, receive) = mpsc::channel();
        client.pending.lock().unwrap().insert(8, send);
        client
            .receive(json!({"id":8,"error":{"code":-1,"message":"test error"}}))
            .unwrap();
        assert_eq!(receive.recv().unwrap().unwrap_err(), "test error");
        client.receive(json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"turn"}}})).unwrap();
        client.receive(json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"threadId":"thread","turnId":"turn","command":"echo test"}})).unwrap();
        assert_eq!(
            session(&store, &project).unwrap().unwrap()["status"],
            "waiting_approval"
        );
        assert_eq!(client.approvals.lock().unwrap().len(), 1);
        recover(&store).unwrap();
        assert_eq!(
            session(&store, &project).unwrap().unwrap()["status"],
            "interrupted"
        );
        assert!(Runtime::default()
            .snapshot(&store, "another-project")
            .unwrap()["messages"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn mismatched_execution_context_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let mut response = json!({"cwd":root,"sandbox":{"type":"workspaceWrite"},"approvalPolicy":"on-request","approvalsReviewer":"user"});
        validate_context(&response, root).unwrap();
        response["sandbox"]["type"] = json!("dangerFullAccess");
        assert!(validate_context(&response, root).is_err());
        response["sandbox"]["type"] = json!("workspaceWrite");
        response["cwd"] = json!(dir.path().join("missing"));
        assert!(validate_context(&response, root).is_err());
    }

    #[test]
    #[ignore = "Starts the installed Codex App Server; no model call"]
    fn installed_codex_probe() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let runtime = Runtime::default();
        let result = runtime.probe(&store).unwrap();
        println!("executable={}", result["executable"]);
        println!(
            "version={} auth={} models={}",
            result["version"],
            result["auth"],
            result["models"].as_array().unwrap().len()
        );
        for model in result["models"].as_array().unwrap() {
            println!(
                "model={} default={} speed={}",
                model["model"], model["isDefault"], model["serviceTiers"]
            );
        }
        runtime.shutdown();
    }

    fn wait(runtime: &Runtime, store: &Store, project: &str) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(150);
        loop {
            let snapshot = runtime.snapshot(store, project).unwrap();
            if !active(snapshot["session"]["status"].as_str().unwrap_or("")) {
                assert_eq!(snapshot["session"]["status"], "completed", "{snapshot}");
                return snapshot;
            }
            assert!(
                snapshot["approvals"].as_array().unwrap().is_empty(),
                "smoke task unexpectedly needs approval; no automatic grant"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "model response timed out"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    #[test]
    #[ignore = "Uses the logged-in account for three bounded model turns in a temporary project"]
    fn installed_codex_conversation_and_resume() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("中文 smoke project");
        std::fs::create_dir(&project_dir).unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let project = store.add_project(project_dir.to_str().unwrap()).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let models = probe["models"].as_array().unwrap();
        let model = models
            .iter()
            .find(|m| m["isDefault"] == true)
            .unwrap_or(&models[0])["model"]
            .as_str()
            .unwrap();
        let marker = format!("AI_GENT_{}", uuid::Uuid::new_v4());
        println!("smoke start model={model}");
        runtime.send(&store,&project.id,model,&format!("请记住暗号 {marker}。在当前目录创建 marker.txt，内容只写此暗号。不要访问其他目录，不要启动子代理。最后只回复暗号。")).unwrap();
        wait(&runtime, &store, &project.id);
        assert_eq!(
            std::fs::read_to_string(project_dir.join("marker.txt"))
                .unwrap()
                .trim(),
            marker
        );
        println!("file creation passed");
        runtime
            .send(
                &store,
                &project.id,
                model,
                "不要用工具。只回复我上一轮告诉你的暗号。",
            )
            .unwrap();
        let snapshot = wait(&runtime, &store, &project.id);
        assert!(snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&marker));
        let thread = snapshot["session"]["thread_id"].clone();
        println!("second turn context passed");
        runtime.shutdown();
        let resumed = Runtime::default();
        recover(&store).unwrap();
        resumed
            .send(
                &store,
                &project.id,
                model,
                "不要用工具。再回复我最初告诉你的暗号。",
            )
            .unwrap();
        let snapshot = wait(&resumed, &store, &project.id);
        assert_eq!(snapshot["session"]["thread_id"], thread);
        assert!(snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&marker));
        println!("restart resume passed");
        resumed.shutdown();
    }

    #[test]
    #[ignore = "Uses two model turns to verify a personal conversation without any project"]
    fn installed_codex_personal_chat_and_resume() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("personal.sqlite");
        let store = Store::open(database.clone()).unwrap();
        let id = store
            .personal_conversation_id(&dir.path().join("Documents/Codex/AI-gent"))
            .unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let models = probe["models"].as_array().unwrap();
        let model = models
            .iter()
            .find(|model| model["isDefault"] == true)
            .unwrap_or(&models[0])["model"]
            .as_str()
            .unwrap();
        let marker = format!("PRIVATE_{}", uuid::Uuid::new_v4());
        runtime
            .send(
                &store,
                &id,
                model,
                &format!("这是普通私聊，不要使用工具。请记住暗号 {marker}，只回复此暗号。"),
            )
            .unwrap();
        let snapshot = wait(&runtime, &store, &id);
        assert!(snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "assistant"
                && message["text"].as_str().unwrap().contains(&marker)));
        assert!(store.list_projects().unwrap().is_empty());
        let thread = snapshot["session"]["thread_id"].clone();
        runtime.shutdown();
        drop(store);
        let store = Store::open(database).unwrap();
        assert_eq!(
            store
                .personal_conversation_id(&dir.path().join("Documents/Codex/AI-gent"))
                .unwrap(),
            id
        );
        let runtime = Runtime::default();
        runtime
            .send(
                &store,
                &id,
                model,
                "不要使用工具。只回复我上一轮告诉你的暗号。",
            )
            .unwrap();
        let snapshot = wait(&runtime, &store, &id);
        assert_eq!(snapshot["session"]["thread_id"], thread);
        assert!(snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["role"] == "assistant")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&marker));
        assert!(store.list_projects().unwrap().is_empty());
        println!("personal chat and restart resume passed; zero projects created");
        runtime.shutdown();
    }

    #[test]
    #[ignore = "Uses one model turn to start and interrupt a temporary sleep command"]
    fn installed_codex_cancel_tool() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("中文 cancel project");
        std::fs::create_dir(&project_dir).unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let project = store.add_project(project_dir.to_str().unwrap()).unwrap();
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let models = probe["models"].as_array().unwrap();
        let model = models
            .iter()
            .find(|m| m["isDefault"] == true)
            .unwrap_or(&models[0])["model"]
            .as_str()
            .unwrap();
        runtime.send(&store,&project.id,model,"这是取消能力测试。只在当前项目运行一个 PowerShell 命令：先将该 PowerShell 的 $PID 写入 sleeper.pid，然后 Start-Sleep -Seconds 90。不要启动子代理，不要做其他操作。不要后台启动，等待命令结束。").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while !project_dir.join("sleeper.pid").exists() {
            let snapshot = runtime.snapshot(&store, &project.id).unwrap();
            assert!(
                snapshot["approvals"].as_array().unwrap().is_empty(),
                "sleep unexpectedly needs approval"
            );
            assert!(
                active(snapshot["session"]["status"].as_str().unwrap_or("")),
                "turn ended before starting sleep: {snapshot}"
            );
            assert!(std::time::Instant::now() < deadline, "sleep did not start");
            std::thread::sleep(Duration::from_millis(200));
        }
        let pid = std::fs::read_to_string(project_dir.join("sleeper.pid")).unwrap();
        let pid: u32 = pid.trim_start_matches('\u{feff}').trim().parse().unwrap();
        println!("sleep started; sending turn/interrupt");
        runtime.cancel(&store, &project.id).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let snapshot = runtime.snapshot(&store, &project.id).unwrap();
            if snapshot["session"]["status"] == "cancelled" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "missing interrupted terminal notification: {snapshot}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        #[cfg(windows)]
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let output = Command::new("tasklist.exe")
                    .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                    .output()
                    .unwrap();
                if !String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "cancelled tool process is still running after five seconds"
                );
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        println!("cancel terminal and tool process exit passed");
        runtime.shutdown();
    }

    #[test]
    #[ignore = "Uses one bounded model turn to verify a real group conversation"]
    fn installed_codex_group_chat() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("group.sqlite")).unwrap();
        let group = store
            .create_group_in(
                "协作群接入探针",
                vec!["codex".into(), "deepseek".into()],
                None,
                &dir.path().join("groups"),
            )
            .unwrap();
        store.validate_group_target(&group.id, "codex").unwrap();
        assert!(store.validate_group_target(&group.id, "deepseek").is_err());
        let runtime = Runtime::default();
        let probe = runtime.probe(&store).unwrap();
        let models = probe["models"].as_array().unwrap();
        let model = models
            .iter()
            .find(|m| m["isDefault"] == true)
            .unwrap_or(&models[0])["model"]
            .as_str()
            .unwrap();
        let marker = format!("GROUP_{}", uuid::Uuid::new_v4());
        runtime
            .send(
                &store,
                &group.id,
                model,
                &format!("@Codex 不要使用工具，只回复此标记：{marker}"),
            )
            .unwrap();
        let snapshot = wait(&runtime, &store, &group.id);
        assert!(snapshot["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "assistant" && m["text"].as_str().unwrap().contains(&marker)));
        assert!(
            runtime.snapshot(&store, "codex-personal").unwrap()["messages"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(store.list_projects().unwrap().is_empty());
        println!("group chat passed; private context empty; DeepSeek dispatch rejected");
        runtime.shutdown();
    }
}
