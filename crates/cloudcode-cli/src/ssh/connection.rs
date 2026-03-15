use anyhow::{bail, Context, Result};
use std::process::Command;
use std::time::Duration;

use crate::state::VpsState;

/// Execute a command on the VPS via SSH
pub fn ssh_exec(state: &VpsState, command: &str) -> Result<String> {
    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let mut args = super::ssh_base_args(ip)?;
    args.push(format!("claude@{}", ip));
    args.push(command.to_string());

    let output = Command::new("ssh")
        .args(&args)
        .output()
        .context("Failed to execute SSH command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("SSH command failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Wait for SSH connectivity to the VPS, retrying every 3s up to `timeout`.
pub async fn wait_for_ssh(state: &VpsState, timeout: Duration) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let key_path = crate::config::Config::ssh_key_path()?;
    let start = std::time::Instant::now();

    loop {
        let status = Command::new("ssh")
            .args([
                "-i",
                &key_path.to_string_lossy(),
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "ConnectTimeout=5",
                &format!("claude@{}", ip),
                "echo ok",
            ])
            .output();

        if let Ok(output) = status {
            if output.status.success() {
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            bail!("Timed out waiting for SSH connectivity after {:?}", timeout);
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
