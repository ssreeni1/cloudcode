use anyhow::{Context, Result, bail};
use std::process::Command;
use std::time::Duration;

use crate::ssh::ssh_base_args;
use crate::state::VpsState;

/// Wait for SSH connectivity to the VPS, retrying every 3s up to `timeout`.
pub async fn wait_for_ssh(state: &VpsState, timeout: Duration) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let start = std::time::Instant::now();

    loop {
        let mut args = ssh_base_args(ip)?;
        args.extend([
            "-o".to_string(),
            "ConnectTimeout=5".to_string(),
            format!("claude@{}", ip),
            "echo ok".to_string(),
        ]);

        let status = Command::new("ssh").args(&args).output();

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
