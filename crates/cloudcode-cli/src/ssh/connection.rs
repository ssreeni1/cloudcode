use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::config::Config;
use crate::state::VpsState;

/// Execute a command on the VPS via SSH
pub fn ssh_exec(state: &VpsState, _config: &Config, command: &str) -> Result<String> {
    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let key_path = Config::ssh_key_path()?;

    let output = Command::new("ssh")
        .args([
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            &format!("claude@{}", ip),
            command,
        ])
        .output()
        .context("Failed to execute SSH command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("SSH command failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
