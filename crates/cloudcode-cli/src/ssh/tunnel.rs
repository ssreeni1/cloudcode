use anyhow::{Context, Result};
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::config::Config;
use crate::state::VpsState;

pub struct DaemonClient {
    tunnel: Child,
    local_port: u16,
}

impl DaemonClient {
    /// Open SSH tunnel to daemon and return a client
    pub fn connect(state: &VpsState, _config: &Config) -> Result<Self> {
        let ip = state.server_ip.as_ref().context("No server IP")?;
        let key_path = Config::ssh_key_path()?;
        let local_port = 17700; // local forwarding port

        let tunnel = Command::new("ssh")
            .args([
                "-i",
                &key_path.to_string_lossy(),
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "LogLevel=ERROR",
                "-N", // no remote command
                "-L",
                &format!("{}:127.0.0.1:7700", local_port),
                &format!("claude@{}", ip),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start SSH tunnel")?;

        // Give tunnel time to establish
        std::thread::sleep(Duration::from_secs(2));

        Ok(Self { tunnel, local_port })
    }

    /// Send a request to the daemon and get a response
    pub fn request(&self, req: &DaemonRequest) -> Result<DaemonResponse> {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.local_port))
            .context("Failed to connect to daemon via tunnel")?;
        stream.set_read_timeout(Some(Duration::from_secs(180)))?;

        let mut json = serde_json::to_string(req)?;
        json.push('\n');
        stream.write_all(json.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        serde_json::from_str(&line).context("Failed to parse daemon response")
    }
}

impl Drop for DaemonClient {
    fn drop(&mut self) {
        let _ = self.tunnel.kill();
    }
}

#[cfg(test)]
mod tests {
    use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
    use cloudcode_common::session::SessionState;

    // -----------------------------------------------------------------------
    // DaemonRequest serialization produces valid newline-delimited JSON
    // -----------------------------------------------------------------------

    #[test]
    fn request_serializes_to_single_line_json() {
        let requests: Vec<DaemonRequest> = vec![
            DaemonRequest::Spawn {
                name: Some("s1".to_string()),
            },
            DaemonRequest::Spawn { name: None },
            DaemonRequest::List,
            DaemonRequest::Kill {
                session: "s1".to_string(),
            },
            DaemonRequest::Send {
                session: "s1".to_string(),
                message: "hello".to_string(),
            },
            DaemonRequest::Status,
        ];

        for req in &requests {
            let json = serde_json::to_string(req).unwrap();
            // Must not contain newlines (newline-delimited protocol requirement)
            assert!(
                !json.contains('\n'),
                "Serialized request must be a single line: {}",
                json
            );
            // Must be valid JSON that round-trips
            let _: DaemonRequest = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn request_with_newline_in_message_stays_single_json_line() {
        // Even if the message itself contains newlines, the JSON encoding
        // escapes them so the serialized form is still one line.
        let req = DaemonRequest::Send {
            session: "s".to_string(),
            message: "line1\nline2\nline3".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(
            !json.contains('\n'),
            "JSON with embedded newlines must escape them"
        );
    }

    #[test]
    fn request_appended_newline_can_be_stripped_and_deserialized() {
        let req = DaemonRequest::Kill {
            session: "sess-1".to_string(),
        };
        let mut wire = serde_json::to_string(&req).unwrap();
        wire.push('\n');

        // Simulate receiver stripping the trailing newline
        let trimmed = wire.trim_end();
        let deserialized: DaemonRequest = serde_json::from_str(trimmed).unwrap();

        match deserialized {
            DaemonRequest::Kill { session } => assert_eq!(session, "sess-1"),
            other => panic!("Expected Kill, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // DaemonResponse can be deserialized from a JSON line
    // -----------------------------------------------------------------------

    #[test]
    fn response_deserializes_from_json_line_spawned() {
        let json_line = r#"{"type":"spawned","session":{"name":"test","state":"running","created_at":1700000000,"last_activity":1700000100}}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::Spawned { session } => {
                assert_eq!(session.name, "test");
                assert_eq!(session.state, SessionState::Running);
            }
            other => panic!("Expected Spawned, got {:?}", other),
        }
    }

    #[test]
    fn response_deserializes_from_json_line_sessions() {
        let json_line = r#"{"type":"sessions","sessions":[]}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::Sessions { sessions } => assert!(sessions.is_empty()),
            other => panic!("Expected Sessions, got {:?}", other),
        }
    }

    #[test]
    fn response_deserializes_from_json_line_killed() {
        let json_line = r#"{"type":"killed","session":"s1"}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::Killed { session } => assert_eq!(session, "s1"),
            other => panic!("Expected Killed, got {:?}", other),
        }
    }

    #[test]
    fn response_deserializes_from_json_line_send_result() {
        let json_line = r#"{"type":"send_result","output":"done"}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::SendResult { output } => assert_eq!(output, "done"),
            other => panic!("Expected SendResult, got {:?}", other),
        }
    }

    #[test]
    fn response_deserializes_from_json_line_status() {
        let json_line = r#"{"type":"status","uptime_secs":120,"sessions":[]}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::Status {
                uptime_secs,
                sessions,
            } => {
                assert_eq!(uptime_secs, 120);
                assert!(sessions.is_empty());
            }
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    #[test]
    fn response_deserializes_from_json_line_error() {
        let json_line = r#"{"type":"error","message":"something failed"}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::Error { message } => assert_eq!(message, "something failed"),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn response_with_trailing_newline_can_be_deserialized() {
        let json_line = "{\"type\":\"killed\",\"session\":\"s\"}\n";
        let trimmed = json_line.trim_end();
        let resp: DaemonResponse = serde_json::from_str(trimmed).unwrap();
        assert!(matches!(resp, DaemonResponse::Killed { .. }));
    }

    #[test]
    fn response_invalid_json_returns_error() {
        let bad = "not valid json\n";
        let result = serde_json::from_str::<DaemonResponse>(bad.trim_end());
        assert!(result.is_err());
    }

    #[test]
    fn response_unknown_type_returns_error() {
        let json_line = r#"{"type":"unknown_variant"}"#;
        let result = serde_json::from_str::<DaemonResponse>(json_line);
        assert!(result.is_err());
    }
}
