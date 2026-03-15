use cloudcode_common::session::{SessionInfo, SessionState};

mod session_state {
    use super::*;

    #[test]
    fn starting_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionState::Starting).unwrap();
        assert_eq!(json, "\"starting\"");
    }

    #[test]
    fn running_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionState::Running).unwrap();
        assert_eq!(json, "\"running\"");
    }

    #[test]
    fn idle_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionState::Idle).unwrap();
        assert_eq!(json, "\"idle\"");
    }

    #[test]
    fn dead_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionState::Dead).unwrap();
        assert_eq!(json, "\"dead\"");
    }

    #[test]
    fn starting_roundtrip() {
        let json = serde_json::to_string(&SessionState::Starting).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SessionState::Starting);
    }

    #[test]
    fn running_roundtrip() {
        let json = serde_json::to_string(&SessionState::Running).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SessionState::Running);
    }

    #[test]
    fn idle_roundtrip() {
        let json = serde_json::to_string(&SessionState::Idle).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SessionState::Idle);
    }

    #[test]
    fn dead_roundtrip() {
        let json = serde_json::to_string(&SessionState::Dead).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SessionState::Dead);
    }

    #[test]
    fn equality_same_variants() {
        assert_eq!(SessionState::Starting, SessionState::Starting);
        assert_eq!(SessionState::Running, SessionState::Running);
        assert_eq!(SessionState::Idle, SessionState::Idle);
        assert_eq!(SessionState::Dead, SessionState::Dead);
    }

    #[test]
    fn inequality_different_variants() {
        assert_ne!(SessionState::Starting, SessionState::Running);
        assert_ne!(SessionState::Running, SessionState::Idle);
        assert_ne!(SessionState::Idle, SessionState::Dead);
        assert_ne!(SessionState::Dead, SessionState::Starting);
    }

    #[test]
    fn deserialize_unknown_state_returns_error() {
        let result = serde_json::from_str::<SessionState>("\"unknown\"");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_capitalized_returns_error() {
        // Ensure "Running" (capitalized) is NOT valid -- must be "running"
        let result = serde_json::from_str::<SessionState>("\"Running\"");
        assert!(result.is_err());
    }

    #[test]
    fn copy_semantics() {
        let a = SessionState::Running;
        let b = a; // Copy
        assert_eq!(a, b); // a is still usable because SessionState is Copy
    }
}

mod session_info {
    use super::*;

    fn sample() -> SessionInfo {
        SessionInfo {
            name: "my-session".to_string(),
            state: SessionState::Running,
            created_at: 1700000000,
            last_activity: 1700000100,
        }
    }

    #[test]
    fn roundtrip() {
        let info = sample();
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "my-session");
        assert_eq!(deserialized.state, SessionState::Running);
        assert_eq!(deserialized.created_at, 1700000000);
        assert_eq!(deserialized.last_activity, 1700000100);
    }

    #[test]
    fn json_field_names() {
        let info = sample();
        let json = serde_json::to_string(&info).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Verify snake_case field names in JSON
        assert!(value.get("name").is_some());
        assert!(value.get("state").is_some());
        assert!(value.get("created_at").is_some());
        assert!(value.get("last_activity").is_some());
    }

    #[test]
    fn with_each_state_variant() {
        for state in [
            SessionState::Starting,
            SessionState::Running,
            SessionState::Idle,
            SessionState::Dead,
        ] {
            let info = SessionInfo {
                name: "s".to_string(),
                state,
                created_at: 0,
                last_activity: 0,
            };
            let json = serde_json::to_string(&info).unwrap();
            let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.state, state);
        }
    }

    #[test]
    fn with_zero_timestamps() {
        let info = SessionInfo {
            name: "s".to_string(),
            state: SessionState::Starting,
            created_at: 0,
            last_activity: 0,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.created_at, 0);
        assert_eq!(deserialized.last_activity, 0);
    }

    #[test]
    fn with_max_timestamps() {
        let info = SessionInfo {
            name: "s".to_string(),
            state: SessionState::Idle,
            created_at: u64::MAX,
            last_activity: u64::MAX,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.created_at, u64::MAX);
        assert_eq!(deserialized.last_activity, u64::MAX);
    }

    #[test]
    fn with_empty_name() {
        let info = SessionInfo {
            name: String::new(),
            state: SessionState::Dead,
            created_at: 0,
            last_activity: 0,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.name.is_empty());
    }

    #[test]
    fn deserialize_from_handwritten_json() {
        let json = r#"{
            "name": "test-sess",
            "state": "idle",
            "created_at": 12345,
            "last_activity": 67890
        }"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "test-sess");
        assert_eq!(info.state, SessionState::Idle);
        assert_eq!(info.created_at, 12345);
        assert_eq!(info.last_activity, 67890);
    }

    #[test]
    fn missing_field_returns_error() {
        // Missing "last_activity"
        let json = r#"{"name":"s","state":"running","created_at":0}"#;
        let result = serde_json::from_str::<SessionInfo>(json);
        assert!(result.is_err());
    }
}
