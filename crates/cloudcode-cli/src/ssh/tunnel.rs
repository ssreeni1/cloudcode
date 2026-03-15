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
