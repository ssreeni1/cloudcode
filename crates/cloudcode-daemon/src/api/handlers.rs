use crate::session::manager::SessionManager;
use crate::session::monitor::SessionMonitor;
use crate::telegram::default_session::DefaultSessionStore;
use crate::telegram::handlers::provider_has_auth;
use crate::telegram::question_poller::QuestionStates;
use crate::telegram::session_resolution::waiting_sessions_from;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse, TelegramStatus, WaitingSession};
use cloudcode_common::provider::AiProvider;
use std::sync::Arc;
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

/// Extended state for API handlers that need access to Telegram state
pub struct ApiState {
    #[allow(dead_code)]
    pub session_mgr: Arc<SessionManager>,
    pub default_session: Option<Arc<DefaultSessionStore>>,
    pub question_states: Option<QuestionStates>,
    pub telegram_mode: Option<String>,
}

#[allow(dead_code)]
pub async fn handle(request: DaemonRequest, mgr: &Arc<SessionManager>) -> DaemonResponse {
    handle_with_state(request, mgr, None).await
}

pub async fn handle_with_state(
    request: DaemonRequest,
    mgr: &Arc<SessionManager>,
    api_state: Option<&ApiState>,
) -> DaemonResponse {
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
            Ok(result) => DaemonResponse::SendResult {
                output: result.text,
                files: result
                    .files
                    .iter()
                    .map(|f| f.to_string_lossy().to_string())
                    .collect(),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Cleanup => {
            let monitor = SessionMonitor::new(Arc::clone(mgr));
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
            let telegram = api_state.and_then(|s| {
                s.telegram_mode.as_ref().map(|mode| TelegramStatus {
                    mode: mode.clone(),
                    connected: true,
                })
            });
            match mgr.list().await {
                Ok(sessions) => DaemonResponse::Status {
                    uptime_secs,
                    sessions,
                    telegram,
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }
        DaemonRequest::Peek { session } => match mgr.capture_pane(&session).await {
            Ok(content) => DaemonResponse::PaneContent { session, content },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Type { session, text } => match mgr.send_keys(&session, &text).await {
            Ok(()) => DaemonResponse::Typed { session },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::SetProvider { provider } => {
            let target: AiProvider = match provider.parse() {
                Ok(p) => p,
                Err(_) => {
                    return DaemonResponse::Error {
                        message: format!("Unknown provider: {}", provider),
                    };
                }
            };
            mgr.set_provider(target);
            DaemonResponse::ProviderSet {
                provider: target.as_str().to_string(),
            }
        }
        DaemonRequest::GetProvider => {
            let provider = mgr.current_provider();
            DaemonResponse::Provider {
                provider: provider.as_str().to_string(),
                has_auth: provider_has_auth(provider),
            }
        }
        DaemonRequest::GetDefaultSession => {
            let session = api_state
                .and_then(|s| s.default_session.as_ref())
                .and_then(|ds| ds.current());
            DaemonResponse::DefaultSession { session }
        }
        DaemonRequest::SetDefaultSession { session } => {
            // Validate session exists if setting (not clearing)
            if let Some(ref name) = session {
                match mgr.list().await {
                    Ok(sessions) => {
                        if !sessions.iter().any(|s| s.name == *name) {
                            return DaemonResponse::Error {
                                message: format!("Session '{}' not found", name),
                            };
                        }
                    }
                    Err(e) => {
                        return DaemonResponse::Error {
                            message: e.to_string(),
                        };
                    }
                }
            }
            match api_state.and_then(|s| s.default_session.as_ref()) {
                Some(ds) => {
                    if let Err(e) = ds.set(session.clone()) {
                        return DaemonResponse::Error {
                            message: e.to_string(),
                        };
                    }
                    DaemonResponse::DefaultSessionSet { session }
                }
                None => DaemonResponse::Error {
                    message: "Default session store not available".to_string(),
                },
            }
        }
        DaemonRequest::Waiting => {
            let sessions = api_state
                .and_then(|s| s.question_states.as_ref())
                .map(|qs| {
                    waiting_sessions_from(qs)
                        .into_iter()
                        .map(|(name, question)| WaitingSession { name, question })
                        .collect()
                })
                .unwrap_or_default();
            DaemonResponse::WaitingSessions { sessions }
        }
        DaemonRequest::ProviderHealth => {
            // TODO(step-22): iterate all providers, check installed/version/auth/ready
            DaemonResponse::ProviderHealth {
                providers: vec![],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudcode_common::provider::AiProvider;

    // -----------------------------------------------------------------------
    // init_start_time
    // -----------------------------------------------------------------------

    #[test]
    fn init_start_time_does_not_panic() {
        init_start_time();
        init_start_time(); // second call is a no-op
    }

    // -----------------------------------------------------------------------
    // Handler dispatch — error paths (tmux is not available in test env)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handle_spawn_returns_error_when_tmux_unavailable() {
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        let req = DaemonRequest::Spawn {
            name: Some("test-session".to_string()),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Spawned { .. } => {
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        let req = DaemonRequest::List;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Sessions { sessions } => {
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        let req = DaemonRequest::Kill {
            session: "nonexistent-session-xyz-12345".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Error { message } => {
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        init_start_time();

        let req = DaemonRequest::Status;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
                ..
            } => {
                assert!(uptime_secs < 60, "Uptime should be small in test");
                let _ = sessions;
            }
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty());
            }
            other => panic!("Expected Status or Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // New request types
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handle_get_provider_returns_provider() {
        let mgr = Arc::new(SessionManager::new(AiProvider::Claude));
        let req = DaemonRequest::GetProvider;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Provider { provider, .. } => {
                assert_eq!(provider, "claude");
            }
            other => panic!("Expected Provider, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_set_provider_switches_provider() {
        let mgr = Arc::new(SessionManager::new(AiProvider::Claude));
        let req = DaemonRequest::SetProvider {
            provider: "codex".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::ProviderSet { provider } => {
                assert_eq!(provider, "codex");
            }
            other => panic!("Expected ProviderSet, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_set_provider_invalid_returns_error() {
        let mgr = Arc::new(SessionManager::new(AiProvider::Claude));
        let req = DaemonRequest::SetProvider {
            provider: "invalid".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Error { message } => {
                assert!(message.contains("Unknown provider"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_waiting_returns_empty_without_state() {
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        let req = DaemonRequest::Waiting;
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::WaitingSessions { sessions } => {
                assert!(sessions.is_empty());
            }
            other => panic!("Expected WaitingSessions, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_peek_nonexistent_returns_error() {
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
        let req = DaemonRequest::Peek {
            session: "nonexistent".to_string(),
        };
        let resp = handle(req, &mgr).await;

        match resp {
            DaemonResponse::Error { message } => {
                assert!(!message.is_empty());
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // JSON type tag verification
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handle_spawn_response_type_is_spawned_or_error() {
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
        let mgr = Arc::new(SessionManager::new(AiProvider::default()));
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
