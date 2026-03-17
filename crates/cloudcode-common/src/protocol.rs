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
    },
    CleanedUp {
        sessions: Vec<String>,
    },
    Error {
        message: String,
    },
}
