use anyhow::{Context, Result};
use colored::Colorize;

use crate::config::Config;
use crate::state::VpsState;

pub async fn run(session: String) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run `cloudcode up` first.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let key_path = Config::ssh_key_path()?;

    println!(
        "{} Attaching to session '{}' on {}...",
        "→".cyan(),
        session.green(),
        ip
    );
    println!(
        "{}",
        "  (Detach with Ctrl-b d, or close terminal to disconnect)".dimmed()
    );

    // Use std::process::Command (not tokio) to exec ssh with inherited stdio
    // This gives us full PTY support, resize handling, etc. for free
    let status = std::process::Command::new("ssh")
        .args([
            "-t", // force PTY allocation
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            &format!("claude@{}", ip),
            &format!("tmux attach-session -t {}", session),
        ])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to start SSH")?;

    if status.success() {
        println!("\n{} Detached from session '{}'", "✓".green(), session);
    } else {
        // Exit code 1 from tmux usually means session doesn't exist
        let code = status.code().unwrap_or(-1);
        if code == 1 {
            eprintln!(
                "{} Session '{}' not found. Use `cloudcode list` to see available sessions.",
                "Error:".red(),
                session
            );
        } else {
            eprintln!("{} SSH exited with code {}", "Error:".red(), code);
        }
    }

    Ok(())
}
