use anyhow::{Context, Result, bail};
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use std::io::Read;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::daemon_socket_path;
use crate::config::Config;
use crate::ssh::ssh_base_args;
use crate::state::VpsState;

pub struct DaemonClient {
    tunnel: Option<Child>,
    socket_path: PathBuf,
    server_ip: String,
}

impl DaemonClient {
    /// Open SSH tunnel to daemon and return a client
    pub fn connect(state: &VpsState, _config: &Config) -> Result<Self> {
        let ip = state.server_ip.as_ref().context("No server IP")?;
        let server_id = state.server_id.context("No server ID")?;
        let socket_path = daemon_socket_path(server_id)?;

        let mut client = Self {
            tunnel: None,
            socket_path,
            server_ip: ip.clone(),
        };

        client.spawn_tunnel()?;
        client.wait_for_ready()?;

        Ok(client)
    }

    /// Poll until the Unix socket is connectable
    fn wait_for_ready(&mut self) -> Result<()> {
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let interval = Duration::from_millis(100);

        loop {
            if UnixStream::connect(&self.socket_path).is_ok() {
                return Ok(());
            }

            if let Some(ref mut child) = self.tunnel {
                if let Ok(Some(status)) = child.try_wait() {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = pipe.read_to_string(&mut stderr);
                    }
                    let stderr = stderr.trim();
                    if stderr.is_empty() {
                        bail!("SSH tunnel exited early with status {}", status);
                    } else {
                        bail!("SSH tunnel exited early: {}", stderr);
                    }
                }
            }

            if start.elapsed() > timeout {
                bail!(
                    "SSH tunnel failed to establish within {}s. Is the VPS reachable?",
                    timeout.as_secs()
                );
            }

            std::thread::sleep(interval);
        }
    }

    /// Send a request to the daemon with retry logic
    pub fn request(&mut self, req: &DaemonRequest) -> Result<DaemonResponse> {
        let delays = [0, 500, 1500]; // ms delays before each attempt
        let mut last_error = None;

        for (attempt, delay_ms) in delays.iter().enumerate() {
            if *delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(*delay_ms));
            }

            // Check tunnel is alive before attempting
            if attempt > 0 {
                if let Err(e) = self.ensure_tunnel_alive() {
                    last_error = Some(e);
                    continue;
                }
            }

            match self.try_request(req) {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    let err_str = e.to_string();
                    // Only retry on transport errors, not application errors
                    if err_str.contains("Connection refused")
                        || err_str.contains("Connection reset")
                        || err_str.contains("Broken pipe")
                        || err_str.contains("connect to daemon")
                    {
                        last_error = Some(e);
                        continue;
                    }
                    // Non-retryable error
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts failed")))
    }

    fn try_request(&self, req: &DaemonRequest) -> Result<DaemonResponse> {
        let mut stream = UnixStream::connect(&self.socket_path)
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

    fn ensure_tunnel_alive(&mut self) -> Result<()> {
        // Check if we can connect to the socket
        if UnixStream::connect(&self.socket_path).is_ok() {
            return Ok(());
        }

        // Tunnel might be dead, try to restart
        if let Some(ref mut child) = self.tunnel {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process exited, need to restart
                }
                Ok(None) => {
                    // Process still running but socket not connectable
                    // Give it a moment
                    std::thread::sleep(Duration::from_millis(500));
                    if UnixStream::connect(&self.socket_path).is_ok() {
                        return Ok(());
                    }
                }
                Err(_) => {}
            }
        }

        // Clean up and re-establish tunnel
        self.cleanup_tunnel();

        eprintln!("Reconnecting SSH tunnel to {}...", self.server_ip);
        match self.spawn_tunnel() {
            Ok(()) => {
                self.wait_for_ready()?;
                Ok(())
            }
            Err(e) => {
                bail!(
                    "SSH tunnel died and reconnect failed: {}. Try running the command again.",
                    e
                );
            }
        }
    }

    /// Spawn a new SSH tunnel process using stored connection params.
    fn spawn_tunnel(&mut self) -> Result<()> {
        let ip = &self.server_ip;
        let socket_path = &self.socket_path;

        let _ = std::fs::remove_file(socket_path);

        let base_args = ssh_base_args(ip)?;
        let mut args: Vec<String> = Vec::new();
        let mut skip_next = false;
        for (i, arg) in base_args.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "-o" {
                if let Some(next) = base_args.get(i + 1) {
                    if next.starts_with("ControlMaster=")
                        || next.starts_with("ControlPath=")
                        || next.starts_with("ControlPersist=")
                    {
                        skip_next = true;
                        continue;
                    }
                }
            }
            args.push(arg.clone());
        }
        args.extend([
            "-o".to_string(),
            "ControlMaster=no".to_string(),
            "-o".to_string(),
            "ControlPath=none".to_string(),
            "-o".to_string(),
            "ControlPersist=no".to_string(),
            "-N".to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-o".to_string(),
            "StreamLocalBindUnlink=yes".to_string(),
            "-L".to_string(),
            format!("{}:127.0.0.1:7700", socket_path.display()),
            format!("claude@{}", ip),
        ]);

        let tunnel = Command::new("ssh")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start SSH tunnel")?;

        self.tunnel = Some(tunnel);
        Ok(())
    }

    fn cleanup_tunnel(&mut self) {
        if let Some(ref mut child) = self.tunnel {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.tunnel = None;
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for DaemonClient {
    fn drop(&mut self) {
        // With ControlMaster, we don't necessarily kill the master.
        // But we should clean up the forwarding socket and our tunnel process.
        // The ControlMaster will persist for ControlPersist=300 seconds.
        if let Some(ref mut child) = self.tunnel {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.socket_path);
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
        // files field is optional (serde default) so old JSON without it still works
        let json_line = r#"{"type":"send_result","output":"done"}"#;
        let resp: DaemonResponse = serde_json::from_str(json_line).unwrap();
        match resp {
            DaemonResponse::SendResult { output, files } => {
                assert_eq!(output, "done");
                assert!(files.is_empty());
            }
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
                ..
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

    #[test]
    fn daemon_socket_path_is_unique_per_server() {
        let path1 = super::daemon_socket_path(123).unwrap();
        let path2 = super::daemon_socket_path(456).unwrap();
        assert_ne!(path1, path2);
        assert!(path1.to_string_lossy().contains("123"));
        assert!(path2.to_string_lossy().contains("456"));
    }
}
