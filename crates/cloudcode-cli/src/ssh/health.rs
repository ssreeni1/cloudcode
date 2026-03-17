use anyhow::{Context, Result};
use std::time::Duration;

use crate::ssh::ssh_base_args;
use crate::state::VpsState;

#[derive(Debug)]
pub enum CloudInitStatus {
    Ready,
    Failed { error: String },
}

/// Poll VPS for cloud-init completion
pub async fn wait_for_cloud_init(state: &VpsState, timeout: Duration) -> Result<CloudInitStatus> {
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Ok(CloudInitStatus::Failed {
                error: "Timed out waiting for cloud-init".to_string(),
            });
        }

        // Try reading our status marker first
        let ip = state.server_ip.as_ref().context("No server IP")?;
        let mut args = ssh_base_args(ip)?;
        args.extend([
            "-o".to_string(),
            "ConnectTimeout=5".to_string(),
            format!("claude@{}", ip),
            "cat /home/claude/.cloudcode-status.json 2>/dev/null || cloud-init status 2>/dev/null || echo unknown".to_string(),
        ]);
        let result = std::process::Command::new("ssh").args(&args).output();

        if let Ok(output) = result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let text = stdout.trim();

                if text.contains("\"status\":\"ready\"") || text.contains("\"status\": \"ready\"") {
                    return Ok(CloudInitStatus::Ready);
                }
                if text.contains("\"status\":\"error\"") || text.contains("\"status\": \"error\"") {
                    let error = text.to_string();
                    return Ok(CloudInitStatus::Failed { error });
                }
                if text.contains("status: done") {
                    // cloud-init done but our marker missing = our script failed
                    return Ok(CloudInitStatus::Failed {
                        error: "cloud-init completed but cloudcode setup marker not found"
                            .to_string(),
                    });
                }
                // Still running, continue polling
            }
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Verify expected software is installed
pub async fn verify_installation(state: &VpsState) -> Result<Vec<(String, bool)>> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let mut args = ssh_base_args(ip)?;
    args.extend([
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        format!("claude@{}", ip),
        "export PATH=\"$HOME/.local/bin:$PATH\"; echo tmux:$(which tmux >/dev/null 2>&1 && echo ok || echo missing); echo claude:$(which claude >/dev/null 2>&1 && echo ok || echo missing)".to_string(),
    ]);

    let output = std::process::Command::new("ssh")
        .args(&args)
        .output()
        .context("Failed to run verification")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        if let Some((name, status)) = line.split_once(':') {
            results.push((name.trim().to_string(), status.trim() == "ok"));
        }
    }

    Ok(results)
}
