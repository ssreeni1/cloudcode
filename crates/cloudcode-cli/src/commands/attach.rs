use anyhow::{Context, Result};
use colored::Colorize;
use crate::state::VpsState;
use crate::ssh::ssh_base_args;

pub async fn run(session: String) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run `cloudcode up` first.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;

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

    let mut args = ssh_base_args(ip)?;
    args.extend([
        "-t".to_string(), // force PTY allocation
        format!("claude@{}", ip),
        format!("tmux attach-session -t {}", session),
    ]);

    let status = std::process::Command::new("ssh")
        .args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to start SSH")?;

    if status.success() {
        println!("\n{} Detached from session '{}'", "✓".green(), session);
    } else {
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
