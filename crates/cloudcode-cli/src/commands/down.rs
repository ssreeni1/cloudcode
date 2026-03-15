use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::Confirm;

use crate::config::Config;
use crate::hetzner::provisioner;
use crate::state::VpsState;

pub async fn run(force: bool) -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;

    if !state.is_provisioned() {
        bail!("No VPS is currently provisioned.");
    }

    if !force {
        let confirmed = Confirm::new()
            .with_prompt("Are you sure you want to destroy the VPS? All sessions will be lost.")
            .default(false)
            .interact()?;

        if !confirmed {
            println!("{}", "Aborted.".yellow());
            return Ok(());
        }
    }

    println!("{}", "Destroying VPS...".bold().red());

    provisioner::deprovision(&state, &config).await?;
    VpsState::clear()?;

    println!("{}", "VPS destroyed successfully.".green());

    Ok(())
}
