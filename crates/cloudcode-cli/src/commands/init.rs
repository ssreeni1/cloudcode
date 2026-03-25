use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use std::process::Command as ProcessCommand;

use crate::config::{
    AiProvider, AiProviderConfig, AuthMethod, ClaudeConfig, CloudConfig, CloudKind, CodexConfig,
    Config, HetznerConfig, TelegramConfig, TelegramMode,
};
use crate::providers;

/// Check whether a given AI provider has config set.
fn has_provider_config(config: &Config, provider: AiProvider) -> bool {
    match provider {
        AiProvider::Claude => config.claude.is_some(),
        AiProvider::Codex => config.codex.is_some(),
        AiProvider::Amp => config.amp.is_some(),
        AiProvider::OpenCode => config.opencode.is_some(),
        AiProvider::Pi => config.pi.is_some(),
        AiProvider::Cursor => config.cursor.is_some(),
    }
}

/// Mask a secret string, showing only the first 4 characters followed by dots.
fn mask_secret(s: &str) -> String {
    if s.len() <= 4 {
        "****".to_string()
    } else {
        format!("{}...", &s[..4])
    }
}

/// Check that required CLI tools (ssh, ssh-keygen) are available on PATH.
fn check_required_tools() -> Result<()> {
    let tools = ["ssh", "ssh-keygen"];
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
    check_required_tools()?;

    println!("\n{}", "Welcome to cloudcode setup!".bold().cyan());
    crate::commands::security::print_trust_summary(true);

    if auto {
        println!(
            "  {}",
            "AI-assisted setup is not yet available. Using interactive mode.".yellow()
        );
    }

    let mut config = Config::load()?;

    // Migrate v1 config if old [hetzner] exists without [cloud]
    if config.hetzner.is_some() && config.cloud.is_none() {
        println!(
            "  {}",
            "Migrating v1 config to v2 format...".yellow()
        );
        config.migrate_v1_to_v2();
        config.save_with_backup()?;
        println!(
            "  {} Config migrated (backup saved as .v1.bak)",
            "✓".green().bold()
        );
    }

    // Init re-run protection
    let has_cloud = config.effective_cloud_kind().is_some();
    let has_ai = config.claude.is_some() || config.codex.is_some();
    if !reauth && has_cloud && has_ai {
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

    // Step 1: Cloud provider selection
    if !reauth || config.effective_cloud_kind().is_none() {
        println!("\n{}", "Step 1: Cloud Provider Setup".bold().cyan());

        // Only Hetzner is available for now (DigitalOcean coming soon)
        let cloud_options = vec!["Hetzner Cloud"];
        let cloud_selection = Select::new()
            .with_prompt("Which cloud provider would you like to use?")
            .items(&cloud_options)
            .default(0)
            .interact()?;

        match cloud_selection {
            0 => {
                // Hetzner
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
                let provider = providers::cloud_provider("hetzner", api_token.clone())?;
                match provider.validate_credentials().await {
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

                config.cloud = Some(CloudConfig {
                    provider: CloudKind::Hetzner,
                    hetzner: Some(HetznerConfig {
                        api_token: api_token.clone(),
                    }),
                    digitalocean: None,
                });
                // Keep legacy field for backward compat
                config.hetzner = Some(HetznerConfig { api_token });
            }
            _ => unreachable!(),
        }
    }

    // Step 2: AI Provider selection
    let all_providers = [
        AiProvider::Claude,
        AiProvider::Codex,
        AiProvider::Amp,
        AiProvider::OpenCode,
        AiProvider::Pi,
        AiProvider::Cursor,
    ];
    let selected_providers: Vec<AiProvider> = if !reauth {
        println!("\n{}", "Step 2: AI Provider".bold().cyan());

        let provider_options: Vec<String> = all_providers
            .iter()
            .map(|p| {
                let meta = p.meta();
                let stability = if meta.stable { "" } else { " (experimental)" };
                format!("{}{}", p.display_name(), stability)
            })
            .collect();

        // Multi-select: ask which providers to enable
        let mut selected = Vec::new();
        for (i, label) in provider_options.iter().enumerate() {
            let default = matches!(all_providers[i], AiProvider::Claude);
            let enable = Confirm::new()
                .with_prompt(format!("Enable {}?", label))
                .default(default)
                .interact()?;
            if enable {
                selected.push(all_providers[i]);
            }
        }

        if selected.is_empty() {
            println!(
                "  {} {}",
                "✗".red().bold(),
                "At least one AI provider must be selected.".red()
            );
            anyhow::bail!("No AI provider selected");
        }

        // Pick default provider
        if selected.len() == 1 {
            config.default_provider = Some(selected[0]);
            println!(
                "  {} {} selected as default provider",
                "✓".green().bold(),
                selected[0].display_name()
            );
        } else {
            let default_options: Vec<&str> = selected.iter().map(|p| p.display_name()).collect();
            let default_selection = Select::new()
                .with_prompt("Which provider should be the default?")
                .items(&default_options)
                .default(0)
                .interact()?;
            config.default_provider = Some(selected[default_selection]);
            println!(
                "  {} {} providers, default: {}",
                "✓".green().bold(),
                selected.len(),
                config.default_provider.unwrap().display_name()
            );
        }

        selected
    } else {
        // In reauth mode, keep existing provider choices
        let mut selected = Vec::new();
        if config.claude.is_some() {
            selected.push(AiProvider::Claude);
        }
        if config.codex.is_some() {
            selected.push(AiProvider::Codex);
        }
        if config.amp.is_some() {
            selected.push(AiProvider::Amp);
        }
        if config.opencode.is_some() {
            selected.push(AiProvider::OpenCode);
        }
        if config.pi.is_some() {
            selected.push(AiProvider::Pi);
        }
        if config.cursor.is_some() {
            selected.push(AiProvider::Cursor);
        }
        if selected.is_empty() {
            selected.push(AiProvider::Claude);
        }
        selected
    };

    let wants_claude = selected_providers.contains(&AiProvider::Claude);
    let wants_codex = selected_providers.contains(&AiProvider::Codex);

    // Step 3: Claude auth
    if wants_claude && (!reauth || config.claude.is_none()) {
        println!("\n{}", "Step 3: Claude Authentication".bold().cyan());

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
                    auth_method: AuthMethod::ApiKey,
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
                    auth_method: AuthMethod::Oauth,
                    api_key: None,
                    oauth_token: None,
                }
            }
            _ => unreachable!(),
        };

        config.claude = Some(claude_config);
    }

    // Step 4: Codex auth
    if wants_codex && (!reauth || config.codex.is_none()) {
        println!("\n{}", "Step 4: Codex Authentication".bold().cyan());

        println!(
            "  {}",
            "API key: paste a key from platform.openai.com (simpler)".dimmed()
        );
        println!(
            "  {}",
            "Device auth: log in via openai.com after provisioning (no key needed now)".dimmed()
        );
        let auth_options = vec!["API Key", "Device Auth (log in later on VPS)"];
        let auth_selection = Select::new()
            .with_prompt("How would you like to authenticate with Codex?")
            .items(&auth_options)
            .default(0)
            .interact()?;

        let codex_config = match auth_selection {
            0 => {
                let url = "https://platform.openai.com/api-keys";
                println!("  {}", "Opening OpenAI Console...".cyan());
                if open::that(url).is_err() {
                    println!("  {} {}", "→".dimmed(), url.dimmed());
                }
                let api_key: String = Input::new()
                    .with_prompt("Enter your OpenAI API key")
                    .interact_text()?;
                println!(
                    "  {} API key saved ({})",
                    "✓".green().bold(),
                    mask_secret(&api_key).dimmed()
                );
                CodexConfig {
                    auth_method: AuthMethod::ApiKey,
                    api_key: Some(api_key),
                }
            }
            1 => {
                println!(
                    "  {} Device auth selected. After /up, run: /spawn then /open <session>",
                    "✓".green().bold(),
                );
                println!(
                    "  {}",
                    "Codex will use device-code auth (works on VPS — no localhost needed)."
                        .dimmed()
                );
                println!(
                    "  {}",
                    "You'll see a code + URL to visit in your browser to authorize.".dimmed()
                );
                CodexConfig {
                    auth_method: AuthMethod::Oauth,
                    api_key: None,
                }
            }
            _ => unreachable!(),
        };

        config.codex = Some(codex_config);
    }

    // Step 5: Auth for additional providers (Amp, OpenCode, Pi, Cursor)
    // Detect which existing credentials can be proxied
    let has_anthropic_auth = config
        .claude
        .as_ref()
        .map_or(false, |c| c.api_key.is_some() || c.auth_method == AuthMethod::Oauth);
    let has_openai_auth = config
        .codex
        .as_ref()
        .map_or(false, |c| c.api_key.is_some() || c.auth_method == AuthMethod::Oauth);

    for &provider in &[AiProvider::Amp, AiProvider::OpenCode, AiProvider::Pi, AiProvider::Cursor] {
        if !selected_providers.contains(&provider) {
            continue;
        }
        let step_label = format!("Step 5: {} Authentication", provider.display_name());
        if !reauth || !has_provider_config(&config, provider) {
            println!("\n{}", step_label.bold().cyan());

            // Determine which existing auth this provider can use
            let meta = provider.meta();
            let can_use_anthropic = meta.auth_env_vars.contains(&"ANTHROPIC_API_KEY") && has_anthropic_auth;
            let can_use_openai = meta.auth_env_vars.contains(&"OPENAI_API_KEY") && has_openai_auth;
            let can_proxy = can_use_anthropic || can_use_openai;

            let proxy_source = if can_use_anthropic {
                "Claude"
            } else if can_use_openai {
                "Codex"
            } else {
                ""
            };

            let mut auth_options = Vec::new();
            if can_proxy {
                auth_options.push(format!("Use {} credentials (recommended)", proxy_source));
            }
            auth_options.push("API Key".to_string());
            auth_options.push("Skip (configure later)".to_string());

            let auth_refs: Vec<&str> = auth_options.iter().map(|s| s.as_str()).collect();
            let auth_selection = Select::new()
                .with_prompt(format!("How would you like to authenticate with {}?", provider.display_name()))
                .items(&auth_refs)
                .default(0)
                .interact()?;

            let chosen = &auth_options[auth_selection];
            let provider_config = if chosen.starts_with("Use ") {
                // Proxy from existing credentials
                println!(
                    "  {} {} will use {} credentials automatically on the VPS",
                    "✓".green().bold(),
                    provider.display_name(),
                    proxy_source
                );
                // Create a config entry with oauth method (signals "use proxy, no separate key")
                Some(AiProviderConfig {
                    auth_method: AuthMethod::Oauth,
                    api_key: None,
                })
            } else if chosen == "API Key" {
                let api_key: String = Input::new()
                    .with_prompt(format!("Enter your {} API key", provider.display_name()))
                    .interact_text()?;
                println!(
                    "  {} API key saved ({})",
                    "✓".green().bold(),
                    mask_secret(&api_key).dimmed()
                );
                Some(AiProviderConfig {
                    auth_method: AuthMethod::ApiKey,
                    api_key: Some(api_key),
                })
            } else {
                println!("  {} Skipped", "−".dimmed());
                None
            };

            match provider {
                AiProvider::Amp => config.amp = provider_config,
                AiProvider::OpenCode => config.opencode = provider_config,
                AiProvider::Pi => config.pi = provider_config,
                AiProvider::Cursor => config.cursor = provider_config,
                _ => {}
            }
        }
    }

    // Step 6: Telegram (optional)
    if !reauth {
        println!(
            "\n{}",
            "Step 6: Telegram Notifications (Optional)".bold().cyan()
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

            // Telegram mode selection
            let mode_options = vec![
                "Legacy (teloxide bot — default, most compatible)",
                "Channels (Claude Code Channel + reqwest — requires Claude CLI >= 2.1.80)",
                "Auto (try channels, fall back to legacy)",
            ];
            let mode_selection = Select::new()
                .with_prompt("Which Telegram transport mode?")
                .items(&mode_options)
                .default(0)
                .interact()?;

            let mode = match mode_selection {
                0 => TelegramMode::Legacy,
                1 => TelegramMode::Channels,
                2 => TelegramMode::Auto,
                _ => unreachable!(),
            };

            println!(
                "  {} Telegram configured (bot: {}, mode: {})",
                "✓".green().bold(),
                mask_secret(&bot_token).dimmed(),
                mode
            );

            config.telegram = Some(TelegramConfig {
                bot_token,
                owner_id,
                mode,
            });

            println!("\n  {}", "How to use Telegram with cloudcode:".bold());
            println!(
                "  {}",
                "After running /up (or cloudcode up), message your bot on Telegram.".dimmed()
            );
            println!(
                "  {}",
                "Send /spawn to create a session, then type messages to interact.".dimmed()
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
        Config::ensure_dir()?;

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
