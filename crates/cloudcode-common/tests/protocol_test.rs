use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use cloudcode_common::session::{SessionInfo, SessionState};

// ---------------------------------------------------------------------------
// DaemonRequest tests
// ---------------------------------------------------------------------------

mod daemon_request {
    use super::*;

    #[test]
    fn spawn_with_name_roundtrip() {
        let req = DaemonRequest::Spawn {
            name: Some("my-session".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Spawn { name } => {
                assert_eq!(name, Some("my-session".to_string()));
            }
            other => panic!("Expected Spawn, got {:?}", other),
        }
    }

    #[test]
    fn spawn_with_none_name_roundtrip() {
        let req = DaemonRequest::Spawn { name: None };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Spawn { name } => {
                assert_eq!(name, None);
            }
            other => panic!("Expected Spawn, got {:?}", other),
        }
    }

    #[test]
    fn spawn_tagged_format() {
        let req = DaemonRequest::Spawn {
            name: Some("foo".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "spawn");
        assert_eq!(value["name"], "foo");
    }

    #[test]
    fn spawn_none_name_tagged_format() {
        let req = DaemonRequest::Spawn { name: None };
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "spawn");
        // When name is None, serde omits it or sets it to null
        assert!(value.get("name").is_none() || value["name"].is_null());
    }

    #[test]
    fn list_roundtrip() {
        let req = DaemonRequest::List;
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, DaemonRequest::List));
    }

    #[test]
    fn list_tagged_format() {
        let req = DaemonRequest::List;
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "list");
    }

    #[test]
    fn kill_roundtrip() {
        let req = DaemonRequest::Kill {
            session: "sess-1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Kill { session } => assert_eq!(session, "sess-1"),
            other => panic!("Expected Kill, got {:?}", other),
        }
    }

    #[test]
    fn kill_tagged_format() {
        let req = DaemonRequest::Kill {
            session: "sess-1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "kill");
        assert_eq!(value["session"], "sess-1");
    }

    #[test]
    fn send_roundtrip() {
        let req = DaemonRequest::Send {
            session: "sess-2".to_string(),
            message: "hello world".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Send { session, message } => {
                assert_eq!(session, "sess-2");
                assert_eq!(message, "hello world");
            }
            other => panic!("Expected Send, got {:?}", other),
        }
    }

    #[test]
    fn send_tagged_format() {
        let req = DaemonRequest::Send {
            session: "s".to_string(),
            message: "m".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "send");
        assert_eq!(value["session"], "s");
        assert_eq!(value["message"], "m");
    }

    #[test]
    fn status_roundtrip() {
        let req = DaemonRequest::Status;
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, DaemonRequest::Status));
    }

    #[test]
    fn status_tagged_format() {
        let req = DaemonRequest::Status;
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "status");
    }

    #[test]
    fn deserialize_unknown_type_returns_error() {
        let json = r#"{"type":"unknown_variant","foo":"bar"}"#;
        let result = serde_json::from_str::<DaemonRequest>(json);
        assert!(result.is_err(), "Deserializing an unknown type should fail");
    }

    #[test]
    fn deserialize_from_known_json_string() {
        // Verify we can deserialize from a hand-written JSON payload
        let json = r#"{"type":"spawn","name":"test-session"}"#;
        let req: DaemonRequest = serde_json::from_str(json).unwrap();
        match req {
            DaemonRequest::Spawn { name } => assert_eq!(name, Some("test-session".to_string())),
            other => panic!("Expected Spawn, got {:?}", other),
        }
    }

    #[test]
    fn send_with_special_characters() {
        let req = DaemonRequest::Send {
            session: "sess".to_string(),
            message: "line1\nline2\ttab \"quoted\"".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Send { message, .. } => {
                assert_eq!(message, "line1\nline2\ttab \"quoted\"");
            }
            other => panic!("Expected Send, got {:?}", other),
        }
    }

    #[test]
    fn send_with_empty_message() {
        let req = DaemonRequest::Send {
            session: "s".to_string(),
            message: String::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Send { message, .. } => assert!(message.is_empty()),
            other => panic!("Expected Send, got {:?}", other),
        }
    }

    #[test]
    fn send_with_unicode() {
        let req = DaemonRequest::Send {
            session: "s".to_string(),
            message: "Hello \u{1F600} world \u{00E9}\u{00F1}".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonRequest::Send { message, .. } => {
                assert!(message.contains('\u{1F600}'));
            }
            other => panic!("Expected Send, got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// DaemonResponse tests
// ---------------------------------------------------------------------------

mod daemon_response {
    use super::*;

    fn sample_session() -> SessionInfo {
        SessionInfo {
            name: "test".to_string(),
            state: SessionState::Running,
            created_at: 1700000000,
            last_activity: 1700000100,
        }
    }

    #[test]
    fn spawned_roundtrip() {
        let resp = DaemonResponse::Spawned {
            session: sample_session(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Spawned { session } => {
                assert_eq!(session.name, "test");
                assert_eq!(session.state, SessionState::Running);
            }
            other => panic!("Expected Spawned, got {:?}", other),
        }
    }

    #[test]
    fn spawned_tagged_format() {
        let resp = DaemonResponse::Spawned {
            session: sample_session(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "spawned");
    }

    #[test]
    fn sessions_roundtrip_empty() {
        let resp = DaemonResponse::Sessions {
            sessions: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Sessions { sessions } => assert!(sessions.is_empty()),
            other => panic!("Expected Sessions, got {:?}", other),
        }
    }

    #[test]
    fn sessions_roundtrip_multiple() {
        let resp = DaemonResponse::Sessions {
            sessions: vec![sample_session(), sample_session()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Sessions { sessions } => assert_eq!(sessions.len(), 2),
            other => panic!("Expected Sessions, got {:?}", other),
        }
    }

    #[test]
    fn sessions_tagged_format() {
        let resp = DaemonResponse::Sessions { sessions: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "sessions");
    }

    #[test]
    fn killed_roundtrip() {
        let resp = DaemonResponse::Killed {
            session: "sess-x".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Killed { session } => assert_eq!(session, "sess-x"),
            other => panic!("Expected Killed, got {:?}", other),
        }
    }

    #[test]
    fn killed_tagged_format() {
        let resp = DaemonResponse::Killed {
            session: "s".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "killed");
        assert_eq!(value["session"], "s");
    }

    #[test]
    fn send_result_roundtrip() {
        let resp = DaemonResponse::SendResult {
            output: "result text".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::SendResult { output } => assert_eq!(output, "result text"),
            other => panic!("Expected SendResult, got {:?}", other),
        }
    }

    #[test]
    fn send_result_tagged_format() {
        let resp = DaemonResponse::SendResult {
            output: "ok".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "send_result");
    }

    #[test]
    fn status_roundtrip() {
        let resp = DaemonResponse::Status {
            uptime_secs: 3600,
            sessions: vec![sample_session()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
            } => {
                assert_eq!(uptime_secs, 3600);
                assert_eq!(sessions.len(), 1);
            }
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    #[test]
    fn status_tagged_format() {
        let resp = DaemonResponse::Status {
            uptime_secs: 0,
            sessions: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "status");
        assert_eq!(value["uptime_secs"], 0);
    }

    #[test]
    fn status_with_zero_uptime_and_empty_sessions() {
        let resp = DaemonResponse::Status {
            uptime_secs: 0,
            sessions: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
            } => {
                assert_eq!(uptime_secs, 0);
                assert!(sessions.is_empty());
            }
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    #[test]
    fn status_with_max_uptime() {
        let resp = DaemonResponse::Status {
            uptime_secs: u64::MAX,
            sessions: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Status { uptime_secs, .. } => {
                assert_eq!(uptime_secs, u64::MAX);
            }
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    #[test]
    fn error_roundtrip() {
        let resp = DaemonResponse::Error {
            message: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Error { message } => {
                assert_eq!(message, "something went wrong");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn error_tagged_format() {
        let resp = DaemonResponse::Error {
            message: "err".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "error");
        assert_eq!(value["message"], "err");
    }

    #[test]
    fn deserialize_unknown_response_type_returns_error() {
        let json = r#"{"type":"bogus","data":123}"#;
        let result = serde_json::from_str::<DaemonResponse>(json);
        assert!(result.is_err());
    }

    #[test]
    fn error_with_empty_message() {
        let resp = DaemonResponse::Error {
            message: String::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Error { message } => assert!(message.is_empty()),
            other => panic!("Expected Error, got {:?}", other),
        }
    }
}
