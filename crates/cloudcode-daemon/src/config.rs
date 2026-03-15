use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub listen_addr: String,
    pub listen_port: u16,
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub owner_id: i64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 7700,
            telegram: None,
        }
    }
}

impl DaemonConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read daemon config from {}", path.display()))?;
        toml::from_str(&content).context("Failed to parse daemon config")
    }
}
