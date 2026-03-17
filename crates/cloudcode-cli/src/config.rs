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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_all_none() {
        let config = Config::default();
        assert!(config.hetzner.is_none());
        assert!(config.claude.is_none());
        assert!(config.telegram.is_none());
        assert!(config.vps.is_none());
    }

    #[test]
    fn default_config_toml_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert!(deserialized.hetzner.is_none());
        assert!(deserialized.claude.is_none());
        assert!(deserialized.telegram.is_none());
        assert!(deserialized.vps.is_none());
    }

    #[test]
    fn full_config_toml_roundtrip() {
        let config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "hcloud-token-123".to_string(),
            }),
            claude: Some(ClaudeConfig {
                auth_method: "api_key".to_string(),
                api_key: Some("sk-ant-key".to_string()),
                oauth_token: None,
            }),
            telegram: Some(TelegramConfig {
                bot_token: "123456:ABC-DEF".to_string(),
                owner_id: 987654321,
            }),
            vps: Some(VpsConfig {
                server_type: Some("cx22".to_string()),
                location: Some("fsn1".to_string()),
                image: Some("ubuntu-24.04".to_string()),
            }),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        let h = deserialized.hetzner.unwrap();
        assert_eq!(h.api_token, "hcloud-token-123");

        let c = deserialized.claude.unwrap();
        assert_eq!(c.auth_method, "api_key");
        assert_eq!(c.api_key, Some("sk-ant-key".to_string()));
        assert!(c.oauth_token.is_none());

        let t = deserialized.telegram.unwrap();
        assert_eq!(t.bot_token, "123456:ABC-DEF");
        assert_eq!(t.owner_id, 987654321);

        let v = deserialized.vps.unwrap();
        assert_eq!(v.server_type, Some("cx22".to_string()));
        assert_eq!(v.location, Some("fsn1".to_string()));
        assert_eq!(v.image, Some("ubuntu-24.04".to_string()));
    }

    #[test]
    fn partial_config_only_hetzner() {
        let config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "token".to_string(),
            }),
            claude: None,
            telegram: None,
            vps: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert!(deserialized.hetzner.is_some());
        assert!(deserialized.claude.is_none());
        assert!(deserialized.telegram.is_none());
        assert!(deserialized.vps.is_none());
    }

    #[test]
    fn partial_config_hetzner_and_vps_no_telegram() {
        let config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "tok".to_string(),
            }),
            claude: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: Some("cx22".to_string()),
                location: None,
                image: None,
            }),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert!(deserialized.hetzner.is_some());
        assert!(deserialized.telegram.is_none());
        let v = deserialized.vps.unwrap();
        assert_eq!(v.server_type, Some("cx22".to_string()));
        assert!(v.location.is_none());
        assert!(v.image.is_none());
    }

    #[test]
    fn vps_config_all_none_fields() {
        let config = Config {
            hetzner: None,
            claude: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: None,
                location: None,
                image: None,
            }),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        let v = deserialized.vps.unwrap();
        assert!(v.server_type.is_none());
        assert!(v.location.is_none());
        assert!(v.image.is_none());
    }

    #[test]
    fn claude_config_oauth_method() {
        let config = Config {
            hetzner: None,
            claude: Some(ClaudeConfig {
                auth_method: "oauth".to_string(),
                api_key: None,
                oauth_token: Some("oauth-tok-xyz".to_string()),
            }),
            telegram: None,
            vps: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        let c = deserialized.claude.unwrap();
        assert_eq!(c.auth_method, "oauth");
        assert!(c.api_key.is_none());
        assert_eq!(c.oauth_token, Some("oauth-tok-xyz".to_string()));
    }

    #[test]
    fn deserialize_from_handwritten_toml() {
        let toml_str = r#"
[hetzner]
api_token = "my-token"

[telegram]
bot_token = "bot123"
owner_id = 42
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hetzner.unwrap().api_token, "my-token");
        assert!(config.claude.is_none());
        let t = config.telegram.unwrap();
        assert_eq!(t.bot_token, "bot123");
        assert_eq!(t.owner_id, 42);
        assert!(config.vps.is_none());
    }

    #[test]
    fn telegram_owner_id_negative() {
        let config = Config {
            hetzner: None,
            claude: None,
            telegram: Some(TelegramConfig {
                bot_token: "tok".to_string(),
                owner_id: -1,
            }),
            vps: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.telegram.unwrap().owner_id, -1);
    }
}
