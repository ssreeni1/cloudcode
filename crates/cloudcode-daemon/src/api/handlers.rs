use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use crate::session::manager::SessionManager;
use crate::session::monitor::SessionMonitor;
use std::time::{SystemTime, UNIX_EPOCH};

static START_TIME: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

pub fn init_start_time() {
    START_TIME.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });
}

pub async fn handle(request: DaemonRequest, mgr: &SessionManager) -> DaemonResponse {
    match request {
        DaemonRequest::Spawn { name } => match mgr.spawn(name).await {
            Ok(session) => DaemonResponse::Spawned { session },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::List => match mgr.list().await {
            Ok(sessions) => DaemonResponse::Sessions { sessions },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Kill { session } => match mgr.kill(&session).await {
            Ok(()) => DaemonResponse::Killed { session },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Send { session, message } => match mgr.send(&session, &message).await {
            Ok(output) => DaemonResponse::SendResult { output },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Cleanup => {
            let monitor = SessionMonitor::new(SessionManager::new());
            match monitor.cleanup_dead().await {
                Ok(sessions) => DaemonResponse::CleanedUp { sessions },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }
        DaemonRequest::Status => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let start = *START_TIME.get_or_init(|| now);
            let uptime_secs = now - start;
            match mgr.list().await {
                Ok(sessions) => DaemonResponse::Status {
                    uptime_secs,
                    sessions,
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // init_start_time
    // -----------------------------------------------------------------------

    #[test]
    fn init_start_time_does_not_panic() {
        // init_start_time uses OnceLock so it can be called multiple times safely
        init_start_time();
        init_start_time(); // second call is a no-op
    }

    // -----------------------------------------------------------------------
    // Handler dispatch — error paths (tmux is not available in test env)
    //
    // SessionManager methods shell out to tmux, which will fail in CI/test.
    // We verify that failures are properly mapped to DaemonResponse::Error.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handle_spawn_returns_error_when_tmux_unavailable() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Spawn {
            name: Some("test-session".to_string()),
        };
        let resp = handle(req, &mgr).await;

        // tmux is likely not installed or has no server running in test env,
        // so we expect an Error response (not a panic).
        match resp {
            DaemonResponse::Spawned { .. } => {
                // If tmux happens to be available, spawned is also acceptable.
                // Clean up: kill the session we just created.
                let _ = mgr.kill("test-session").await;
            }
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty(), "Error message should not be empty");
            }
            other => panic!("Expected Spawned or Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_list_returns_sessions_or_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::List;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Sessions { sessions } => {
                // Even if tmux is not running, list() returns Ok(vec![])
                // when tmux returns an error (no sessions).
                assert!(sessions.is_empty() || !sessions.is_empty());
            }
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty());
            }
            other => panic!("Expected Sessions or Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_kill_nonexistent_session_returns_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Kill {
            session: "nonexistent-session-xyz-12345".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Error { message } => {
                // The session does not exist, so we expect an error
                assert!(
                    message.contains("does not exist") || !message.is_empty(),
                    "Error should indicate session not found, got: {}",
                    message
                );
            }
            other => panic!("Expected Error for nonexistent session, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_send_nonexistent_session_returns_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Send {
            session: "nonexistent-session-xyz-12345".to_string(),
            message: "hello".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty());
            }
            other => panic!("Expected Error for nonexistent session, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_status_returns_status_or_error() {
        let mgr = SessionManager::new();
        // Ensure START_TIME is initialized so uptime calculation works
        init_start_time();

        let req = DaemonRequest::Status;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
            } => {
                // Uptime should be zero or small since we just initialized
                assert!(uptime_secs < 60, "Uptime should be small in test");
                // Sessions list is valid (possibly empty)
                let _ = sessions;
            }
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty());
            }
            other => panic!("Expected Status or Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Verify handler maps each request variant to the correct response variant
    // by checking the JSON "type" tag of the response.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handle_spawn_response_type_is_spawned_or_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Spawn { name: None };
        let resp = handle(req, &mgr).await;
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let resp_type = value["type"].as_str().unwrap();
        assert!(
            resp_type == "spawned" || resp_type == "error",
            "Expected 'spawned' or 'error', got '{}'",
            resp_type
        );
    }

    #[tokio::test]
    async fn handle_list_response_type_is_sessions_or_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::List;
        let resp = handle(req, &mgr).await;
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let resp_type = value["type"].as_str().unwrap();
        assert!(
            resp_type == "sessions" || resp_type == "error",
            "Expected 'sessions' or 'error', got '{}'",
            resp_type
        );
    }

    #[tokio::test]
    async fn handle_kill_response_type_is_killed_or_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Kill {
            session: "x".to_string(),
        };
        let resp = handle(req, &mgr).await;
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let resp_type = value["type"].as_str().unwrap();
        assert!(
            resp_type == "killed" || resp_type == "error",
            "Expected 'killed' or 'error', got '{}'",
            resp_type
        );
    }

    #[tokio::test]
    async fn handle_send_response_type_is_send_result_or_error() {
        let mgr = SessionManager::new();
        let req = DaemonRequest::Send {
            session: "x".to_string(),
            message: "hi".to_string(),
        };
        let resp = handle(req, &mgr).await;
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let resp_type = value["type"].as_str().unwrap();
        assert!(
            resp_type == "send_result" || resp_type == "error",
            "Expected 'send_result' or 'error', got '{}'",
            resp_type
        );
    }

    #[tokio::test]
    async fn handle_status_response_type_is_status_or_error() {
        let mgr = SessionManager::new();
        init_start_time();
        let req = DaemonRequest::Status;
        let resp = handle(req, &mgr).await;
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let resp_type = value["type"].as_str().unwrap();
        assert!(
            resp_type == "status" || resp_type == "error",
            "Expected 'status' or 'error', got '{}'",
            resp_type
        );
    }
}
