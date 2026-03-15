use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub hetzner: Option<HetznerConfig>,
    pub claude: Option<ClaudeConfig>,
    pub telegram: Option<TelegramConfig>,
    pub vps: Option<VpsConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HetznerConfig {
    pub api_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub auth_method: String, // "api_key" or "oauth"
    pub api_key: Option<String>,
    pub oauth_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VpsConfig {
    pub server_type: Option<String>,
    pub location: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub owner_id: i64,
}

impl Config {
    pub fn dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".cloudcode"))
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        toml::from_str(&content).context("Failed to parse config")
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir()?;
        fs::create_dir_all(&dir)?;
        let path = Self::path()?;
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, &content)?;
        // Set 0600 permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn ssh_key_path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("id_ed25519"))
    }

    pub fn ssh_pub_key_path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("id_ed25519.pub"))
    }
}
