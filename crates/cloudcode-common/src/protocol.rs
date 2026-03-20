use serde::{Deserialize, Serialize};

use crate::session::SessionInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    Spawn { name: Option<String> },
    List,
    Kill { session: String },
    Send { session: String, message: String },
    Status,
    Cleanup,
    Peek { session: String },
    Type { session: String, text: String },
    SetProvider { provider: String },
    GetProvider,
    GetDefaultSession,
    SetDefaultSession { session: Option<String> },
    Waiting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitingSession {
    pub name: String,
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramStatus {
    pub mode: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonResponse {
    Spawned {
        session: SessionInfo,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    Killed {
        session: String,
    },
    SendResult {
        output: String,
        #[serde(default)]
        files: Vec<String>,
    },
    Status {
        uptime_secs: u64,
        sessions: Vec<SessionInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        telegram: Option<TelegramStatus>,
    },
    CleanedUp {
        sessions: Vec<String>,
    },
    PaneContent {
        session: String,
        content: String,
    },
    Typed {
        session: String,
    },
    ProviderSet {
        provider: String,
    },
    Provider {
        provider: String,
        has_auth: bool,
    },
    DefaultSession {
        session: Option<String>,
    },
    DefaultSessionSet {
        session: Option<String>,
    },
    WaitingSessions {
        sessions: Vec<WaitingSession>,
    },
    Error {
        message: String,
    },
}
