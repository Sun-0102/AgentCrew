use super::*;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Providers {
    sessions: RefCell<BTreeMap<String, Value>>,
    starts: RefCell<Vec<String>>,
    stops: RefCell<Vec<String>>,
}

impl Providers {
    fn snapshot(&self, task: &Task) -> Result<Value, String> {
        Ok(self.sessions.borrow().get(&task.current_agent).cloned().unwrap_or(Value::Null))
    }
    fn send(&self, store: &Store, task: &Task, prompt: &str) -> Result<(), String> {
        assert!(!self.snapshot(task)?["status"].as_str().is_some_and(codex::active));
        self.starts.borrow_mut().push(task.current_agent.clone());
        self.sessions.borrow_mut().insert(task.current_agent.clone(), json!({"thread_id":format!("thread-{}",task.current_agent),"turn_id":uuid::Uuid::new_v4().to_string(),"status":"running"}));
        store.connect()?.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'assignment',?3,?4)",params![task.conversation_id,uuid::Uuid::new_v4().to_string(),prompt,task.current_agent]).unwrap();
        Ok(())
    }
    fn cancel(&self, task: &Task) -> Result<(), String> {
        self.stops.borrow_mut().push(task.current_agent.clone());
        if let Some(session) = self.sessions.borrow_mut().get_mut(&task.current_agent) { session["status"] = json!("cancelled"); }
        Ok(())
    }
    fn tick(&self, store: &Store) -> bool {
        tick_with(store,"task",&|t|self.snapshot(t),&|t,p|self.send(store,t,p),&|t|self.cancel(t)).unwrap()
    }
    fn finish(&self, store: &Store, group: &str, agent: &str, text: &str) {
        store.connect().unwrap().execute("INSERT INTO codex_messages(conversation_id,item_id,role,text,agent) VALUES(?1,?2,'assistant',?3,?4)",params![group,uuid::Uuid::new_v4().to_string(),text,agent]).unwrap();
        self.sessions.borrow_mut().get_mut(agent).unwrap()["status"] = json!("completed");
    }
}

fn fixture() -> (tempfile::TempDir, Store, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let group = store.create_group_in("parallel",vec!["codex".into(),"deepseek".into()],None,&dir.path().join("groups")).unwrap();
    store.connect().unwrap().execute("INSERT INTO group_tasks(id,conversation_id,objective,status,current_agent,codex_model,deepseek_model,pending_prompt) VALUES('task',?1,'parallel','running','codex','model','model','')",[&group.id]).unwrap();
    for agent in ["codex","deepseek"] {
        store.connect().unwrap().execute("INSERT INTO group_task_members(task_id,agent) VALUES('task',?1)",[agent]).unwrap();
        enqueue(&store,agent,"first");
    }
    (dir,store,group.id)
}

fn enqueue(store: &Store, agent: &str, message: &str) {
    store.connect().unwrap().execute("INSERT INTO group_task_inputs(task_id,message_id,target_agent,model,text) VALUES('task',?1,?2,'model','user request')",params![message,agent]).unwrap();
}

#[test]
fn recipient_selection_broadcasts_by_default_and_respects_explicit_mentions() {
    let cases: Value = serde_json::from_str(include_str!("../../tests/fixtures/group-routing.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let members: Vec<String> = serde_json::from_value(case["members"].clone()).unwrap();
        assert_eq!(json!(message_recipients(case["text"].as_str().unwrap(),&members)),case["expected"],"{}",case["text"]);
    }
}

#[test]
fn unaddressed_request_starts_both_and_single_mention_cannot_wake_the_other() {
    for text in ["你们协调完成这个任务", "请 @DeepSeek 独立处理"] {
        let (_dir,store,group) = fixture();
        store.connect().unwrap().execute("DELETE FROM group_task_inputs",[]).unwrap();
        let recipients = message_recipients(text,&store.group(&group).unwrap().members);
        for agent in &recipients { enqueue(&store,agent,"request"); }
        store.connect().unwrap().execute("UPDATE group_task_inputs SET text=?1",[text]).unwrap();
        let providers = Providers::default();
        assert!(providers.tick(&store));
        assert_eq!(*providers.starts.borrow(),recipients);
        let current = member_task(&store,"task","deepseek").unwrap();
        if recipients.len() == 1 {
            assert!(current.pending_prompt.contains("不转交或唤醒其他成员"));
            providers.finish(&store,&group,"deepseek","@Codex 请继续");
            assert!(!providers.tick(&store));
            assert_eq!(*providers.starts.borrow(),["deepseek"]);
            assert_eq!(latest(&store,&group).unwrap().unwrap().queued_count,0);
        } else {
            assert!(current.pending_prompt.contains("已同时投递给群内所有成员"));
            assert!(latest(&store,&group).unwrap().unwrap().members.iter().all(|member|member.status == "running"));
        }
    }
}

#[test]
fn both_members_launch_before_either_finishes_and_archive_independently() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    assert!(providers.tick(&store));
    assert_eq!(*providers.starts.borrow(),["codex","deepseek"]);
    let current = latest(&store,&group).unwrap().unwrap();
    assert!(current.members.iter().all(|m|m.status == "running"));
    assert_eq!(current.turn_count,2);
    assert_eq!(current.queued_count,0);
    providers.finish(&store,&group,"codex","codex done");
    assert!(providers.tick(&store));
    assert_eq!(member_task(&store,"task","codex").unwrap().status,"completed");
    assert_eq!(member_task(&store,"task","deepseek").unwrap().status,"running");
    let data = task_results::result(&store,&group,"task").unwrap();
    assert_eq!(data["steps"][0]["status"],"completed");
    assert_eq!(data["steps"][1]["status"],"running");
    assert_eq!(data["steps"][0]["messages"][0]["text"],"codex done");
    providers.finish(&store,&group,"deepseek","deepseek done");
    assert!(!providers.tick(&store));
    assert_eq!(task(&store,"task").unwrap().status,"idle");
    let data = task_results::result(&store,&group,"task").unwrap();
    assert_eq!(data["steps"][1]["messages"][0]["text"],"deepseek done");
}

#[test]
fn a_member_can_take_its_next_message_while_the_other_is_still_running() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    providers.tick(&store);
    enqueue(&store,"codex","second");
    enqueue(&store,"deepseek","second");
    assert!(providers.tick(&store));
    assert_eq!(providers.starts.borrow().len(),2);
    providers.finish(&store,&group,"deepseek","first done");
    assert!(providers.tick(&store));
    assert_eq!(*providers.starts.borrow(),["codex","deepseek","deepseek"]);
    assert_eq!(member_task(&store,"task","codex").unwrap().queued_count,0);
    assert_eq!(latest(&store,&group).unwrap().unwrap().queued_count,1);
}

#[test]
fn failure_of_one_member_does_not_stop_the_other() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    providers.tick(&store);
    providers.sessions.borrow_mut().get_mut("codex").unwrap()["status"] = json!("interrupted");
    assert!(providers.tick(&store));
    assert_eq!(member_task(&store,"task","codex").unwrap().status,"blocked");
    assert_eq!(member_task(&store,"task","deepseek").unwrap().status,"running");
    providers.finish(&store,&group,"deepseek","done");
    assert!(!providers.tick(&store));
    assert_eq!(task(&store,"task").unwrap().status,"blocked");
}

#[test]
fn handoff_waits_only_for_its_recipient_and_does_not_duplicate() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    providers.tick(&store);
    providers.finish(&store,&group,"codex","@DeepSeek please review");
    assert!(providers.tick(&store));
    assert!(providers.tick(&store));
    assert_eq!(providers.starts.borrow().len(),2);
    assert_eq!(latest(&store,&group).unwrap().unwrap().queued_count,1);
    providers.finish(&store,&group,"deepseek","first done");
    assert!(providers.tick(&store));
    assert_eq!(*providers.starts.borrow(),["codex","deepseek","deepseek"]);
    let data = task_results::result(&store,&group,"task").unwrap();
    assert_eq!(data["inputs"].as_array().unwrap().len(),2);
    assert_eq!(data["steps"][2]["source_agent"],"codex");
    let current = member_task(&store,"task","deepseek").unwrap();
    let prompt = build_prompt(&store,&current,&task_results::capture(&store.conversation_root(&group).unwrap())).unwrap();
    let owner_section = prompt.split("本次用户亲自发送的新消息").nth(1).unwrap().split("本轮投递内容").next().unwrap();
    assert!(!owner_section.contains("please review"));
    assert!(!prompt.contains("DeepSeek 当前不能用终端"));
    assert!(prompt.contains("问候、闲聊、询问是否在线时只简短回应"));
    assert!(prompt.contains("不预设哪位只能写文件"));
}


#[test]
fn pause_allows_completion_but_delays_new_starts_and_stop_cancels_both() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    providers.tick(&store);
    enqueue(&store,"codex","second");
    change(&store,&task(&store,"task").unwrap(),"paused","paused").unwrap();
    providers.finish(&store,&group,"codex","done");
    assert!(providers.tick(&store));
    assert_eq!(providers.starts.borrow().len(),2);
    change(&store,&task(&store,"task").unwrap(),"running","resume").unwrap();
    assert!(providers.tick(&store));
    assert_eq!(providers.starts.borrow().len(),3);
    enqueue(&store,"deepseek","second");
    change(&store,&task(&store,"task").unwrap(),"cancelling","stop").unwrap();
    stop_members_with(&store,"task",&|t|providers.cancel(t)).unwrap();
    change(&store,&task(&store,"task").unwrap(),"cancelled","stopped").unwrap();
    assert_eq!(*providers.stops.borrow(),["codex","deepseek"]);
    assert!(!providers.tick(&store));
    assert_eq!(latest(&store,&group).unwrap().unwrap().queued_count,0);
    assert!(latest(&store,&group).unwrap().unwrap().members.iter().all(|m|m.status == "cancelled"));
}

#[test]
fn a_failed_cancel_still_attempts_the_other_member_and_recovery_clears_both() {
    let (_dir,store,group) = fixture();
    let providers = Providers::default();
    providers.tick(&store);
    let stops = RefCell::new(Vec::new());
    assert!(stop_members_with(&store,"task",&|t| {
        stops.borrow_mut().push(t.current_agent.clone());
        if t.current_agent == "codex" {Err("stop failed".into())} else {Ok(())}
    }).is_err());
    assert_eq!(*stops.borrow(),["codex","deepseek"]);
    recover(&store).unwrap();
    assert_eq!(task(&store,"task").unwrap().status,"interrupted");
    assert!(latest(&store,&group).unwrap().unwrap().members.iter().all(|m|m.status != "running"));
}

#[test]
fn version_eleven_upgrade_backs_up_history_and_creates_member_records() {
    let (dir,store,group) = fixture();
    store.connect().unwrap().execute_batch("DROP TABLE group_task_members; ALTER TABLE group_task_inputs DROP COLUMN source_agent; PRAGMA user_version=11;").unwrap();
    let upgraded = Store::open(store.path.clone()).unwrap();
    let current = latest(&upgraded,&group).unwrap().unwrap();
    assert_eq!(current.members.len(),2);
    assert_eq!(current.queued_count,2);
    let backup = std::fs::read_dir(dir.path()).unwrap().map(|p|p.unwrap().path()).find(|p|p.file_name().unwrap().to_string_lossy().contains("v11-")).unwrap();
    let backup = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(backup.query_row("PRAGMA user_version",[],|r|r.get::<_,u32>(0)).unwrap(),11);
    recover(&upgraded).unwrap();
    assert_eq!(task(&upgraded,"task").unwrap().status,"interrupted");
    assert_eq!(latest(&upgraded,&group).unwrap().unwrap().queued_count,0);
}

#[test]
#[ignore = "Uses two real model turns in a temporary group to verify overlapping provider execution"]
fn installed_providers_execute_in_parallel() {
    let (_dir,store,group) = fixture();
    let codex_runtime = codex::Runtime::default();
    let deepseek_runtime = deepseek::Runtime::default();
    let result = (|| -> Result<(),String> {
        let codex_probe = codex_runtime.probe(&store)?;
        let deepseek_probe = deepseek_runtime.probe(&store)?;
        let models = codex_probe["models"].as_array().ok_or("Codex models missing")?;
        let codex_model = models.iter().find(|m|m["isDefault"] == true).unwrap_or(&models[0])["model"].as_str().ok_or("Codex model missing")?;
        let deepseek_model = deepseek_probe["models"][0]["model"].as_str().ok_or("DeepSeek model missing")?;
        let request = "这是并行调度测试。不要使用工具，不要提及或呼叫另一位成员，不要启动子代理。请直接写一篇约800字的中文短文，解释如何把大任务拆成互不依赖的小任务。最后单独写 PARALLEL_OK。";
        store.connect()?.execute("UPDATE group_tasks SET objective=?1,codex_model=?2,deepseek_model=?3 WHERE id='task'",params![request,codex_model,deepseek_model]).map_err(|e|e.to_string())?;
        store.connect()?.execute("UPDATE group_task_inputs SET text=?1,model=CASE WHEN target_agent='codex' THEN ?2 ELSE ?3 END WHERE task_id='task'",params![request,codex_model,deepseek_model]).map_err(|e|e.to_string())?;
        let snapshot = |t: &Task| if t.current_agent == "codex" {Ok(codex_runtime.snapshot(&store,&group)?["session"].clone())} else {deepseek::session(&store,&group)};
        let send = |t: &Task,p: &str| {
            if t.current_agent == "codex" {codex_runtime.send_recorded(&store,&group,&t.codex_model,p,"assignment")?;} else {deepseek_runtime.send_recorded(&store,&group,&t.deepseek_model,p,"assignment")?;}
            Ok(())
        };
        let cancel = |t: &Task| {
            if snapshot(t)?["status"].as_str().is_some_and(codex::active) {
                if t.current_agent == "codex" {codex_runtime.cancel(&store,&group)?;} else {deepseek_runtime.cancel_in(&store,&group)?;}
            }
            Ok(())
        };
        tick_with(&store,"task",&snapshot,&send,&cancel)?;
        for agent in ["codex","deepseek"] {
            let session = snapshot(&member_task(&store,"task",agent)?)?;
            if !session["status"].as_str().is_some_and(codex::active) {return Err(format!("{agent} was not active at overlap check: {}",session["status"]));}
        }
        println!("Both real providers are active before either finishes.");
        let deadline = std::time::Instant::now()+Duration::from_secs(180);
        while tick_with(&store,"task",&snapshot,&send,&cancel)? {
            if std::time::Instant::now() > deadline {return Err("Parallel integration timed out".into());}
            std::thread::sleep(Duration::from_millis(200));
        }
        if task(&store,"task")?.status != "idle" {return Err("Parallel task did not finish normally".into());}
        let data = task_results::result(&store,&group,"task")?;
        for step in data["steps"].as_array().ok_or("steps missing")? {
            if step["status"] != "completed" || !step["messages"].to_string().contains("PARALLEL_OK") {return Err("A provider did not return the test marker".into());}
        }
        println!("Both providers completed and their replies were archived separately.");
        Ok(())
    })();
    codex_runtime.shutdown();
    deepseek_runtime.shutdown();
    result.unwrap();
}
