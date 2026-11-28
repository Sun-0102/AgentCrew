    use super::*;

    fn fixture() -> (tempfile::TempDir, Store, String, Client) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let project = store.add_project(dir.path().to_str().unwrap()).unwrap();
        store.connect().unwrap().execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider,status) VALUES(?1,'thread','model','test','test','starting')", [&project.id]).unwrap();
        #[cfg(unix)]
        let mut child = command(Path::new("/bin/cat"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(not(unix))]
        let mut child = command(&std::env::current_exe().unwrap())
            .arg("--list")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(any(windows, unix))]
        let job = ProcessJob::attach(&child).unwrap();
        let client = Client {
            input: Mutex::new(child.stdin.take().unwrap()),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            loaded: Mutex::new(HashSet::from(["thread".to_owned()])),
            token_usage: Mutex::new(HashMap::new()),
            stderr_tail: Mutex::new(VecDeque::with_capacity(20)),
            next_id: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            database: store.path.clone(),
            version: "test".into(),
            executable: PathBuf::from("test-codex"),
            #[cfg(any(windows, unix))]
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
    fn disconnect_writes_task_end_and_timeout_does_not_kill_shared_client() {
        let (_dir, store, project, client) = fixture();
        client
            .receive(json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"turn"}}}))
            .unwrap();
        // A timed-out secondary RPC must not mark the shared client unhealthy.
        let err = client
            .request_with_timeout("account/rateLimits/read", json!({}), Duration::from_millis(20))
            .unwrap_err();
        assert!(err.contains("Codex 请求超时"));
        assert!(client.healthy.load(Ordering::SeqCst));
        // An unexpected disconnect must record an explicit task_end='interrupted' marker.
        client.disconnected("stream closed");
        assert!(!client.healthy.load(Ordering::SeqCst));
        let sess = session(&store, &project).unwrap().unwrap();
        assert_eq!(sess["status"], "interrupted");
        assert_eq!(sess["error"], "stream closed");
        let end: String = store
            .connect()
            .unwrap()
            .query_row(
                "SELECT text FROM codex_messages WHERE conversation_id=?1 AND role='task_end'",
                [&project],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(end, "interrupted");
        // Sensitive stderr lines are redacted while normal diagnostics are retained.
        assert!(sanitize_stderr_line("Authorization: Bearer sk-12345").is_none());
        assert_eq!(
            sanitize_stderr_line("  upstream websocket reset  ").as_deref(),
            Some("upstream websocket reset")
        );
    }

    #[test]
    fn mismatched_execution_context_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let mut response = json!({"cwd":root,"sandbox":{"type":"dangerFullAccess"},"approvalPolicy":"never"});
        validate_context(&response, root).unwrap();
        response["sandbox"]["type"] = json!("workspaceWrite");
        assert!(validate_context(&response, root).is_err());
        response["sandbox"]["type"] = json!("dangerFullAccess");
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
            .personal_conversation_id(&dir.path().join("Documents/Codex/AgentCrew"))
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
                .personal_conversation_id(&dir.path().join("Documents/Codex/AgentCrew"))
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
        #[cfg_attr(not(windows), allow(unused_variables))]
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
