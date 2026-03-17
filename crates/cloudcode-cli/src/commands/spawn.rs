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

            if let Some(ref claude) = config.claude {
                if claude.auth_method == "oauth" {
                    println!(
                        "\n{}  Run {} (or {}) to complete OAuth login.",
                        "!".yellow().bold(),
                        format!("/open {}", session.name).bold(),
                        format!("cloudcode open {}", session.name).bold()
                    );
                    println!("  Highlight and copy the login URL manually (don't press 'c').");
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
