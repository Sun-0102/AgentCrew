use crate::{claude, codex, deepseek, gemini, storage::Store};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProviderDriver {
    pub provider_kind: &'static str,
    pub display_name: &'static str,
    pub runtime_kind: &'static str,
    pub runtime_label: &'static str,
    pub session_table: &'static str,
    pub auth_hint: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProviderCapabilities {
    pub resume: bool,
    pub cancel: bool,
    pub approval: bool,
    pub tools: bool,
    pub streaming: bool,
    pub acp: bool,
    pub steer: bool,
}

impl ProviderCapabilities {
    pub fn to_value(self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

/// Unified Provider Adapter trait modeled after T3 Code's Driver/Adapter architecture.
/// Adding a new CLI provider only requires implementing this trait and registering it
/// in `BUILT_IN_DRIVERS`.
pub trait ProviderAdapter: Sync {
    fn driver(&self) -> ProviderDriver;

    /// Returns true if the runtime currently holds an exclusive execution lock and
    /// should not be probed concurrently.
    fn is_busy(&self, _store: &Store) -> bool {
        false
    }

    fn probe(&self, app: &tauri::AppHandle, store: &Store) -> Result<Value, String>;

    fn capabilities(&self, _probe: &Value) -> Value {
        ProviderCapabilities {
            resume: true,
            cancel: true,
            approval: false,
            tools: true,
            streaming: true,
            acp: false,
            steer: false,
        }
        .to_value()
    }

    fn session(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String>;

    #[allow(dead_code)]
    fn send(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
    ) -> Result<Value, String> {
        self.send_with_context(app, store, runtime_conversation, model, text, role, None)
    }

    fn send_with_context(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Result<Value, String>;

    fn cancel(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String>;

    fn supports_approval(&self) -> bool {
        false
    }

    fn approvals(
        &self,
        _app: &tauri::AppHandle,
        _store: &Store,
        _runtime_conversation: &str,
    ) -> Result<Vec<Value>, String> {
        Ok(Vec::new())
    }

    fn approve(
        &self,
        _app: &tauri::AppHandle,
        _store: &Store,
        _runtime_conversation: &str,
        _id: &str,
        _allow: bool,
    ) -> Result<Value, String> {
        Err("此运行组件不提供交互审批。".into())
    }

    fn supports_steer(&self) -> bool {
        false
    }

    fn steer(
        &self,
        _app: &tauri::AppHandle,
        _store: &Store,
        _runtime_conversation: &str,
        _text: &str,
    ) -> Result<Value, String> {
        Err("此运行组件暂不支持运行中直接纠偏。".into())
    }
}

struct CodexAdapter;

impl ProviderAdapter for CodexAdapter {
    fn driver(&self) -> ProviderDriver {
        ProviderDriver {
            provider_kind: "codex",
            display_name: "Codex",
            runtime_kind: "codex-app-server",
            runtime_label: "App Server",
            session_table: "codex_sessions",
            auth_hint: "请先在本机执行 codex login 完成登录，认证与额度以真实执行为准。",
        }
    }

    fn probe(&self, app: &tauri::AppHandle, store: &Store) -> Result<Value, String> {
        app.state::<codex::Runtime>().probe(store)
    }

    fn capabilities(&self, _probe: &Value) -> Value {
        ProviderCapabilities {
            resume: true,
            cancel: true,
            approval: true,
            tools: true,
            streaming: true,
            acp: false,
            steer: true,
        }
        .to_value()
    }

    fn session(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        Ok(app
            .state::<codex::Runtime>()
            .snapshot(store, runtime_conversation)?["session"]
            .clone())
    }

    fn send_with_context(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Result<Value, String> {
        app.state::<codex::Runtime>()
            .send_recorded_with_context(store, runtime_conversation, model, text, role, extra_context)
    }

    fn cancel(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        app.state::<codex::Runtime>()
            .cancel(store, runtime_conversation)
    }

    fn supports_approval(&self) -> bool {
        true
    }

    fn approvals(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Vec<Value>, String> {
        Ok(app
            .state::<codex::Runtime>()
            .snapshot(store, runtime_conversation)?["approvals"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    fn approve(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        id: &str,
        allow: bool,
    ) -> Result<Value, String> {
        app.state::<codex::Runtime>()
            .approve(store, runtime_conversation, id, allow)
    }

    fn supports_steer(&self) -> bool {
        true
    }

    fn steer(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        text: &str,
    ) -> Result<Value, String> {
        app.state::<codex::Runtime>()
            .steer(store, runtime_conversation, text)
    }
}

struct DeepSeekAdapter;

impl ProviderAdapter for DeepSeekAdapter {
    fn driver(&self) -> ProviderDriver {
        ProviderDriver {
            provider_kind: "deepseek",
            display_name: "DeepSeek",
            runtime_kind: "deepseek-harness-sdk",
            runtime_label: "Harness SDK",
            session_table: "deepseek_sessions",
            auth_hint: "请在 DeepSeek Harness 中配置模型服务凭证，认证与额度以真实执行为准。",
        }
    }

    fn is_busy(&self, store: &Store) -> bool {
        deepseek::ensure_idle(store).is_err()
    }

    fn probe(&self, app: &tauri::AppHandle, store: &Store) -> Result<Value, String> {
        app.state::<deepseek::Runtime>().probe(store)
    }

    fn session(
        &self,
        _app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        deepseek::session(store, runtime_conversation)
    }

    fn send_with_context(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Result<Value, String> {
        app.state::<deepseek::Runtime>()
            .send_recorded_with_context(store, runtime_conversation, model, text, role, extra_context)
    }

    fn cancel(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        app.state::<deepseek::Runtime>()
            .cancel_in(store, runtime_conversation)
    }
}

struct GeminiAdapter;

impl ProviderAdapter for GeminiAdapter {
    fn driver(&self) -> ProviderDriver {
        ProviderDriver {
            provider_kind: "gemini",
            display_name: "Gemini",
            runtime_kind: "gemini-cli-acp",
            runtime_label: "ACP v1",
            session_table: "gemini_sessions",
            auth_hint: "请先在本机完成 Gemini CLI 登录配置，认证与额度以真实执行为准。",
        }
    }

    fn probe(&self, app: &tauri::AppHandle, store: &Store) -> Result<Value, String> {
        app.state::<gemini::Runtime>().probe(store)
    }

    fn capabilities(&self, probe: &Value) -> Value {
        let raw = &probe["capabilities"];
        ProviderCapabilities {
            resume: raw["resume"].as_bool().unwrap_or(true),
            cancel: raw["cancel"].as_bool().unwrap_or(true),
            approval: raw["approval"].as_bool().unwrap_or(true),
            tools: raw["tools"].as_bool().unwrap_or(true),
            streaming: raw["streaming"].as_bool().unwrap_or(true),
            acp: true,
            steer: false,
        }
        .to_value()
    }

    fn session(
        &self,
        _app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        gemini::session(store, runtime_conversation)
    }

    fn send_with_context(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Result<Value, String> {
        app.state::<gemini::Runtime>()
            .send_recorded_with_context(store, runtime_conversation, model, text, role, extra_context)
    }

    fn cancel(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        app.state::<gemini::Runtime>()
            .cancel(store, runtime_conversation)
    }

    fn supports_approval(&self) -> bool {
        true
    }

    fn approvals(
        &self,
        app: &tauri::AppHandle,
        _store: &Store,
        runtime_conversation: &str,
    ) -> Result<Vec<Value>, String> {
        app.state::<gemini::Runtime>()
            .approvals(runtime_conversation)
    }

    fn approve(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        id: &str,
        allow: bool,
    ) -> Result<Value, String> {
        app.state::<gemini::Runtime>()
            .approve(store, runtime_conversation, id, allow)
    }
}

struct ClaudeAdapter;

impl ProviderAdapter for ClaudeAdapter {
    fn driver(&self) -> ProviderDriver {
        ProviderDriver {
            provider_kind: "claude",
            display_name: "Claude Code",
            runtime_kind: "claude-code-cli",
            runtime_label: "Claude CLI",
            session_table: "claude_sessions",
            auth_hint: "请先在本机安装 Claude Code CLI 并执行 claude login 完成登录。",
        }
    }

    fn probe(&self, app: &tauri::AppHandle, store: &Store) -> Result<Value, String> {
        app.state::<claude::Runtime>().probe(store)
    }

    fn session(
        &self,
        _app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        claude::session(store, runtime_conversation)
    }

    fn send_with_context(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
        model: &str,
        text: &str,
        role: &str,
        extra_context: Option<&str>,
    ) -> Result<Value, String> {
        app.state::<claude::Runtime>()
            .send_recorded_with_context(store, runtime_conversation, model, text, role, extra_context)
    }

    fn cancel(
        &self,
        app: &tauri::AppHandle,
        store: &Store,
        runtime_conversation: &str,
    ) -> Result<Value, String> {
        app.state::<claude::Runtime>()
            .cancel(store, runtime_conversation)
    }
}

static CODEX_ADAPTER: CodexAdapter = CodexAdapter;
static DEEPSEEK_ADAPTER: DeepSeekAdapter = DeepSeekAdapter;
static GEMINI_ADAPTER: GeminiAdapter = GeminiAdapter;
static CLAUDE_ADAPTER: ClaudeAdapter = ClaudeAdapter;

pub static BUILT_IN_DRIVERS: &[&'static dyn ProviderAdapter] =
    &[&CODEX_ADAPTER, &DEEPSEEK_ADAPTER, &GEMINI_ADAPTER, &CLAUDE_ADAPTER];

pub fn all() -> &'static [&'static dyn ProviderAdapter] {
    BUILT_IN_DRIVERS
}

pub fn resolve(provider_kind: &str) -> Result<&'static dyn ProviderAdapter, String> {
    BUILT_IN_DRIVERS
        .iter()
        .copied()
        .find(|adapter| adapter.driver().provider_kind == provider_kind)
        .ok_or_else(|| "provider_not_supported".into())
}

pub fn driver(provider_kind: &str) -> Option<ProviderDriver> {
    resolve(provider_kind).ok().map(|adapter| adapter.driver())
}
