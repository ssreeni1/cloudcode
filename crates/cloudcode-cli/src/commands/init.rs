use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use std::process::Command as ProcessCommand;

use crate::config::{ClaudeConfig, Config, HetznerConfig, TelegramConfig};
use crate::hetzner::client::HetznerClient;

pub async fn run(auto: bool, reauth: bool) -> Result<()> {
    println!(
        "{}",
        "Welcome to cloudcode setup!".bold().cyan()
    );

    if auto {
        println!(
            "{}",
            "AI-assisted setup is not yet available. Using interactive mode."
                .yellow()
        );
    }

    let mut config = Config::load()?;

    if reauth {
        println!("{}", "Re-authentication mode.".yellow());
    }

    // Step 1: Hetzner setup
    if !reauth || config.hetzner.is_none() {
        println!("\n{}", "Step 1: Hetzner Cloud Setup".bold());

        let has_account = Confirm::new()
            .with_prompt("Do you have a Hetzner Cloud account?")
            .default(true)
            .interact()?;

        if !has_account {
            println!("Opening Hetzner Cloud signup...");
            let _ = open::that("https://console.hetzner.cloud/");
            println!(
                "{}",
                "Create an account and come back when you have an API token."
                    .yellow()
            );
        }

        let api_token: String = Input::new()
            .with_prompt("Enter your Hetzner API token")
            .interact_text()?;

        // Validate token
        println!("Validating token...");
        let client = HetznerClient::new(api_token.clone());
        client.validate_token().await?;
        println!("{}", "Token validated successfully!".green());

        config.hetzner = Some(HetznerConfig { api_token });
    }

    // Step 2: Claude auth
    if !reauth || config.claude.is_none() {
        println!("\n{}", "Step 2: Claude Authentication".bold());

        let auth_options = vec!["API Key", "OAuth Token"];
        let auth_selection = Select::new()
            .with_prompt("How would you like to authenticate with Claude?")
            .items(&auth_options)
            .default(0)
            .interact()?;

        let claude_config = match auth_selection {
            0 => {
                let api_key: String = Input::new()
                    .with_prompt("Enter your Anthropic API key")
                    .interact_text()?;
                ClaudeConfig {
                    auth_method: "api_key".to_string(),
                    api_key: Some(api_key),
                    oauth_token: None,
                }
            }
            1 => {
                let oauth_token: String = Input::new()
                    .with_prompt("Enter your OAuth token")
                    .interact_text()?;
                ClaudeConfig {
                    auth_method: "oauth".to_string(),
                    api_key: None,
                    oauth_token: Some(oauth_token),
                }
            }
            _ => unreachable!(),
        };

        config.claude = Some(claude_config);
    }

    // Step 3: Telegram (optional)
    if !reauth {
        println!("\n{}", "Step 3: Telegram Notifications (Optional)".bold());

        let setup_telegram = Confirm::new()
            .with_prompt("Would you like to set up Telegram notifications?")
            .default(false)
            .interact()?;

        if setup_telegram {
            let bot_token: String = Input::new()
                .with_prompt("Enter your Telegram bot token")
                .interact_text()?;
            let owner_id: i64 = Input::new()
                .with_prompt("Enter your Telegram user ID")
                .interact_text()?;

            config.telegram = Some(TelegramConfig {
                bot_token,
                owner_id,
            });
        }
    }

    // Generate SSH keypair
    let ssh_key_path = Config::ssh_key_path()?;
    if !ssh_key_path.exists() {
        println!("\n{}", "Generating SSH keypair...".bold());
        let config_dir = Config::dir()?;
        std::fs::create_dir_all(&config_dir)?;

        let status = ProcessCommand::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-f",
                ssh_key_path
                    .to_str()
                    .context("Invalid SSH key path")?,
                "-N",
                "",
                "-C",
                "cloudcode",
            ])
            .status()
            .context("Failed to run ssh-keygen")?;

        if !status.success() {
            anyhow::bail!("ssh-keygen failed");
        }
        println!("{}", "SSH keypair generated.".green());
    } else {
        println!("\n{}", "SSH keypair already exists, skipping.".yellow());
    }

    // Save config
    config.save()?;
    println!(
        "\n{}",
        "Configuration saved! Run `cloudcode up` to provision your VPS."
            .bold()
            .green()
    );

    Ok(())
}
