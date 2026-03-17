use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use std::process::Command as ProcessCommand;

use crate::config::{ClaudeConfig, Config, HetznerConfig, TelegramConfig};
use crate::hetzner::client::HetznerClient;

/// Mask a secret string, showing only the first 4 characters followed by dots.
fn mask_secret(s: &str) -> String {
    if s.len() <= 4 {
        "****".to_string()
    } else {
        format!("{}...", &s[..4])
    }
}

/// Check that required CLI tools (ssh, rsync, ssh-keygen) are available on PATH.
fn check_required_tools() -> Result<()> {
    let tools = ["ssh", "rsync", "ssh-keygen"];
    let mut missing = Vec::new();

    for tool in &tools {
        let result = ProcessCommand::new("which").arg(tool).output();

        match result {
            Ok(output) if output.status.success() => {}
            _ => missing.push(*tool),
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "Required tool(s) not found: {}. Please install them before running cloudcode init.",
            missing.join(", ")
        );
    }

    Ok(())
}

pub async fn run(auto: bool, reauth: bool) -> Result<()> {
    // Check required tools are available
    check_required_tools()?;

    println!("\n{}", "Welcome to cloudcode setup!".bold().cyan());

    if auto {
        println!(
            "  {}",
            "AI-assisted setup is not yet available. Using interactive mode.".yellow()
        );
    }

    let mut config = Config::load()?;

    // Init re-run protection: if config already has hetzner + claude and --reauth is not set, confirm overwrite
    if !reauth && config.hetzner.is_some() && config.claude.is_some() {
        print!(
            "  {} ",
            "Configuration already exists. Overwrite? [y/N]".yellow()
        );
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("  {}", "Exiting without changes.".dimmed());
            return Ok(());
        }
    }

    if reauth {
        println!("  {}", "Re-authentication mode.".yellow());
    }

    // Step 1: Hetzner setup
    if !reauth || config.hetzner.is_none() {
        println!("\n{}", "Step 1: Hetzner Cloud Setup".bold().cyan());

        let has_account = Confirm::new()
            .with_prompt("Do you have a Hetzner Cloud account?")
            .default(true)
            .interact()?;

        if !has_account {
            let url = "https://console.hetzner.cloud/";
            println!("  {}", "Opening Hetzner Cloud signup...".cyan());
            if open::that(url).is_err() {
                println!("  {} {}", "→".dimmed(), url.dimmed());
            }
            println!(
                "  {}",
                "Create an account and come back when you have an API token.".yellow()
            );
        }

        println!(
            "  {}",
            "Create a token with Read & Write access at console.hetzner.cloud → Security → API Tokens"
                .dimmed()
        );
        let api_token: String = Input::new()
            .with_prompt("Enter your Hetzner API token")
            .interact_text()?;

        println!("  Validating token...");
        let client = HetznerClient::new(api_token.clone());
        match client.validate_token().await {
            Ok(()) => {
                println!(
                    "  {} Token validated ({})",
                    "✓".green().bold(),
                    mask_secret(&api_token).dimmed()
                );
            }
            Err(e) => {
                println!(
                    "  {} {}",
                    "✗".red().bold(),
                    format!("Token validation failed: {e}").red()
                );
                return Err(e);
            }
        }

        config.hetzner = Some(HetznerConfig { api_token });
    }

    // Step 2: Claude auth
    if !reauth || config.claude.is_none() {
        println!("\n{}", "Step 2: Claude Authentication".bold().cyan());

        println!(
            "  {}",
            "API key: paste a key from console.anthropic.com (simpler)".dimmed()
        );
        println!(
            "  {}",
            "OAuth: log in via claude.ai after provisioning (no key needed now)".dimmed()
        );
        let auth_options = vec!["API Key", "OAuth (log in later on VPS)"];
        let auth_selection = Select::new()
            .with_prompt("How would you like to authenticate with Claude?")
            .items(&auth_options)
            .default(0)
            .interact()?;

        let claude_config = match auth_selection {
            0 => {
                let url = "https://console.anthropic.com/settings/keys";
                println!("  {}", "Opening Anthropic Console...".cyan());
                if open::that(url).is_err() {
                    println!("  {} {}", "→".dimmed(), url.dimmed());
                }
                let api_key: String = Input::new()
                    .with_prompt("Enter your Anthropic API key")
                    .interact_text()?;
                println!(
                    "  {} API key saved ({})",
                    "✓".green().bold(),
                    mask_secret(&api_key).dimmed()
                );
                ClaudeConfig {
                    auth_method: "api_key".to_string(),
                    api_key: Some(api_key),
                    oauth_token: None,
                }
            }
            1 => {
                println!(
                    "  {} OAuth selected. After /up (or cloudcode up), run:",
                    "✓".green().bold(),
                );
                println!(
                    "    {}",
                    "/open <session> (or cloudcode open <session>)".bold()
                );
                println!(
                    "  {}",
                    "Claude Code will prompt you to log in on first launch.".dimmed()
                );
                ClaudeConfig {
                    auth_method: "oauth".to_string(),
                    api_key: None,
                    oauth_token: None,
                }
            }
            _ => unreachable!(),
        };

        config.claude = Some(claude_config);
    }

    // Step 3: Telegram (optional)
    if !reauth {
        println!(
            "\n{}",
            "Step 3: Telegram Notifications (Optional)".bold().cyan()
        );

        let setup_telegram = Confirm::new()
            .with_prompt("Would you like to set up Telegram notifications?")
            .default(false)
            .interact()?;

        if setup_telegram {
            println!(
                "  {}",
                "Create a bot via @BotFather on Telegram: send /newbot and follow the prompts."
                    .dimmed()
            );
            let bot_token: String = Input::new()
                .with_prompt("Enter your Telegram bot token")
                .interact_text()?;
            println!(
                "  {}",
                "Send /start to @userinfobot on Telegram to find your numeric user ID.".dimmed()
            );
            let owner_id: i64 = Input::new()
                .with_prompt("Enter your Telegram user ID")
                .interact_text()?;

            println!(
                "  {} Telegram configured (bot: {})",
                "✓".green().bold(),
                mask_secret(&bot_token).dimmed()
            );

            config.telegram = Some(TelegramConfig {
                bot_token,
                owner_id,
            });

            println!("\n  {}", "How to use Telegram with cloudcode:".bold());
            println!(
                "  {}",
                "After running /up (or cloudcode up), message your bot on Telegram.".dimmed()
            );
            println!(
                "  {}",
                "Send /spawn to create a session, then type messages to interact with Claude."
                    .dimmed()
            );
            println!(
                "  {}",
                "Send /help in the bot chat for all available commands.".dimmed()
            );
        } else {
            println!("  {} Skipped", "−".dimmed());
        }
    }

    // Generate SSH keypair
    let ssh_key_path = Config::ssh_key_path()?;
    if !ssh_key_path.exists() {
        println!("\n{}", "Generating SSH keypair...".bold().cyan());
        let config_dir = Config::dir()?;
        std::fs::create_dir_all(&config_dir)?;

        let status = ProcessCommand::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-f",
                ssh_key_path.to_str().context("Invalid SSH key path")?,
                "-N",
                "",
                "-C",
                "cloudcode",
            ])
            .status()
            .context("Failed to run ssh-keygen")?;

        if !status.success() {
            println!("  {} ssh-keygen failed", "✗".red().bold());
            anyhow::bail!("ssh-keygen failed");
        }
        println!("  {} SSH keypair generated", "✓".green().bold());
    } else {
        println!(
            "\n  {} SSH keypair already exists, skipping.",
            "✓".green().bold()
        );
    }

    // Save config
    config.save()?;
    println!(
        "\n{} {}",
        "✓".green().bold(),
        "Configuration saved! Run /up or `cloudcode up` to provision your VPS."
            .bold()
            .green()
    );

    Ok(())
}
