mod api;
mod config;
mod session;
mod telegram;

use anyhow::{Context, Result};
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

    let session_mgr = Arc::new(SessionManager::new());

    // Periodic session health check
    let health_mgr = SessionManager::new();
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

        // Spawn question poller
        tokio::spawn(async move {
            question_poller::run_poller(poller_mgr, poller_bot, owner_id, poller_states).await;
        });

        // Run telegram bot (pass bot instance and question states)
        tokio::spawn(async move {
            telegram::bot::run_with_bot(bot, &tg, mgr, states).await;
        });
    }

    api::server::run(&config.listen_addr, config.listen_port, session_mgr).await
}
