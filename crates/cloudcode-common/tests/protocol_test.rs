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
        let resp = DaemonResponse::Sessions { sessions: vec![] };
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
            files: vec!["screenshot.png".to_string()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::SendResult { output, files } => {
                assert_eq!(output, "result text");
                assert_eq!(files, vec!["screenshot.png"]);
            }
            other => panic!("Expected SendResult, got {:?}", other),
        }
    }

    #[test]
    fn send_result_tagged_format() {
        let resp = DaemonResponse::SendResult {
            output: "ok".to_string(),
            files: vec![],
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
            telegram: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
                ..
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
            telegram: None,
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
            telegram: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&json).unwrap();

        match deserialized {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
                ..
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
            telegram: None,
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

// ---------------------------------------------------------------------------
// Newline-delimited protocol integration tests
// ---------------------------------------------------------------------------

mod newline_delimited_protocol {
    use super::*;

    /// Helper: serialize a request to a newline-terminated wire format line
    fn to_wire(req: &DaemonRequest) -> String {
        let mut json = serde_json::to_string(req).unwrap();
        json.push('\n');
        json
    }

    /// Helper: serialize a response to a newline-terminated wire format line
    fn resp_to_wire(resp: &DaemonResponse) -> String {
        let mut json = serde_json::to_string(resp).unwrap();
        json.push('\n');
        json
    }

    // -----------------------------------------------------------------------
    // Single message: serialize, add newline, strip newline, deserialize
    // -----------------------------------------------------------------------

    #[test]
    fn request_survives_newline_delimited_roundtrip() {
        let requests: Vec<DaemonRequest> = vec![
            DaemonRequest::Spawn {
                name: Some("sess-1".to_string()),
            },
            DaemonRequest::Spawn { name: None },
            DaemonRequest::List,
            DaemonRequest::Kill {
                session: "s".to_string(),
            },
            DaemonRequest::Send {
                session: "s".to_string(),
                message: "msg".to_string(),
            },
            DaemonRequest::Status,
        ];

        for req in &requests {
            let wire = to_wire(req);
            // The wire format must end with exactly one newline
            assert!(wire.ends_with('\n'));
            assert!(!wire[..wire.len() - 1].contains('\n'));

            // Strip newline and deserialize (simulates BufReader::read_line then trim)
            let line = wire.trim_end_matches('\n');
            let _: DaemonRequest = serde_json::from_str(line).unwrap();
        }
    }

    #[test]
    fn response_survives_newline_delimited_roundtrip() {
        let sample = SessionInfo {
            name: "s".to_string(),
            state: SessionState::Running,
            created_at: 100,
            last_activity: 200,
        };

        let responses: Vec<DaemonResponse> = vec![
            DaemonResponse::Spawned {
                session: sample.clone(),
            },
            DaemonResponse::Sessions {
                sessions: vec![sample.clone()],
            },
            DaemonResponse::Killed {
                session: "s".to_string(),
            },
            DaemonResponse::SendResult {
                output: "ok".to_string(),
                files: vec![],
            },
            DaemonResponse::Status {
                uptime_secs: 60,
                sessions: vec![],
                telegram: None,
            },
            DaemonResponse::Error {
                message: "fail".to_string(),
            },
        ];

        for resp in &responses {
            let wire = resp_to_wire(resp);
            assert!(wire.ends_with('\n'));
            assert!(!wire[..wire.len() - 1].contains('\n'));

            let line = wire.trim_end_matches('\n');
            let _: DaemonResponse = serde_json::from_str(line).unwrap();
        }
    }

    // -----------------------------------------------------------------------
    // Multiple messages in sequence (simulating a TCP stream)
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_requests_in_stream_can_be_split_and_deserialized() {
        let requests = vec![
            DaemonRequest::List,
            DaemonRequest::Spawn {
                name: Some("a".to_string()),
            },
            DaemonRequest::Send {
                session: "a".to_string(),
                message: "hello".to_string(),
            },
            DaemonRequest::Kill {
                session: "a".to_string(),
            },
            DaemonRequest::Status,
        ];

        // Build a single byte stream of newline-delimited messages
        let stream: String = requests.iter().map(|r| to_wire(r)).collect();

        // Split on newlines and deserialize each (simulates BufReader::lines)
        let deserialized: Vec<DaemonRequest> = stream
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(deserialized.len(), requests.len());

        // Verify the types match in order
        assert!(matches!(deserialized[0], DaemonRequest::List));
        assert!(matches!(deserialized[1], DaemonRequest::Spawn { .. }));
        assert!(matches!(deserialized[2], DaemonRequest::Send { .. }));
        assert!(matches!(deserialized[3], DaemonRequest::Kill { .. }));
        assert!(matches!(deserialized[4], DaemonRequest::Status));
    }

    #[test]
    fn multiple_responses_in_stream_can_be_split_and_deserialized() {
        let sample = SessionInfo {
            name: "x".to_string(),
            state: SessionState::Idle,
            created_at: 0,
            last_activity: 0,
        };

        let responses = vec![
            DaemonResponse::Sessions { sessions: vec![] },
            DaemonResponse::Spawned {
                session: sample.clone(),
            },
            DaemonResponse::SendResult {
                output: "done".to_string(),
                files: vec![],
            },
            DaemonResponse::Killed {
                session: "x".to_string(),
            },
            DaemonResponse::Error {
                message: "oops".to_string(),
            },
        ];

        let stream: String = responses.iter().map(|r| resp_to_wire(r)).collect();

        let deserialized: Vec<DaemonResponse> = stream
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(deserialized.len(), responses.len());

        assert!(matches!(deserialized[0], DaemonResponse::Sessions { .. }));
        assert!(matches!(deserialized[1], DaemonResponse::Spawned { .. }));
        assert!(matches!(deserialized[2], DaemonResponse::SendResult { .. }));
        assert!(matches!(deserialized[3], DaemonResponse::Killed { .. }));
        assert!(matches!(deserialized[4], DaemonResponse::Error { .. }));
    }

    #[test]
    fn interleaved_request_response_pairs_in_stream() {
        // Simulate a conversation: request line followed by response line
        let req1 = DaemonRequest::List;
        let resp1 = DaemonResponse::Sessions { sessions: vec![] };
        let req2 = DaemonRequest::Spawn {
            name: Some("s".to_string()),
        };
        let resp2 = DaemonResponse::Spawned {
            session: SessionInfo {
                name: "s".to_string(),
                state: SessionState::Running,
                created_at: 1,
                last_activity: 1,
            },
        };

        let mut stream = String::new();
        stream.push_str(&to_wire(&req1));
        stream.push_str(&resp_to_wire(&resp1));
        stream.push_str(&to_wire(&req2));
        stream.push_str(&resp_to_wire(&resp2));

        let lines: Vec<&str> = stream.lines().collect();
        assert_eq!(lines.len(), 4);

        // Odd indices are requests, even are responses (0-indexed: 0,2 = req; 1,3 = resp)
        let _: DaemonRequest = serde_json::from_str(lines[0]).unwrap();
        let _: DaemonResponse = serde_json::from_str(lines[1]).unwrap();
        let _: DaemonRequest = serde_json::from_str(lines[2]).unwrap();
        let _: DaemonResponse = serde_json::from_str(lines[3]).unwrap();
    }

    #[test]
    fn empty_stream_produces_no_messages() {
        let stream = "";
        let messages: Vec<&str> = stream.lines().filter(|l| !l.is_empty()).collect();
        assert!(messages.is_empty());
    }

    #[test]
    fn stream_with_only_newlines_produces_no_valid_messages() {
        let stream = "\n\n\n";
        let messages: Vec<&str> = stream.lines().filter(|l| !l.is_empty()).collect();
        assert!(messages.is_empty());
    }

    #[test]
    fn message_with_embedded_newlines_in_payload_does_not_break_framing() {
        // A Send message whose payload contains newline characters.
        // The JSON serializer must escape them so the wire line is still one line.
        let req = DaemonRequest::Send {
            session: "s".to_string(),
            message: "first\nsecond\nthird".to_string(),
        };
        let wire = to_wire(&req);

        // The wire should be exactly one newline-terminated line
        let line_count = wire.matches('\n').count();
        assert_eq!(
            line_count, 1,
            "Wire format should have exactly one newline (the delimiter), got {}",
            line_count
        );

        let deserialized: DaemonRequest =
            serde_json::from_str(wire.trim_end_matches('\n')).unwrap();
        match deserialized {
            DaemonRequest::Send { message, .. } => {
                assert_eq!(message, "first\nsecond\nthird");
            }
            other => panic!("Expected Send, got {:?}", other),
        }
    }
}
