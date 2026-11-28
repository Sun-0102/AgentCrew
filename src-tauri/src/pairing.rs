use crate::{agents, storage::Store};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

pub struct Service {
    pub instance: String,
    pub wiki_token: String,
    pub address: Mutex<Result<String, String>>,
    failures: Mutex<Vec<i64>>,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            instance: uuid::Uuid::new_v4().to_string(),
            wiki_token: secret(),
            address: Mutex::new(Err("配对服务尚未启动。".into())),
            failures: Mutex::new(Vec::new()),
        }
    }
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message,"retryable":false}})),
    )
        .into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hint {
    executable_path: Option<String>,
    version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    protocol_version: u32,
    server_instance_id: String,
    invitation_id: String,
    pairing_code: String,
    request_id: String,
    provider_kind: String,
    runtime_kind: Option<String>,
    runtime_hint: Option<Hint>,
}

#[derive(Clone)]
struct Server {
    app: tauri::AppHandle,
    service: Arc<Service>,
    host: String,
}
fn headers_ok(state: &Server, headers: &HeaderMap) -> bool {
    !headers.contains_key("origin")
        && !headers.contains_key("forwarded")
        && !headers.contains_key("x-forwarded-for")
        && headers.get("host").and_then(|v| v.to_str().ok()) == Some(state.host.as_str())
}

pub fn start(app: &tauri::AppHandle) -> Result<(), String> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|e| format!("配对端口不可用：{e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let host = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    let service = app.state::<Arc<Service>>().inner().clone();
    let base_url = format!("http://{host}");
    *service.address.lock().map_err(|e| e.to_string())? = Ok(base_url.clone());
    crate::wiki::set_local_http_endpoint(base_url, service.wiki_token.clone());
    let state = Server {
        app: app.clone(),
        service,
        host,
    };
    let router = Router::new()
        .route("/local/v1/health", get(health))
        .route("/local/v1/pairing-guide", get(guide))
        .route("/local/v1/pairings/claim", post(claim))
        .route("/local/v1/pairings/{id}", get(status))
        .route("/local/v2/health", get(health_v2))
        .route("/local/v2/pairing-guide", get(guide_v2))
        .route("/local/v2/pairings/claim", post(claim_v2))
        .route("/local/v2/pairings/{id}", get(status))
        .route("/local-wiki/search", get(local_wiki_search))
        .route("/local-wiki/read", get(local_wiki_read))
        .fallback(|| async { error(StatusCode::NOT_FOUND, "not_found", "接口不存在。") })
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
                    Ok(response) => response,
                    Err(_) => error(StatusCode::REQUEST_TIMEOUT, "request_timeout", "请求超时。"),
                }
            },
        ))
        .with_state(state.clone());
    tauri::async_runtime::spawn(async move {
        let result = async {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            axum::serve(listener, router).await
        }
        .await;
        if let Err(error) = result {
            *state
                .service
                .address
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Err(format!("配对服务已停止：{error}"));
        }
    });
    Ok(())
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query_map(uri: &Uri) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(q) = uri.query() {
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            map.insert(percent_decode(k), percent_decode(v));
        }
    }
    map
}

fn wiki_token_ok(state: &Server, headers: &HeaderMap, query_token: Option<&str>) -> bool {
    let header_token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer ")))
        .map(str::trim);
    let candidate = header_token.or_else(|| query_token.map(str::trim));
    candidate == Some(state.service.wiki_token.as_str())
}

async fn local_wiki_search(
    State(state): State<Server>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if !headers_ok(&state, &headers) {
        return error(StatusCode::FORBIDDEN, "request_rejected", "只允许本机直接请求。");
    }
    let query = parse_query_map(&uri);
    if !wiki_token_ok(&state, &headers, query.get("token").map(String::as_str)) {
        return error(StatusCode::UNAUTHORIZED, "unauthorized", "缺少或无效的知识库只读令牌。");
    }
    let store = state.app.state::<Store>();
    let include_archives = query
        .get("include_archives")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let limit = query
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(10);
    match crate::wiki::http_search_json(
        &store,
        query.get("source_id").map(String::as_str).filter(|s| !s.is_empty()),
        query.get("q").map(String::as_str).unwrap_or(""),
        query.get("domain").map(String::as_str).filter(|s| !s.is_empty()),
        include_archives,
        limit,
    ) {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => error(StatusCode::BAD_REQUEST, "wiki_search_failed", &err),
    }
}

async fn local_wiki_read(
    State(state): State<Server>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if !headers_ok(&state, &headers) {
        return error(StatusCode::FORBIDDEN, "request_rejected", "只允许本机直接请求。");
    }
    let query = parse_query_map(&uri);
    if !wiki_token_ok(&state, &headers, query.get("token").map(String::as_str)) {
        return error(StatusCode::UNAUTHORIZED, "unauthorized", "缺少或无效的知识库只读令牌。");
    }
    let Some(path) = query.get("path").map(String::as_str).filter(|s| !s.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "missing_path", "缺少 path 参数。");
    };
    let store = state.app.state::<Store>();
    match crate::wiki::http_read_json(
        &store,
        query.get("source_id").map(String::as_str).filter(|s| !s.is_empty()),
        path,
        query.get("section").map(String::as_str).filter(|s| !s.is_empty()),
    ) {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => error(StatusCode::NOT_FOUND, "wiki_read_failed", &err),
    }
}

async fn health(State(state): State<Server>, headers: HeaderMap) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    Json(json!({"version":env!("CARGO_PKG_VERSION"),"protocol_version":1,"server_instance_id":state.service.instance})).into_response()
}
async fn guide(State(state): State<Server>, headers: HeaderMap) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    Json(json!({"protocol_version":1,"supported_providers":["codex","deepseek"],"claim_method":"POST","claim_path":"/local/v1/pairings/claim","request_fields":{"protocol_version":1,"server_instance_id":"邀请中的实例 ID","invitation_id":"邀请 ID","pairing_code":"配对码","request_id":"自行生成 UUID，重试时不变","provider_kind":"邀请指定类型","runtime_hint":{"executable_path":"可选的绝对路径，仅作线索","version":"可选"}},"poll":"GET /local/v1/pairings/{claim_id}，Authorization: Bearer <claim_token>，每秒一次，最多 60 秒；重试 claim 会替换旧查询凭证。","security":"不提交密钥、登录文件、环境变量、命令参数。应用独立发现并检查运行组件，不执行请求中的路径。此接口只登记，不执行任务。","completion":"只有查询返回 active 才能告知接入成功。"})).into_response()
}

async fn health_v2(State(state): State<Server>, headers: HeaderMap) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    Json(json!({"version":env!("CARGO_PKG_VERSION"),"protocol_version":2,"server_instance_id":state.service.instance})).into_response()
}
async fn guide_v2(State(state): State<Server>, headers: HeaderMap) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    Json(json!({"protocol_version":2,"supported_runtimes":agents::PROVIDERS.iter().map(|(provider,runtime)|json!({"provider_kind":provider,"runtime_kind":runtime})).collect::<Vec<_>>(),"claim_method":"POST","claim_path":"/local/v2/pairings/claim","request_fields":{"protocol_version":2,"server_instance_id":"邀请中的实例 ID","invitation_id":"邀请 ID","pairing_code":"配对码","request_id":"自行生成 UUID，重试保持不变","provider_kind":"自己识别的厂商","runtime_kind":"自己识别的实际组件，必须匹配 supported_runtimes","runtime_hint":{"executable_path":"可选的绝对路径，仅作核验线索，不作为命令执行","version":"可选"}},"poll":"GET /local/v2/pairings/{claim_id}；Authorization: Bearer <claim_token>；每秒一次，最多 60 秒，重试会替换旧凭证。","security":"使用你自己的工具发送请求，不转交给另一位 Agent。不提交凭证、环境变量、命令参数，不安装软件或修改原厂配置。","completion":"202 只表示检查已受理；查询返回 active 后才报告接入成功，并说明 verified_provider_kind、verified_runtime_kind 和版本。Gemini 后续由 Gemini CLI ACP 执行，Antigravity 尚未适配。"})).into_response()
}

pub fn invitation(
    app: &tauri::AppHandle,
    name: &str,
    previous: Option<&str>,
) -> Result<Value, String> {
    if name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err("名称最多 40 个字，且不能包含换行或控制字符。".into());
    }
    let service = app.state::<Arc<Service>>();
    let address = service.address.lock().map_err(|e| e.to_string())?.clone()?;
    let id = uuid::Uuid::new_v4().to_string();
    let code = uuid::Uuid::new_v4().simple().to_string();
    let expiry = now() + 600;
    let store = app.state::<Store>();
    let mut connection = store.connect()?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    if let Some(previous) = previous {
        let validating: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pairing_claims WHERE invitation_id=?1 AND state='validating')", [previous], |r| r.get(0)).map_err(|e| e.to_string())?;
        if validating {
            return Err("接入检查正在进行，请等待检查结束。".into());
        }
        tx.execute(
            "UPDATE pairing_invitations SET state='revoked' WHERE id=?1 AND state='pending'",
            [previous],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute("INSERT INTO pairing_invitations(id,instance_id,expected_provider,display_name,code_hash,expires_at,protocol_version) VALUES(?1,?2,'',?3,?4,?5,2)",params![id,service.instance,name.trim(),hash(&code),expiry]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let display_label = if name.trim().is_empty() {
        "自动按接入组件命名".to_owned()
    } else {
        name.trim().to_owned()
    };
    let prompt=format!("请将你自己使用的本机 Agent 运行组件接入我的 AgentCrew。\nAgentCrew 地址：{address}\n应用实例：{}\n协议版本：2\n邀请 ID：{id}\n配对码：{code}\n有效期至：UTC Unix 时间 {expiry}（10 分钟）\n显示名称：{}\n邀请不绑定厂商，请自行识别并上报 provider_kind 和 runtime_kind。\n\n1. 用你已有的命令或网络工具读取 /local/v2/health 和 /local/v2/pairing-guide，确认实例和协议。\n2. 识别自己的厂商和运行组件，按说明提交请求，不让另一位 Agent 代办。Gemini 应上报自己的 Gemini CLI；不借 Codex 的运行身份。未知组件或无法识别时如实报告。\n3. 生成 request_id，重试保持不变。用返回的短期 claim_token 每秒查询一次，最多 60 秒。202 仅表示受理；只有返回 active 才报告‘已接入 AgentCrew’，并给出实际核验的名称、厂商、组件和版本。\n4. 不打印或保存配对码和令牌，不提供账号密钥、登录文件或环境变量全集。后续由 AgentCrew 调用对应组件执行，任务结果在应用中显示；Gemini 不由 Codex 代执行。\n5. 本次只授权登记及无模型接入检查，不安装依赖、不修改原厂全局配置、不开放其他网络端口、不变更项目文件。沙箱阻止请求时使用原厂权限机制；失败报告真实原因，不冒充成功。",service.instance,display_label);
    Ok(
        json!({"id":id,"display_name":name.trim(),"protocol_version":2,"code":code,"expires_at":expiry,"prompt":prompt,"state":"pending"}),
    )
}

async fn claim(
    State(state): State<Server>,
    headers: HeaderMap,
    body: Result<Json<Claim>, axum::extract::rejection::JsonRejection>,
) -> Response {
    claim_impl(state, headers, body, 1).await
}
async fn claim_v2(
    State(state): State<Server>,
    headers: HeaderMap,
    body: Result<Json<Claim>, axum::extract::rejection::JsonRejection>,
) -> Response {
    claim_impl(state, headers, body, 2).await
}
async fn claim_impl(
    state: Server,
    headers: HeaderMap,
    body: Result<Json<Claim>, axum::extract::rejection::JsonRejection>,
    version: u32,
) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    let Json(request) = match body {
        Ok(body) => body,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "请求必须为不超过 32 KiB 的有效配对 JSON。",
            )
        }
    };
    if !agents::supported(&request.provider_kind) {
        return error(
            StatusCode::BAD_REQUEST,
            "provider_not_supported",
            "已有程序调用路线，AgentCrew 适配尚未开发。",
        );
    }
    if request.protocol_version != version {
        return error(
            StatusCode::BAD_REQUEST,
            "protocol_unsupported",
            "协议版本不支持。",
        );
    }
    let runtime = request.runtime_kind.as_deref().or_else(|| {
        if version == 1 {
            agents::runtime_kind(&request.provider_kind)
        } else {
            None
        }
    });
    if runtime.is_none() || !agents::supports_runtime(&request.provider_kind, runtime.unwrap_or(""))
    {
        return error(StatusCode::BAD_REQUEST,"runtime_not_supported","未适配此厂商与运行组件组合；Gemini 目前支持 Gemini CLI ACP，不支持 Antigravity，也不会转给 Codex。");
    }
    if request.server_instance_id != state.service.instance {
        return error(
            StatusCode::CONFLICT,
            "instance_mismatch",
            "应用已重启，请重新生成邀请。",
        );
    }
    if uuid::Uuid::parse_str(&request.request_id).is_err()
        || request.pairing_code.len() > 128
        || request.invitation_id.len() > 64
        || request.runtime_hint.as_ref().is_some_and(|h| {
            h.executable_path.as_ref().is_some_and(|s| s.len() > 4096)
                || h.version.as_ref().is_some_and(|s| s.len() > 128)
        })
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "请求字段无效或过长。",
        );
    }
    if let Some(path) = request
        .runtime_hint
        .as_ref()
        .and_then(|hint| hint.executable_path.as_deref())
    {
        let path = std::path::Path::new(path);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let accepted = match request.provider_kind.as_str() {
            "codex" => matches!(name.as_str(), "codex" | "codex.exe"),
            "deepseek" => matches!(name.as_str(), "deepseek harness" | "deepseek harness.exe"),
            "gemini" => matches!(
                name.as_str(),
                "gemini" | "gemini.cmd" | "gemini.ps1" | "gemini.js" | "index.js"
            ),
            _ => false,
        };
        if !path.is_absolute() || !accepted {
            return error(
                StatusCode::BAD_REQUEST,
                "runtime_rejected",
                "运行线索必须是对应厂商的绝对运行文件路径，不能提供 shell 或脚本。",
            );
        }
    }
    {
        let mut failures = state
            .service
            .failures
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        failures.retain(|at| *at + 60 > now());
        if failures.len() >= 20 {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "请求过于频繁，请稍后重试。",
            );
        }
    }
    let store = state.app.state::<Store>();
    let result = claim_record(&store, &state.service, &request);
    let (response, validate) = match result {
        Ok(result) => result,
        Err((status, code, message)) => {
            state
                .service
                .failures
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(now());
            return error(status, code, &message);
        }
    };
    if validate {
        let app = state.app.clone();
        let claim_id = response["claim_id"].as_str().unwrap().to_owned();
        let agent_id = response["agent_id"].as_str().unwrap().to_owned();
        let kind = request.provider_kind;
        let deadline = now() + 60;
        tauri::async_runtime::spawn(async move {
            let worker = app.clone();
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                tauri::async_runtime::spawn_blocking(move || {
                    let gate = worker.state::<crate::deepseek::ExecutionGate>();
                    let _guard = gate.0.lock().map_err(|e| e.to_string())?;
                    let store = worker.state::<Store>();
                    if now() >= deadline {
                        return Err("validation_timeout".to_owned());
                    }
                    crate::deepseek::ensure_idle(&store)?;
                    agents::probe(&worker, &store, &kind)
                }),
            )
            .await;
            let probe = match result {
                Ok(Ok(result)) => result,
                _ => Err("validation_timeout".into()),
            };
            let _ = finish(&app.state::<Store>(), &claim_id, &agent_id, probe);
            crate::events::emit_agents_sync();
        });
    }
    (StatusCode::ACCEPTED, Json(response)).into_response()
}

type ClaimError = (StatusCode, &'static str, String);
fn claim_record(
    store: &Store,
    service: &Service,
    request: &Claim,
) -> Result<(Value, bool), ClaimError> {
    let failure = |_: rusqlite::Error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_failed",
            "无法保存接入请求。".into(),
        )
    };
    let mut connection = store.connect().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_failed",
            "数据库不可用。".into(),
        )
    })?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(failure)?;
    let invitation: Option<(String,String,String,String,i64,String,u32,u32)> = tx.query_row("SELECT instance_id,expected_provider,display_name,code_hash,expires_at,state,failures,protocol_version FROM pairing_invitations WHERE id=?1",[&request.invitation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional().map_err(failure)?;
    let Some((instance, provider, name, code, expiry, state, failures, protocol)) = invitation
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "invitation_invalid",
            "邀请不存在。".into(),
        ));
    };
    if instance != service.instance || expiry < now() || state == "revoked" {
        return Err((
            StatusCode::GONE,
            "invitation_expired",
            "邀请已失效，请重新生成。".into(),
        ));
    }
    if failures >= 5 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "invitation_locked",
            "邀请码已锁定，请重新生成。".into(),
        ));
    }
    if code != hash(&request.pairing_code) {
        tx.execute(
            "UPDATE pairing_invitations SET failures=failures+1 WHERE id=?1",
            [&request.invitation_id],
        )
        .map_err(failure)?;
        tx.commit().map_err(failure)?;
        return Err((
            StatusCode::FORBIDDEN,
            "invitation_invalid",
            "配对码错误。".into(),
        ));
    }
    if protocol != request.protocol_version {
        return Err((
            StatusCode::CONFLICT,
            "protocol_unsupported",
            "邀请与请求协议版本不匹配，请使用对应接入说明。".into(),
        ));
    }
    if protocol == 1 && provider != request.provider_kind {
        return Err((
            StatusCode::CONFLICT,
            "provider_mismatch",
            "邀请类型不匹配。".into(),
        ));
    }
    let fingerprint = hash(&json!({"provider":request.provider_kind,"runtime":request.runtime_kind,"protocol":request.protocol_version,"hint":request.runtime_hint.as_ref().map(|h|json!({"path":h.executable_path,"version":h.version}))}).to_string());
    let existing: Option<(String,String,String)> = tx.query_row("SELECT id,agent_id,request_fingerprint FROM pairing_claims WHERE invitation_id=?1 AND request_id=?2",params![request.invitation_id,request.request_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(failure)?;
    let validate = existing.is_none();
    let (id, agent) = if let Some((id, agent, old)) = existing {
        if old != fingerprint {
            return Err((
                StatusCode::CONFLICT,
                "request_conflict",
                "相同 request_id 的内容已改变。".into(),
            ));
        }
        (id, agent)
    } else {
        if state != "pending" {
            return Err((
                StatusCode::CONFLICT,
                "invitation_used",
                "邀请已被领取。".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let agent = uuid::Uuid::new_v4().to_string();
        let runtime = agents::runtime_kind(&request.provider_kind).ok_or((
            StatusCode::BAD_REQUEST,
            "provider_not_supported",
            "此厂商尚未适配。".into(),
        ))?;
        let resolved_name = resolve_agent_display_name(&tx, &name, &request.provider_kind).map_err(failure)?;
        tx.execute("INSERT INTO agents(id,display_name,provider_kind,runtime_kind,registration_state) VALUES(?1,?2,?3,?4,'validating')",params![agent,resolved_name,request.provider_kind,runtime]).map_err(failure)?;
        tx.execute("INSERT INTO pairing_claims(id,invitation_id,request_id,request_fingerprint,agent_id,token_hash,token_expiry,reported_provider_kind,reported_runtime_kind) VALUES(?1,?2,?3,?4,?5,'',0,?6,?7)",params![id,request.invitation_id,request.request_id,fingerprint,agent,request.provider_kind,runtime]).map_err(failure)?;
        tx.execute(
            "UPDATE pairing_invitations SET state='used' WHERE id=?1",
            [&request.invitation_id],
        )
        .map_err(failure)?;
        (id, agent)
    };
    let claim_state: String = tx
        .query_row("SELECT state FROM pairing_claims WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .map_err(failure)?;
    let token = secret();
    let expires = now() + 120;
    tx.execute(
        "UPDATE pairing_claims SET token_hash=?2,token_expiry=?3 WHERE id=?1",
        params![id, hash(&token), expires],
    )
    .map_err(failure)?;
    tx.commit().map_err(failure)?;
    Ok((
        json!({"claim_id":id,"claim_token":token,"claim_token_expires_at":expires,"agent_id":agent,"state":claim_state,"poll_after_ms":1000}),
        validate,
    ))
}

fn finish(
    store: &Store,
    claim: &str,
    agent: &str,
    probe: Result<Value, String>,
) -> Result<(), String> {
    let mut connection = store.connect()?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (state, code, reason) = if probe.is_ok() {
        ("active", None, None)
    } else {
        let timeout = probe
            .as_ref()
            .err()
            .is_some_and(|e| e == "validation_timeout");
        (
            "failed",
            Some(if timeout {
                "validation_timeout"
            } else {
                let message = probe.as_ref().err().map(String::as_str).unwrap_or("");
                [
                    "runtime_not_found",
                    "runtime_identity_mismatch",
                    "runtime_version_unsupported",
                    "authentication_failed",
                    "quota_exhausted",
                    "runtime_init_failed",
                ]
                .into_iter()
                .find(|code| message.starts_with(code))
                .unwrap_or("runtime_init_failed")
            }),
            probe.as_ref().err().map(String::as_str),
        )
    };
    let changed=tx.execute("UPDATE pairing_claims SET state=?2,reason_code=?3,reason=?4 WHERE id=?1 AND state='validating'",params![claim,state,code,reason]).map_err(|e| e.to_string())?;
    if changed == 0 {
        return Ok(());
    }
    tx.execute(
        "UPDATE agents SET registration_state=?2,availability=?3,error=?4 WHERE id=?1",
        params![
            agent,
            state,
            if state == "active" {
                "ready"
            } else {
                "unavailable"
            },
            reason
        ],
    )
    .map_err(|e| e.to_string())?;
    if let Ok(probe) = &probe {
        let kind: String = tx
            .query_row(
                "SELECT provider_kind FROM agents WHERE id=?1",
                [agent],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let capabilities = agents::capabilities(&kind, probe);
        tx.execute("UPDATE pairing_claims SET verified_provider_kind=?2,verified_runtime_kind=?3,verified_runtime_version=?4 WHERE id=?1",params![claim,kind,agents::runtime_kind(&kind),probe["version"].as_str()]).map_err(|e|e.to_string())?;
        let login_needed = probe["requires_auth"] == true && probe["auth"].is_null();
        tx.execute("UPDATE agents SET runtime_version=?2,executable_path=?3,capabilities=?4,availability=?5,error=?6,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",params![agent,probe["version"].as_str(),probe["executable"].as_str(),capabilities.to_string(),if login_needed {"degraded"} else {"ready"},if login_needed {Some("请先完成原厂登录；认证与额度以真实执行为准。")} else {None}]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

async fn status(
    State(state): State<Server>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !headers_ok(&state, &headers) {
        return error(
            StatusCode::FORBIDDEN,
            "request_rejected",
            "只允许本机直接请求。",
        );
    }
    let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "需要短期查询凭证。",
        );
    };
    if token.len() > 128 {
        return error(StatusCode::UNAUTHORIZED, "unauthorized", "查询凭证无效。");
    }
    let store = state.app.state::<Store>();
    let result = (|| -> Result<Value, String> {
        store.connect()?.query_row("SELECT c.agent_id,c.state,c.reason_code,c.reason,a.display_name,c.reported_provider_kind,c.reported_runtime_kind,c.verified_provider_kind,c.verified_runtime_kind,c.verified_runtime_version FROM pairing_claims c JOIN agents a ON a.id=c.agent_id JOIN pairing_invitations i ON i.id=c.invitation_id WHERE c.id=?1 AND c.token_hash=?2 AND c.token_expiry>=?3 AND i.instance_id=?4",params![id,hash(token),now(),state.service.instance],|r|Ok(json!({"agent_id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"reason_code":r.get::<_,Option<String>>(2)?,"message":r.get::<_,Option<String>>(3)?,"display_name":r.get::<_,String>(4)?,"reported_provider_kind":r.get::<_,Option<String>>(5)?,"reported_runtime_kind":r.get::<_,Option<String>>(6)?,"verified_provider_kind":r.get::<_,Option<String>>(7)?,"verified_runtime_kind":r.get::<_,Option<String>>(8)?,"runtime_version":r.get::<_,Option<String>>(9)?,"retryable":false}))).map_err(|e|e.to_string())
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "查询凭证无效、已被替换或已过期。",
        ),
    }
}

pub fn recover(store: &Store) -> Result<(), String> {
    let connection = store.connect()?;
    connection.execute_batch("UPDATE pairing_invitations SET state='revoked' WHERE state='pending'; UPDATE pairing_claims SET state='failed',reason_code='instance_mismatch',reason='应用已重启，请重新生成邀请。' WHERE state='validating'; UPDATE agents SET registration_state='failed',availability='unavailable',error='应用已重启，请重新邀请。' WHERE registration_state IN ('pending','validating'); UPDATE agents SET availability='checking' WHERE registration_state='active' AND enabled=1;").map_err(|e|e.to_string())
}

fn resolve_agent_display_name(
    tx: &rusqlite::Transaction<'_>,
    requested: &str,
    provider_kind: &str,
) -> Result<String, rusqlite::Error> {
    let trimmed = requested.trim();
    if !trimmed.is_empty() && trimmed != "新 Agent" {
        return Ok(trimmed.to_owned());
    }
    let base = match provider_kind {
        "gemini" => "Gemini",
        "codex" => "Codex",
        "deepseek" => "DeepSeek",
        other => other,
    };
    let mut stmt = tx.prepare("SELECT display_name FROM agents WHERE registration_state!='revoked'")?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if !existing.iter().any(|item| item.eq_ignore_ascii_case(base)) {
        return Ok(base.to_owned());
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base} {index}");
        if !existing.iter().any(|item| item.eq_ignore_ascii_case(&candidate)) {
            return Ok(candidate);
        }
        index += 1;
    }
}
