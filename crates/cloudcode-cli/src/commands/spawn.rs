use std::io::{IsTerminal, Write};

use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::Config;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run(name: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    // If no name provided and running in a TTY, prompt the user
    let name = if name.is_none() && std::io::stdout().is_terminal() {
        print!("Session name (leave empty for auto-generated): ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        name
    };

    let mut client = DaemonClient::connect(&state, &config)?;
    let response = client.request(&DaemonRequest::Spawn { name })?;

    match response {
        DaemonResponse::Spawned { session } => {
            println!("{} Session '{}' created", "✓".green(), session.name);
            println!(
                "  Open with: /open {} (or cloudcode open {})",
                session.name, session.name
            );

            let claude_needs_oauth = config.claude.as_ref().is_some_and(|c| c.uses_oauth());
            let codex_needs_oauth = config
                .codex
                .as_ref()
                .is_some_and(|c| matches!(c.auth_method, crate::config::AuthMethod::Oauth));

            if claude_needs_oauth || codex_needs_oauth {
                println!(
                    "\n{}  Run {} (or {}) to complete login.",
                    "!".yellow().bold(),
                    format!("/open {}", session.name).bold(),
                    format!("cloudcode open {}", session.name).bold()
                );
                if claude_needs_oauth {
                    println!("  Claude: copy the login URL manually (don't press 'c').");
                }
                if codex_needs_oauth {
                    println!(
                        "  Codex: select 'Device code' when prompted, then visit the URL in your browser."
                    );
                }
            }
        }
        DaemonResponse::Error { message } => {
            eprintln!("{} {}", "Error:".red(), message);
        }
        _ => eprintln!("Unexpected response from daemon"),
    }
    Ok(())
}
