use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::{AiProvider, Config};
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run(provider: Option<String>) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` first.");
    }
    let config = Config::load()?;

    match provider {
        None => {
            // Show current provider via daemon API
            let mut client = DaemonClient::connect(&state, &config)?;
            match client.request(&DaemonRequest::GetProvider)? {
                DaemonResponse::Provider {
                    provider, has_auth, ..
                } => {
                    let other = if provider == "codex" { "claude" } else { "codex" };
                    let in_tui = std::env::var("NO_COLOR").is_ok();
                    let prefix = if in_tui { "/" } else { "cloudcode " };
                    let auth_status = if has_auth {
                        "authenticated".green().to_string()
                    } else {
                        "not authenticated".yellow().to_string()
                    };
                    println!("Current provider: {} ({})", provider.green(), auth_status);
                    println!(
                        "{}",
                        format!("Switch to {} with: {}provider {}", other, prefix, other).dimmed()
                    );
                }
                DaemonResponse::Error { message } => {
                    anyhow::bail!("Failed to get provider: {}", message);
                }
                _ => {
                    anyhow::bail!("Unexpected response from daemon");
                }
            }
            Ok(())
        }
        Some(name) => {
            // Validate provider name
            let _provider: AiProvider = name.parse().map_err(|_| {
                anyhow::anyhow!("Unknown provider '{}'. Use 'claude' or 'codex'.", name)
            })?;

            // Switch via daemon API (no restart needed)
            let mut client = DaemonClient::connect(&state, &config)?;
            match client.request(&DaemonRequest::SetProvider {
                provider: name.to_lowercase(),
            })? {
                DaemonResponse::ProviderSet { provider } => {
                    println!(
                        "{} Switched to {}. New sessions will use this provider.",
                        "✓".green(),
                        provider.green()
                    );
                }
                DaemonResponse::Error { message } => {
                    anyhow::bail!("Failed to switch provider: {}", message);
                }
                _ => {
                    anyhow::bail!("Unexpected response from daemon");
                }
            }
            Ok(())
        }
    }
}
