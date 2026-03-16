use anyhow::{Context, Result};
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;
use crate::config::Config;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;
use crate::ssh::ssh_base_args;

pub async fn run(session: String) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run `cloudcode up` first.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;

    // Pre-attach: check if the session exists via daemon query
    if let Ok(config) = Config::load() {
        if let Ok(mut client) = DaemonClient::connect(&state, &config) {
            if let Ok(DaemonResponse::Sessions { sessions }) =
                client.request(&DaemonRequest::List)
            {
                let exists = sessions.iter().any(|s| s.name == session);
                if !exists {
                    eprintln!(
                        "{} Session '{}' not found.",
                        "Error:".red(),
                        session
                    );
                    if sessions.is_empty() {
                        eprintln!("No active sessions. Create one with `cloudcode spawn`.");
                    } else {
                        eprintln!("Available sessions:");
                        for s in &sessions {
                            eprintln!(
                                "  {} [{}]",
                                s.name.green(),
                                format!("{:?}", s.state).yellow()
                            );
                        }
                    }
                    return Ok(());
                }
            }
            // If the daemon query failed, fall through to attempt attach anyway
        }
    }

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

    let mut cmd = std::process::Command::new("ssh");
    cmd.args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    // If the local TERM isn't widely supported (e.g. xterm-ghostty),
    // override to xterm-256color so tmux works on the remote.
    if let Ok(term) = std::env::var("TERM") {
        if !term.starts_with("xterm-256") && !term.starts_with("screen") && !term.starts_with("tmux") {
            cmd.env("TERM", "xterm-256color");
        }
    }

    let status = cmd.status().context("Failed to start SSH")?;

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
