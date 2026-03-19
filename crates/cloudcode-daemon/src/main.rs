mod api;
mod config;
mod session;
mod telegram;

use anyhow::{Context, Result};
use cloudcode_common::provider::AiProvider;
use std::path::PathBuf;
use std::sync::Arc;

use session::manager::SessionManager;
use telegram::question_poller;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/cloudcode/daemon.toml"));

    let config = if config_path.exists() {
        config::DaemonConfig::load(&config_path)
            .with_context(|| format!("Failed to load config from {}", config_path.display()))?
    } else {
        eprintln!(
            "Config file not found at {}, using defaults",
            config_path.display()
        );
        config::DaemonConfig::default()
    };

    eprintln!("cloudcode-daemon starting...");
    api::handlers::init_start_time();

    // Read default provider from file, fall back to Claude
    let provider_path = std::path::Path::new("/home/claude/.cloudcode/default-provider");
    let provider = std::fs::read_to_string(provider_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(AiProvider::Claude);
    let session_mgr = Arc::new(SessionManager::new(provider));

    // Periodic session health check
    let health_mgr = Arc::clone(&session_mgr);
    tokio::spawn(async move {
        let monitor = session::monitor::SessionMonitor::new(health_mgr);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Ok(cleaned) = monitor.cleanup_dead().await {
                for name in &cleaned {
                    log::info!("Cleaned up dead session: {}", name);
                }
            }
        }
    });

    // Create shared question states for automatic question forwarding
    let question_states = question_poller::new_question_states();

    if let Some(ref tg_config) = config.telegram {
        let mgr = Arc::clone(&session_mgr);
        let tg = tg_config.clone();
        let states = question_states.clone();

        // Create bot and clone for poller before passing to dispatcher
        let bot = teloxide::Bot::new(&tg.bot_token);
        let poller_bot = bot.clone();
        let poller_mgr = Arc::clone(&session_mgr);
        let poller_states = question_states.clone();
        let owner_id = teloxide::types::ChatId(tg.owner_id);

        // Spawn question poller (delayed to let network stabilize)
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            question_poller::run_poller(poller_mgr, poller_bot, owner_id, poller_states).await;
        });

        // Run telegram bot with retry — DNS may not be ready on fresh VPS.
        // teloxide panics on network errors during init, so we catch panics.
        tokio::spawn(async move {
            for attempt in 1..=5 {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                log::info!("Starting Telegram bot (attempt {}/5)...", attempt);
                let bot_c = bot.clone();
                let tg_c = tg.clone();
                let mgr_c = mgr.clone();
                let states_c = states.clone();
                let result = tokio::spawn(async move {
                    telegram::bot::run_with_bot(bot_c, &tg_c, mgr_c, states_c).await;
                })
                .await;
                match result {
                    Ok(()) => log::warn!("Telegram bot exited cleanly"),
                    Err(e) => log::warn!("Telegram bot panicked: {}", e),
                }
                log::info!("Retrying Telegram bot in 10s...");
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
            log::error!("Telegram bot failed after 5 attempts");
        });
    }

    api::server::run(&config.listen_addr, config.listen_port, session_mgr).await
}
