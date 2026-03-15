use anyhow::{bail, Result};
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

    if !state.is_provisioned() {
        bail!("No VPS is currently provisioned.");
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

    provisioner::deprovision(&state, &config).await?;
    VpsState::clear()?;

    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!("  {} {{msg}}", "✓".green()))
            .expect("invalid template"),
    );
    pb.finish_with_message(format!("{}", "VPS destroyed successfully.".green()));

    Ok(())
}
