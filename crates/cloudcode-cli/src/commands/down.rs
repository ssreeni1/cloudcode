use anyhow::{Result, bail};
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::config::Config;
use crate::hetzner::provisioner;
use crate::state::VpsState;

pub async fn run(force: bool) -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;

    if !state.is_provisioned() && state.ssh_key_id.is_none() {
        bail!("No VPS is currently provisioned.");
    }

    // Handle partial state (SSH key created but server creation failed)
    if !state.is_provisioned() && state.ssh_key_id.is_some() {
        println!(
            "  {} {}",
            "!".yellow().bold(),
            "No server found, cleaning up orphaned SSH key...".yellow()
        );

        match provisioner::deprovision(&state, &config).await {
            Ok(()) => {
                VpsState::clear()?;
                println!(
                    "  {} {}",
                    "✓".green(),
                    "SSH key cleaned up successfully.".green()
                );
            }
            Err(e) => {
                let err_msg = format!("{e}");
                if err_msg.contains("404") {
                    VpsState::clear()?;
                    println!(
                        "  {} {}",
                        "✓".green(),
                        "SSH key already removed. Local state cleared.".green()
                    );
                } else {
                    return Err(e);
                }
            }
        }
        return Ok(());
    }

    let server_id = state.server_id.unwrap();
    let server_ip = state.server_ip.as_deref().unwrap_or("unknown");

    if !force {
        println!();
        println!(
            "  {} {}",
            "WARNING:".bold().red(),
            "This will permanently destroy your VPS.".red()
        );
        println!(
            "  Server ID: {}  IP: {}",
            format!("{server_id}").bold(),
            server_ip.bold()
        );
        println!(
            "  {}",
            "All sessions and data on the server will be lost.".red()
        );
        println!();

        let confirmed = Confirm::new()
            .with_prompt("Are you sure you want to destroy the VPS?")
            .default(false)
            .interact()?;

        if !confirmed {
            println!("  {}", "Aborted.".yellow());
            return Ok(());
        }
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.red} {msg}")
            .expect("invalid template"),
    );
    pb.set_message("Destroying VPS...");
    pb.enable_steady_tick(Duration::from_millis(80));

    match provisioner::deprovision(&state, &config).await {
        Ok(()) => {
            VpsState::clear()?;
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!("  {} {{msg}}", "✓".green()))
                    .expect("invalid template"),
            );
            pb.finish_with_message(format!("{}", "VPS destroyed successfully.".green()));
        }
        Err(e) => {
            let err_msg = format!("{e}");
            // Check for 404 (server not found on Hetzner)
            if err_msg.contains("HTTP 404") || err_msg.contains("404") {
                pb.finish_and_clear();
                println!(
                    "\n  {} {}",
                    "!".yellow().bold(),
                    "Server not found on Hetzner.".yellow()
                );
                println!("  Clear local state? [y/N]");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let trimmed = input.trim();
                if trimmed.eq_ignore_ascii_case("y") {
                    VpsState::clear()?;
                    println!("  {} {}", "✓".green(), "Local state cleared.".green());
                } else {
                    println!("  {}", "Aborted. Local state preserved.".yellow());
                }
            } else {
                pb.finish_and_clear();
                return Err(e);
            }
        }
    }

    Ok(())
}
