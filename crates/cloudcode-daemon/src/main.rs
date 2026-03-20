mod api;
mod config;
mod session;
mod telegram;

use anyhow::{Context, Result};
use cloudcode_common::provider::AiProvider;
use std::path::PathBuf;
use std::sync::Arc;

use config::TelegramMode;
use session::manager::SessionManager;
use telegram::default_session::DefaultSessionStore;
use telegram::dispatch::DaemonState;
use telegram::question_poller;
use telegram::sender::{ReqwestSender, TeloxideSender, TelegramSender};

/// Check prerequisites for channels mode.
fn channels_prerequisites_met() -> bool {
    // 1. claude binary exists, version >= 2.1.80
    let claude_ok = std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()
        .map(|o| {
            let version = String::from_utf8_lossy(&o.stdout);
            // Accept any version that contains a version string (basic check)
            o.status.success() && !version.trim().is_empty()
        })
        .unwrap_or(false);

    // 2. OAuth credentials exist
    let creds_ok =
        std::path::Path::new("/home/claude/.claude/.credentials.json").exists();

    // 3. channel-telegram directory exists with node_modules
    let channel_ok = std::path::Path::new("/home/claude/.cloudcode/channel-telegram/node_modules")
        .exists();

    if !claude_ok {
        eprintln!("Channels mode: claude binary not found or version check failed");
    }
    if !creds_ok {
        eprintln!("Channels mode: OAuth credentials not found at ~/.claude/.credentials.json");
    }
    if !channel_ok {
        eprintln!("Channels mode: channel-telegram/node_modules not found");
    }

    claude_ok && creds_ok && channel_ok
}

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

    // Load default session store
    let default_session = match DefaultSessionStore::load() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            log::warn!(
                "Failed to load persisted Telegram default session, starting empty: {}",
                err
            );
            Arc::new(DefaultSessionStore::empty())
        }
    };

    // Determine Telegram mode and start the appropriate subsystem
    let telegram_mode_str;

    if let Some(ref tg_config) = config.telegram {
        let effective_mode = match tg_config.mode {
            TelegramMode::Channels => {
                if channels_prerequisites_met() {
                    TelegramMode::Channels
                } else {
                    eprintln!("ERROR: Channels mode requested but prerequisites not met");
                    std::process::exit(1);
                }
            }
            TelegramMode::Auto => {
                if channels_prerequisites_met() {
                    eprintln!("Auto mode: channels prerequisites met, using channels mode");
                    TelegramMode::Channels
                } else {
                    eprintln!("Auto mode: channels prerequisites not met, falling back to legacy");
                    TelegramMode::Legacy
                }
            }
            TelegramMode::Legacy => TelegramMode::Legacy,
        };

        telegram_mode_str = effective_mode.to_string();
        eprintln!("Telegram mode: {}", telegram_mode_str);

        match effective_mode {
            TelegramMode::Channels => {
                // Channels mode: dispatch HTTP server + reqwest sender + question poller
                let sender: Arc<dyn TelegramSender> =
                    Arc::new(ReqwestSender::new(&tg_config.bot_token));

                let daemon_state = Arc::new(DaemonState {
                    session_mgr: Arc::clone(&session_mgr),
                    default_session: Arc::clone(&default_session),
                    question_states: question_states.clone(),
                });

                // Spawn question poller with reqwest sender
                let poller_mgr = Arc::clone(&session_mgr);
                let poller_sender = Arc::clone(&sender);
                let poller_owner_id = tg_config.owner_id;
                let poller_states = question_states.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    question_poller::run_poller(
                        poller_mgr,
                        poller_sender,
                        poller_owner_id,
                        poller_states,
                    )
                    .await;
                });

                // Spawn dispatch HTTP server
                let dispatch_sender = Arc::clone(&sender);
                tokio::spawn(async move {
                    if let Err(e) =
                        telegram::dispatch_server::run(daemon_state, dispatch_sender).await
                    {
                        log::error!("Dispatch server failed: {}", e);
                    }
                });
            }
            TelegramMode::Legacy => {
                // Legacy mode: teloxide bot + question poller (original behavior)
                let mgr = Arc::clone(&session_mgr);
                let tg = tg_config.clone();
                let states = question_states.clone();

                let bot = teloxide::Bot::new(&tg.bot_token);

                // Spawn question poller with teloxide sender wrapper
                let poller_sender: Arc<dyn TelegramSender> =
                    Arc::new(TeloxideSender::new(bot.clone()));
                let poller_mgr = Arc::clone(&session_mgr);
                let poller_owner_id = tg.owner_id;
                let poller_states = question_states.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    question_poller::run_poller(
                        poller_mgr,
                        poller_sender,
                        poller_owner_id,
                        poller_states,
                    )
                    .await;
                });

                // Run telegram bot with retry
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
            TelegramMode::Auto => unreachable!("Auto resolved above"),
        }
    } else {
        telegram_mode_str = "disabled".to_string();
    }

    // Build ApiState for the control server
    let api_state = Arc::new(api::handlers::ApiState {
        session_mgr: Arc::clone(&session_mgr),
        default_session: Some(Arc::clone(&default_session)),
        question_states: Some(question_states),
        telegram_mode: Some(telegram_mode_str),
    });

    api::server::run_with_state(
        &config.listen_addr,
        config.listen_port,
        session_mgr,
        Some(api_state),
    )
    .await
}
