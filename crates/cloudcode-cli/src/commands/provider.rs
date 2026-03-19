use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

use crate::config::AiProvider;
use crate::ssh::ssh_base_args;
use crate::state::VpsState;

pub async fn run(provider: Option<String>) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` first.");
    }
    let ip = state.server_ip.as_ref().context("No server IP in state")?;

    match provider {
        None => {
            // Show current provider
            let mut args = ssh_base_args(ip)?;
            args.extend([
                format!("claude@{}", ip),
                "cat /home/claude/.cloudcode/default-provider 2>/dev/null || echo claude"
                    .to_string(),
            ]);
            let output = Command::new("ssh")
                .args(&args)
                .output()
                .context("Failed to read provider")?;
            let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let other = if current == "codex" {
                "claude"
            } else {
                "codex"
            };
            let in_tui = std::env::var("NO_COLOR").is_ok();
            let prefix = if in_tui { "/" } else { "cloudcode " };
            println!("Current provider: {}", current.green());
            println!(
                "{}",
                format!("Switch to {} with: {}provider {}", other, prefix, other).dimmed()
            );
            Ok(())
        }
        Some(name) => {
            // Validate provider name
            let _provider: AiProvider = name.parse().map_err(|_| {
                anyhow::anyhow!("Unknown provider '{}'. Use 'claude' or 'codex'.", name)
            })?;

            // Write provider file on VPS
            let mut args = ssh_base_args(ip)?;
            args.extend([
                format!("claude@{}", ip),
                format!(
                    "echo '{}' > /home/claude/.cloudcode/default-provider",
                    name.to_lowercase()
                ),
            ]);
            let status = Command::new("ssh")
                .args(&args)
                .status()
                .context("Failed to set provider")?;
            if !status.success() {
                anyhow::bail!("Failed to write provider file on VPS");
            }

            // Restart daemon to pick up the change
            let mut args = ssh_base_args(ip)?;
            args.extend([
                format!("claude@{}", ip),
                "sudo systemctl restart cloudcode-daemon".to_string(),
            ]);
            let status = Command::new("ssh")
                .args(&args)
                .status()
                .context("Failed to restart daemon")?;
            if !status.success() {
                eprintln!(
                    "{} Daemon restart failed. Provider file updated but daemon may use old value until next restart.",
                    "Warning:".yellow()
                );
            }

            println!(
                "{} Switched to {} (daemon restarted)",
                "✓".green(),
                name.to_lowercase().green()
            );
            Ok(())
        }
    }
}
