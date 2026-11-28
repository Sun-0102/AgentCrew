use super::*;

pub(super) struct Client {
    pub(super) child: Mutex<Child>,
    pub(super) input: Mutex<ChildStdin>,
    pub(super) pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    pub(super) approvals: Mutex<HashMap<String, Value>>,
    pub(super) loaded: Mutex<HashSet<String>>,
    pub(super) token_usage: Mutex<HashMap<String, LiveTokenUsage>>,
    pub(super) stderr_tail: Mutex<VecDeque<String>>,
    pub(super) next_id: AtomicU64,
    pub(super) healthy: AtomicBool,
    pub(super) database: PathBuf,
    pub(super) version: String,
    pub(super) executable: PathBuf,
    #[cfg(any(windows, unix))]
    pub(super) job: ProcessJob,
}

impl Client {
    pub(super) fn start(database: PathBuf) -> Result<Arc<Self>, String> {
        let (executable, version) = resolve_codex()?;
        let mut child = command(&executable)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 Codex App Server：{e}"))?;
        #[cfg(any(windows, unix))]
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
            token_usage: Mutex::new(HashMap::new()),
            stderr_tail: Mutex::new(VecDeque::with_capacity(20)),
            next_id: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            database,
            version,
            executable,
            #[cfg(any(windows, unix))]
            job,
        });
        // Drain diagnostics separately and retain a small sanitized tail (never auth tokens).
        let stderr_weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let Some(client) = stderr_weak.upgrade() else { break };
                        let line = String::from_utf8_lossy(&buf);
                        if let Some(cleaned) = sanitize_stderr_line(&line) {
                            if let Ok(mut tail) = client.stderr_tail.lock() {
                                if tail.len() >= 20 {
                                    tail.pop_front();
                                }
                                tail.push_back(cleaned);
                            }
                        }
                    }
                }
            }
        });
        let weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut buf = Vec::new();
            let mut cached_conn: Option<crate::storage::PooledConnection> = None;
            let mut disconnect_reason: Option<String> = None;
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Err(err) => {
                        disconnect_reason = Some(format!("Codex 输出流读取失败：{err}"));
                        break;
                    }
                    Ok(_) => {}
                }
                let Some(client) = weak.upgrade() else { break };
                let line = String::from_utf8_lossy(&buf);
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                    // Ignore stray non-JSON diagnostic lines instead of tearing down the shared app-server stream.
                    continue;
                };
                if let Err(error) = client.receive_with_connection(&mut cached_conn, message) {
                    // Reset cached SQLite connection on transient DB errors and keep draining stdout.
                    cached_conn = None;
                    if let Ok(mut tail) = client.stderr_tail.lock() {
                        if tail.len() >= 20 {
                            tail.pop_front();
                        }
                        if let Some(cleaned) = sanitize_stderr_line(&format!("receive error: {error}")) {
                            tail.push_back(cleaned);
                        }
                    }
                }
            }
            if let Some(client) = weak.upgrade() {
                let base = disconnect_reason
                    .unwrap_or_else(|| "Codex 连接已断开，未完成任务的结果未知。".into());
                let detail = match client.recent_stderr() {
                    Some(tail) => format!("{base}（{tail}）"),
                    None => base,
                };
                client.disconnected(&detail);
            }
        });
        client.request("initialize", json!({"clientInfo":{"name":"ai_gent","title":"AgentCrew","version":env!("CARGO_PKG_VERSION")}}))?;
        client.write(json!({"method":"initialized"}))?;
        Ok(client)
    }

    fn recent_stderr(&self) -> Option<String> {
        let tail = self.stderr_tail.lock().ok()?;
        let lines: Vec<&str> = tail.iter().rev().take(3).map(String::as_str).collect();
        if lines.is_empty() {
            None
        } else {
            let ordered: Vec<&str> = lines.into_iter().rev().collect();
            Some(ordered.join(" | "))
        }
    }

    pub(super) fn write(&self, value: Value) -> Result<(), String> {
        let mut input = self.input.lock().map_err(|e| e.to_string())?;
        writeln!(input, "{value}")
            .and_then(|_| input.flush())
            .map_err(|e| format!("Codex 写入失败：{e}"))
    }

    pub(super) fn request(&self, method: &str, params: Value) -> Reply {
        self.request_with_timeout(method, params, Duration::from_secs(45))
    }

    pub(super) fn request_with_timeout(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, send);
        let result = self
            .write(json!({"id":id,"method":method,"params":params}))
            .and_then(|_| match receive.recv_timeout(timeout) {
                Ok(reply) => reply,
                Err(_) => Err(format!("Codex 请求超时：{method}")),
            });
        self.pending.lock().unwrap().remove(&id);
        result
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn receive(&self, message: Value) -> Result<(), String> {
        let mut cached = None;
        self.receive_with_connection(&mut cached, message)
    }

    pub(super) fn receive_with_connection(
        &self,
        cached_conn: &mut Option<crate::storage::PooledConnection>,
        message: Value,
    ) -> Result<(), String> {
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
                self.write(json!({"id":id,"error":{"code":-32601,"message":"AgentCrew does not support this request"}}))?;
            }
            return Ok(());
        }
        let thread = p["threadId"].as_str().unwrap_or("");
        if thread.is_empty() {
            return Ok(());
        }
        if cached_conn.is_none() {
            *cached_conn = Some(
                Store {
                    path: self.database.clone(),
                }
                .connect()?,
            );
        }
        let connection = cached_conn.as_ref().unwrap();
        let current = connection
            .query_row(
                "SELECT conversation_id,turn_id,status FROM codex_sessions WHERE thread_id=?1",
                [thread],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .ok();
        let conversation_id = current.as_ref().map(|(cid, _, _)| cid.clone());
        if let Some((_, current_turn, status)) = current {
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
                if let Some(cid) = conversation_id.as_deref() {
                    crate::events::emit_conversation_sync(cid);
                }
            }
            "item/agentMessage/delta" => {
                let item_id = p["itemId"].as_str().unwrap_or("");
                let delta = p["delta"].as_str().unwrap_or("");
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,'assistant',?3 FROM codex_sessions WHERE thread_id=?1 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text || excluded.text", params![thread, item_id, delta]).map_err(|e| e.to_string())?;
                if let Some(cid) = conversation_id.as_deref() {
                    crate::events::emit_delta(cid, item_id, "assistant", Some("codex"), "append", delta);
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                let item_id = p["itemId"].as_str().unwrap_or("");
                let delta = p["delta"].as_str().unwrap_or("");
                if !item_id.is_empty() && !delta.is_empty() {
                    let reasoning_id = format!("reasoning:{item_id}");
                    connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,'thinking',?3 FROM codex_sessions WHERE thread_id=?1 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=text || excluded.text", params![thread, &reasoning_id, delta]).map_err(|e| e.to_string())?;
                    if let Some(cid) = conversation_id.as_deref() {
                        crate::events::emit_delta(cid, reasoning_id, "thinking", Some("codex"), "append", delta);
                    }
                }
            }
            "item/reasoning/summaryPartAdded" => {
                let item_id = p["itemId"].as_str().unwrap_or("");
                if !item_id.is_empty() {
                    connection.execute("UPDATE codex_messages SET text=CASE WHEN text='' OR text LIKE '%' || char(10) || char(10) THEN text ELSE text || char(10) || char(10) END WHERE conversation_id=(SELECT conversation_id FROM codex_sessions WHERE thread_id=?1) AND item_id=?2", params![thread, format!("reasoning:{item_id}")]).map_err(|e| e.to_string())?;
                    if let Some(cid) = conversation_id.as_deref() {
                        crate::events::emit_sync(&["snapshot"], Some(cid), None, None);
                    }
                }
            }
            "item/commandExecution/outputDelta" => {
                let item_id = p["itemId"].as_str().unwrap_or("");
                let delta = p["delta"].as_str().unwrap_or("");
                if !item_id.is_empty() && !delta.is_empty() {
                    connection.execute("UPDATE codex_messages SET text=text || ?3 WHERE conversation_id=(SELECT conversation_id FROM codex_sessions WHERE thread_id=?1) AND item_id=?2", params![thread, item_id, delta]).map_err(|e| e.to_string())?;
                    if let Some(cid) = conversation_id.as_deref() {
                        crate::events::emit_delta(cid, item_id, "tool", Some("codex"), "append", delta);
                    }
                }
            }
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let kind = item["type"].as_str().unwrap_or("");
                let mut item_id = item["id"].as_str().unwrap_or("").to_owned();
                let mut role = if matches!(kind, "agentMessage" | "imageGeneration") {
                    "assistant"
                } else {
                    "tool"
                };
                let text = match kind {
                    "agentMessage" if method == "item/completed" => {
                        item["text"].as_str().unwrap_or("").to_owned()
                    }
                    "reasoning" => {
                        let summary = extract_reasoning_text(item);
                        if summary.is_empty() {
                            return Ok(());
                        }
                        item_id = format!("reasoning:{item_id}");
                        role = "thinking";
                        summary
                    }
                    "commandExecution" => {
                        let mut header = format!(
                            "命令：{}\n状态：{}",
                            item["command"].as_str().unwrap_or(""),
                            item["status"].as_str().unwrap_or(""),
                        );
                        if let Some(code) = item["exitCode"].as_i64() {
                            header.push_str(&format!("\n退出码：{code}"));
                        }
                        if let Some(ms) = item["durationMs"].as_u64() {
                            header.push_str(&format!("\n耗时：{ms}ms"));
                        }
                        format!("{header}\n{}", item["aggregatedOutput"].as_str().unwrap_or(""))
                    }
                    "fileChange" => format!("文件变更：{}", item["changes"]),
                    "mcpToolCall" => format!(
                        "MCP 工具：{}/{}\n状态：{}\n{}",
                        item["server"].as_str().unwrap_or(""),
                        item["tool"].as_str().unwrap_or(""),
                        item["status"].as_str().unwrap_or(""),
                        serde_json::to_string(&json!({
                            "arguments": item["arguments"],
                            "result": item["result"],
                            "error": item["error"],
                        }))
                        .unwrap_or_default()
                    ),
                    "webSearch" => format!(
                        "联网搜索：{}\n状态：{}",
                        item["query"].as_str().unwrap_or(""),
                        item["status"]
                            .as_str()
                            .unwrap_or(if method == "item/completed" { "completed" } else { "inProgress" })
                    ),
                    "imageGeneration" if method == "item/completed" => {
                        generated_image_message(&Store { path: self.database.clone() }, item)
                    }
                    _ => return Ok(()),
                };
                connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,?3,?4 FROM codex_sessions WHERE thread_id=?1 ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text", params![thread, &item_id, role, &text]).map_err(|e| e.to_string())?;
                if let Some(cid) = conversation_id.as_deref() {
                    crate::events::emit_delta(cid, item_id, role, Some("codex"), "replace", text);
                }
            }
            "thread/tokenUsage/updated" => {
                let last = &p["tokenUsage"]["last"];
                let inp = last["inputTokens"].as_i64().unwrap_or(0);
                let out = last["outputTokens"].as_i64().unwrap_or(0);
                let cached = last["cachedInputTokens"].as_i64().unwrap_or(0);
                let total = last["totalTokens"].as_i64().unwrap_or(inp + out);
                let context_window = p["tokenUsage"]["modelContextWindow"]
                    .as_i64()
                    .filter(|&w| w > 0);
                if inp > 0 || out > 0 || total > 0 || context_window.is_some() {
                    let mut map = self.token_usage.lock().unwrap();
                    let prev_window = map.get(thread).and_then(|u| u.context_window);
                    map.insert(
                        thread.to_owned(),
                        LiveTokenUsage {
                            input_tokens: inp,
                            output_tokens: out,
                            cached_tokens: cached,
                            total_tokens: total.max(inp + out),
                            context_window: context_window.or(prev_window),
                        },
                    );
                    drop(map);
                    if let Some(cid) = conversation_id.as_deref() {
                        crate::events::emit_sync(&["snapshot"], Some(cid), None, None);
                    }
                }
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
                let live = self
                    .token_usage
                    .lock()
                    .unwrap()
                    .get(thread)
                    .copied()
                    .unwrap_or_default();
                if let Ok((conv_id, model_name)) = connection.query_row(
                    "SELECT conversation_id, COALESCE(model, '') FROM codex_sessions WHERE thread_id=?1",
                    [thread],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                ) {
                    crate::usage::record(
                        &self.database,
                        "codex",
                        "codex",
                        &model_name,
                        &conv_id,
                        live.input_tokens,
                        live.output_tokens,
                        live.cached_tokens,
                        status,
                    );
                    crate::events::emit_sync(&["snapshot"], Some(&conv_id), None, None);
                }
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
                if let Some(cid) = conversation_id.as_deref() {
                    crate::events::emit_conversation_sync(cid);
                }
            }
            _ => (),
        }
        Ok(())
    }

    pub(super) fn update_status(&self, thread: &str, status: &str, error: Option<&str>) -> Result<(), String> {
        let mut connection = Store {
            path: self.database.clone(),
        }
        .connect()?;
        let conversation_id: Option<String> = connection
            .query_row(
                "SELECT conversation_id FROM codex_sessions WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .ok();
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
        if let Some(cid) = conversation_id.as_deref() {
            if matches!(status, "completed" | "cancelled" | "failed" | "interrupted") {
                crate::git::finish_turn_snapshot(
                    &Store {
                        path: self.database.clone(),
                    },
                    cid,
                    "codex",
                );
            }
            crate::events::emit_conversation_sync(cid);
        }
        Ok(())
    }

    pub(super) fn disconnected(&self, error: &str) {
        if !self.healthy.swap(false, Ordering::SeqCst) {
            return;
        }
        for (_, send) in self.pending.lock().unwrap().drain() {
            let _ = send.send(Err(error.to_owned()));
        }
        let threads: Vec<String> = self.loaded.lock().unwrap().iter().cloned().collect();
        let store = Store {
            path: self.database.clone(),
        };
        if let Ok(mut connection) = store.connect() {
            for thread in threads {
                let cid: Option<String> = connection
                    .query_row(
                        "SELECT conversation_id FROM codex_sessions WHERE thread_id=?1",
                        [&thread],
                        |row| row.get(0),
                    )
                    .ok();
                if let Ok(tx) = connection.transaction() {
                    let changed = tx
                        .execute(
                            "INSERT INTO codex_messages(conversation_id,item_id,role,text) SELECT conversation_id,?2,'task_end','interrupted' FROM codex_sessions WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')",
                            params![&thread, uuid::Uuid::new_v4().to_string()],
                        )
                        .unwrap_or(0);
                    let _ = tx.execute(
                        "UPDATE codex_sessions SET status='interrupted',error=?2 WHERE thread_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')",
                        params![&thread, error],
                    );
                    let _ = tx.commit();
                    if let Some(cid) = cid.as_deref() {
                        if changed > 0 {
                            crate::git::finish_turn_snapshot(&store, cid, "codex");
                        }
                        crate::events::emit_conversation_sync(cid);
                    }
                }
            }
        }
        self.approvals.lock().unwrap().clear();
    }

    pub(super) fn stop(&self) -> Result<(), String> {
        #[cfg(any(windows, unix))]
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

pub(super) fn model_catalog(client: &Client) -> Result<Vec<Value>, String> {
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

pub(super) fn ensure_client(slot: &Mutex<Option<Arc<Client>>>, store: &Store) -> Result<Arc<Client>, String> {
    let mut slot = slot.lock().map_err(|e| e.to_string())?;
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

pub(super) fn mark_startup_failed(store: &Store, conversation_id: &str, error: &str) {
    if let Ok(mut connection) = store.connect() {
        if let Ok(tx) = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
            let changed = tx
                .execute(
                    "UPDATE codex_sessions SET status='interrupted',error=?2 WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')",
                    params![conversation_id, error],
                )
                .unwrap_or(0);
            if changed > 0 {
                let _ = tx.execute(
                    "INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'task_end','interrupted')",
                    params![conversation_id, uuid::Uuid::new_v4().to_string()],
                );
            }
            let _ = tx.commit();
            if changed > 0 {
                crate::events::emit_conversation_sync(conversation_id);
            }
        }
    }
}
