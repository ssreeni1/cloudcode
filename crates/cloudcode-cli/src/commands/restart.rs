use anyhow::{Context, Result};
use colored::Colorize;

use crate::ssh::ssh_base_args;
use crate::state::VpsState;

pub async fn run() -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;

    println!("{} Restarting cloudcode-daemon on {}...", "→".cyan(), ip);

    // Restart the daemon via SSH
    let mut args = ssh_base_args(ip)?;
    args.extend([
        format!("claude@{}", ip),
        "sudo systemctl restart cloudcode-daemon".to_string(),
    ]);

    let status = std::process::Command::new("ssh")
        .args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to start SSH")?;

    if !status.success() {
        eprintln!(
            "{} Failed to restart daemon (SSH exited with code {})",
            "✗".red().bold(),
            status.code().unwrap_or(-1)
        );
        return Ok(());
    }

    // Wait 2 seconds for the daemon to start
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Check if the daemon is active
    let mut check_args = ssh_base_args(ip)?;
    check_args.extend([
        format!("claude@{}", ip),
        "sudo systemctl is-active cloudcode-daemon".to_string(),
    ]);

    let output = std::process::Command::new("ssh")
        .args(&check_args)
        .output()
        .context("Failed to check daemon status")?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if output.status.success() && stdout == "active" {
        println!(
            "{} Daemon restarted successfully (status: {})",
            "✓".green().bold(),
            "active".green()
        );
    } else {
        eprintln!(
            "{} Daemon may not be running (status: {})",
            "✗".red().bold(),
            stdout.red()
        );
    }

    Ok(())
}
