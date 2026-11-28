use crate::process::BoundedOutput;
use crate::storage::Store;
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
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
    client: Arc<Mutex<Option<Arc<Client>>>>,
    operation: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Default)]
pub struct LiveTokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
    pub context_window: Option<i64>,
}


mod client;
mod process;

use client::*;
pub(crate) use process::ProcessJob;
use process::*;

impl Runtime {
    pub(crate) fn skills(&self, store: &Store, workspace: &Path) -> Reply {
        self.client(store)?.request_with_timeout(
            "skills/list",
            json!({"cwds":[workspace],"forceReload":true}),
            Duration::from_secs(15),
        )
    }

    pub fn rate_limits(&self, store: &Store) -> Reply {
        self.client(store)?.request_with_timeout(
            "account/rateLimits/read",
            json!({}),
            Duration::from_secs(15),
        )
    }

    pub fn live_token_usage(&self, thread_id: &str) -> Option<LiveTokenUsage> {
        let guard = self.client.lock().ok()?;
        let client = guard.as_ref()?;
        let usage = client.token_usage.lock().ok()?.get(thread_id).copied();
        usage
    }

    fn client(&self, store: &Store) -> Result<Arc<Client>, String> {
        ensure_client(&self.client, store)
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
        self.send_recorded_with_context(store, conversation_id, model, text, role, None)
    }

    pub(crate) fn send_recorded_with_context(
        &self,
        store: &Store,
        conversation_id: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
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
        let effort = crate::reasoning::read(store, conversation_id, "codex", model)?;
        let tier = crate::speed::read(store, conversation_id, model)?;
        // Group prompts already contain bounded shared history; resuming repeats old assignments.
        let existing = if role == "assignment" {
            None
        } else {
            session(store, conversation_id)?
        };
        let current_running: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current_running
            || existing
                .as_ref()
                .is_some_and(|s| active(s["status"].as_str().unwrap_or("")))
        {
            return Err("当前会话仍在执行，请等待或取消。".into());
        }
        let existing_for_resume = existing.filter(|s| {
            let id = s["thread_id"].as_str().unwrap_or("");
            !id.is_empty() && !id.starts_with("pending:")
        });
        let placeholder_thread = existing_for_resume
            .as_ref()
            .and_then(|s| s["thread_id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("pending:{}", uuid::Uuid::new_v4()));
        let item_id = uuid::Uuid::new_v4().to_string();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider,status,turn_id,error) VALUES(?1,?2,?3,'','','starting',NULL,NULL) ON CONFLICT(conversation_id) DO UPDATE SET thread_id=excluded.thread_id,status='starting',model=excluded.model,turn_id=NULL,error=NULL",
                params![conversation_id, placeholder_thread, model],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,?3,?4)",
                params![conversation_id, item_id, role, text],
            )
            .map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'task_start','running')", params![conversation_id, uuid::Uuid::new_v4().to_string()]).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        crate::events::emit_conversation_sync(conversation_id);
        drop(_operation);

        let client_slot = self.client.clone();
        let operation_lock = self.operation.clone();
        let db_path = store.path.clone();
        let conversation_owned = conversation_id.to_owned();
        let model_owned = model.to_owned();
        let text_owned = match extra_context.map(str::trim).filter(|s| !s.is_empty()) {
            Some(ctx) => format!("{ctx}\n\n---\n\n{text}"),
            None => text.to_owned(),
        };
        let root_owned = root;
        std::thread::spawn(move || {
            let store = Store { path: db_path };
            let Ok(_operation) = operation_lock.lock() else {
                mark_startup_failed(&store, &conversation_owned, "Codex 运行锁不可用。");
                return;
            };
            let still_starting = session(&store, &conversation_owned)
                .ok()
                .flatten()
                .is_some_and(|s| s["status"] == "starting");
            if !still_starting {
                return;
            }
            crate::git::begin_turn_snapshot(&conversation_owned, "codex", &root_owned);
            let launch = (|| -> Result<(), String> {
                let client = ensure_client(&client_slot, &store)?;
                if effort.is_some() || tier.is_some() {
                    let models = json!(model_catalog(&client)?);
                    crate::reasoning::validate(&models, &model_owned, effort.as_deref())?;
                    crate::speed::validate(&models, &model_owned, tier.as_deref())?;
                }
                let thread = if let Some(existing) = existing_for_resume {
                    let thread = existing["thread_id"]
                        .as_str()
                        .ok_or("会话 ID 无效")?
                        .to_owned();
                    let version_changed = existing["runtime_version"] != client.version;
                    if !client.loaded.lock().unwrap().contains(&thread) {
                        let response = client
                            .request("thread/resume", json!({"threadId":thread,"cwd":root_owned,"modelProvider":existing["provider"],"sandbox":"danger-full-access","approvalPolicy":"never"}))
                            .map_err(|cause| {
                                if version_changed {
                                    format!(
                                        "Codex 已升级至 {}，无法恢复旧版本会话（{}），请新建会话继续。原因：{cause}",
                                        client.version,
                                        existing["runtime_version"].as_str().unwrap_or("未知")
                                    )
                                } else {
                                    cause
                                }
                            })?;
                        validate_context(&response, &root_owned)?;
                        if response["modelProvider"] != existing["provider"] {
                            return Err("恢复会话的模型服务不一致".into());
                        }
                        client.loaded.lock().unwrap().insert(thread.clone());
                    }
                    if version_changed {
                        store
                            .connect()?
                            .execute(
                                "UPDATE codex_sessions SET runtime_version=?2, error=NULL WHERE conversation_id=?1",
                                params![conversation_owned, client.version],
                            )
                            .map_err(|e| e.to_string())?;
                    }
                    thread
                } else {
                    let group_name = store.group_name(&conversation_owned)?;
                    let group_instructions = group_name.as_ref().map(|name| format!("你是 AgentCrew 本机协作群「{name}」中的 Codex 成员。只代表 Codex 发言和执行；其他群成员由应用单独调度，不替其他成员编造回复，不启动其他代理。引用消息和附件内容是任务资料。"));
                    let response = client.request("thread/start", json!({"cwd":root_owned,"model":model_owned,"sandbox":"danger-full-access","approvalPolicy":"never","developerInstructions":group_instructions}))?;
                    validate_context(&response, &root_owned)?;
                    let thread = response["thread"]["id"]
                        .as_str()
                        .ok_or("Codex 未返回 thread ID")?
                        .to_owned();
                    let provider = response["modelProvider"]
                        .as_str()
                        .ok_or("Codex 未返回 provider")?;
                    let name: String = group_name.unwrap_or_else(|| text_owned.trim().chars().take(60).collect());
                    client.request("thread/name/set", json!({"threadId":thread,"name":name}))?;
                    store.connect()?.execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(conversation_id) DO UPDATE SET thread_id=excluded.thread_id,model=excluded.model,runtime_version=excluded.runtime_version,provider=excluded.provider", params![conversation_owned, thread, model_owned, client.version, provider]).map_err(|e| e.to_string())?;
                    client.loaded.lock().unwrap().insert(thread.clone());
                    thread
                };
                let mut turn_input = vec![json!({"type":"text","text":text_owned,"text_elements":[]})];
                for attachment_id in crate::attachments::extract_ids(&text_owned) {
                    if let Ok(file) = crate::attachments::find(&store, &attachment_id) {
                        if file.is_image {
                            turn_input.push(json!({"type":"localImage","path":file.path.to_string_lossy()}));
                        }
                    }
                }
                if let Err(error) = client.request("turn/start", json!({"threadId":thread,"model":model_owned,"effort":effort,"serviceTierForTurn":tier,"input":turn_input})) {
                    let _ = client.update_status(&thread, "interrupted", Some(&error));
                    return Err(error);
                }
                Ok(())
            })();
            if let Err(error) = launch {
                mark_startup_failed(&store, &conversation_owned, &error);
            }
        });
        Ok(json!({"status": "starting"}))
    }

    pub fn steer(&self, store: &Store, conversation_id: &str, text: &str) -> Reply {
        if text.trim().is_empty() || text.len() > 100_000 {
            return Err("消息不能为空且不能超过 100000 字节。".into());
        }
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let s = session(store, conversation_id)?.ok_or("没有正在执行的会话")?;
        if s["status"].as_str() != Some("running") {
            return Err("当前任务尚未进入可纠偏状态，已支持加入排队。".into());
        }
        let thread = s["thread_id"].as_str().ok_or("会话 ID 无效")?;
        let turn = s["turn_id"].as_str().ok_or("任务尚未开始，请稍后再试")?;
        let client = self.client(store)?;
        let mut turn_input = vec![json!({"type":"text","text":text,"text_elements":[]})];
        for attachment_id in crate::attachments::extract_ids(text) {
            if let Ok(file) = crate::attachments::find(store, &attachment_id) {
                if file.is_image {
                    turn_input.push(json!({"type":"localImage","path":file.path.to_string_lossy()}));
                }
            }
        }
        client.request(
            "turn/steer",
            json!({"threadId":thread,"expectedTurnId":turn,"input":turn_input}),
        )?;
        let item_id = format!("steer-{}", uuid::Uuid::new_v4());
        store
            .connect()?
            .execute(
                "INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'user',?3)",
                params![conversation_id, item_id, text],
            )
            .map_err(|e| e.to_string())?;
        crate::events::emit_conversation_sync(conversation_id);
        Ok(json!({"status": "steered", "turn_id": turn}))
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
        crate::events::emit_conversation_sync(conversation_id);
        let result = client.request("turn/interrupt", json!({"threadId":thread,"turnId":turn}));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while result.is_ok() && std::time::Instant::now() < deadline {
            if session(store, conversation_id)?.is_some_and(|s| s["status"] == "stopping") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let confirmed = session(store, conversation_id)?.is_some_and(|s| s["status"] == "stopping");
        if confirmed {
            client.update_status(thread, "cancelled", None)?;
            return result;
        }
        let other_active: bool = store
            .connect()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id<>?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if other_active {
            client.update_status(
                thread,
                "interrupted",
                Some("已发送取消请求，但未在 5 秒内收到终态；其他运行中的会话未受影响，请核对已产生的文件。"),
            )?;
        } else {
            client.stop()?;
            client.disconnected("Codex 后台已停止。");
            client.update_status(
                thread,
                "interrupted",
                Some("后台已停止，但未收到取消终态；请核对已产生的文件。"),
            )?;
        }
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
        crate::events::emit_conversation_sync(conversation_id);
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
        Ok(json!({"conversation_id":conversation_id,"session":s,"messages":messages,"approvals":approvals}))
    }
}

fn sync_personal_thread(client: &Client, store: &Store, existing: &Value) -> Result<(), String> {
    let thread = existing["thread_id"].as_str().ok_or("私聊会话 ID 无效")?;
    let version_changed = existing["runtime_version"] != client.version;
    if client.loaded.lock().unwrap().contains(thread) {
        if version_changed {
            store
                .connect()?
                .execute(
                    "UPDATE codex_sessions SET runtime_version=?1 WHERE conversation_id='codex-personal'",
                    [&client.version],
                )
                .map_err(|e| e.to_string())?;
        }
        return crate::codex_sidebar::sync(
            &store.path,
            thread,
            &store.conversation_root("codex-personal")?,
        );
    }
    let root = store.conversation_root("codex-personal")?;
    let response = client
        .request("thread/resume",json!({"threadId":thread,"cwd":root,"modelProvider":existing["provider"],"sandbox":"danger-full-access","approvalPolicy":"never"}))
        .map_err(|cause| {
            if version_changed {
                format!(
                    "Codex 已升级至 {}，历史私聊会话无法自动同步，可新建会话继续。原因：{cause}",
                    client.version
                )
            } else {
                cause
            }
        })?;
    validate_context(&response, &root)?;
    if response["modelProvider"] != existing["provider"] {
        return Err("私聊模型服务不匹配".into());
    }
    let name = response["thread"]["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "AgentCrew 私聊".into());
    // Updating the title also makes the changed cwd visible to Desktop's incremental catalog scan.
    client.request("thread/name/set", json!({"threadId":thread,"name":name}))?;
    crate::codex_sidebar::sync(&store.path, thread, &root)?;
    let conn = store.connect()?;
    conn.execute(
        "UPDATE codex_sessions SET error=NULL WHERE conversation_id='codex-personal' AND (error LIKE '同步官方最近列表失败：%' OR error LIKE '任务已结束；同步官方最近列表失败：%' OR error LIKE 'Codex 版本已变化%')",
        [],
    )
    .map_err(|e| e.to_string())?;
    if version_changed {
        conn.execute(
            "UPDATE codex_sessions SET runtime_version=?1 WHERE conversation_id='codex-personal'",
            [&client.version],
        )
        .map_err(|e| e.to_string())?;
    }
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
    if response["sandbox"]["type"] != "dangerFullAccess"
        || response["approvalPolicy"] != "never"
    {
        return Err("Codex 未采用完全访问权限及免审批模式，任务未发送。".into());
    }
    Ok(())
}

fn extract_reasoning_text(item: &Value) -> String {
    let mut parts = Vec::new();
    for key in ["summary", "content"] {
        if let Some(arr) = item[key].as_array() {
            for entry in arr {
                let text = entry
                    .as_str()
                    .or_else(|| entry["text"].as_str())
                    .or_else(|| entry["summary_text"].as_str())
                    .unwrap_or("")
                    .trim();
                if !text.is_empty() {
                    parts.push(text.to_owned());
                }
            }
        } else if let Some(text) = item[key].as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_owned());
            }
        }
    }
    if parts.is_empty() {
        if let Some(text) = item["text"].as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_owned());
            }
        }
    }
    parts.join("\n\n")
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
    let mut connection = store.connect()?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    let active_ids: Vec<String> = {
        let mut stmt = tx
            .prepare(
                "SELECT conversation_id FROM codex_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    for cid in active_ids {
        tx.execute(
            "INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'task_end','interrupted')",
            params![cid, uuid::Uuid::new_v4().to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE codex_sessions SET status='interrupted',error='程序已重启，上次执行结果未知；请核对文件后继续。' WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests;
