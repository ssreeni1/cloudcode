mod api;
mod config;
mod session;
mod telegram;

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

use session::manager::SessionManager;

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

    // TODO Phase 4: Start telegram bot if configured
    // if let Some(tg_config) = &config.telegram { ... }

    api::server::run(&config.listen_addr, config.listen_port, session_mgr).await
}
