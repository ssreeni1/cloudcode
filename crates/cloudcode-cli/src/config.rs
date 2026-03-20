use anyhow::{Context, Result};
pub use cloudcode_common::provider::AiProvider;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct CodexConfig {
    pub auth_method: AuthMethod,
    pub api_key: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub hetzner: Option<HetznerConfig>,
    pub claude: Option<ClaudeConfig>,
    pub codex: Option<CodexConfig>,
    pub telegram: Option<TelegramConfig>,
    pub vps: Option<VpsConfig>,
    pub default_provider: Option<AiProvider>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HetznerConfig {
    pub api_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey,
    Oauth,
}

impl AuthMethod {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::ApiKey => "API Key",
            Self::Oauth => "OAuth",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub auth_method: AuthMethod,
    pub api_key: Option<String>,
    pub oauth_token: Option<String>,
}

impl ClaudeConfig {
    pub fn uses_api_key(&self) -> bool {
        matches!(self.auth_method, AuthMethod::ApiKey)
    }

    pub fn uses_oauth(&self) -> bool {
        matches!(self.auth_method, AuthMethod::Oauth)
    }

    pub const fn auth_label(&self) -> &'static str {
        self.auth_method.as_str()
    }

    pub const fn auth_display_name(&self) -> &'static str {
        self.auth_method.display_name()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VpsConfig {
    pub server_type: Option<String>,
    pub location: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramMode {
    Legacy,
    Channels,
    Auto,
}

impl Default for TelegramMode {
    fn default() -> Self {
        Self::Legacy
    }
}

impl std::fmt::Display for TelegramMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy => write!(f, "legacy"),
            Self::Channels => write!(f, "channels"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub owner_id: i64,
    #[serde(default)]
    pub mode: TelegramMode,
}

impl Config {
    pub fn dir() -> Result<PathBuf> {
        crate::paths::config_dir()
    }

    pub fn path() -> Result<PathBuf> {
        crate::paths::config_file()
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
        Self::ensure_dir()?;
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
        crate::paths::ssh_key()
    }

    pub fn ssh_pub_key_path() -> Result<PathBuf> {
        crate::paths::ssh_pub_key()
    }

    pub fn ensure_dir() -> Result<PathBuf> {
        let dir = Self::dir()?;
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(dir)
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
        assert!(config.codex.is_none());
        assert!(config.default_provider.is_none());
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
        assert!(deserialized.codex.is_none());
        assert!(deserialized.default_provider.is_none());
    }

    #[test]
    fn full_config_toml_roundtrip() {
        let config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "hcloud-token-123".to_string(),
            }),
            claude: Some(ClaudeConfig {
                auth_method: AuthMethod::ApiKey,
                api_key: Some("sk-ant-key".to_string()),
                oauth_token: None,
            }),
            codex: None,
            telegram: Some(TelegramConfig {
                bot_token: "123456:ABC-DEF".to_string(),
                owner_id: 987654321,
                mode: TelegramMode::default(),
            }),
            vps: Some(VpsConfig {
                server_type: Some("cx23".to_string()),
                location: Some("fsn1".to_string()),
                image: Some("ubuntu-24.04".to_string()),
            }),
            default_provider: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        let h = deserialized.hetzner.unwrap();
        assert_eq!(h.api_token, "hcloud-token-123");

        let c = deserialized.claude.unwrap();
        assert!(matches!(c.auth_method, AuthMethod::ApiKey));
        assert_eq!(c.api_key, Some("sk-ant-key".to_string()));
        assert!(c.oauth_token.is_none());

        let t = deserialized.telegram.unwrap();
        assert_eq!(t.bot_token, "123456:ABC-DEF");
        assert_eq!(t.owner_id, 987654321);

        let v = deserialized.vps.unwrap();
        assert_eq!(v.server_type, Some("cx23".to_string()));
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
            codex: None,
            telegram: None,
            vps: None,
            default_provider: None,
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
            codex: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: Some("cx23".to_string()),
                location: None,
                image: None,
            }),
            default_provider: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert!(deserialized.hetzner.is_some());
        assert!(deserialized.telegram.is_none());
        let v = deserialized.vps.unwrap();
        assert_eq!(v.server_type, Some("cx23".to_string()));
        assert!(v.location.is_none());
        assert!(v.image.is_none());
    }

    #[test]
    fn vps_config_all_none_fields() {
        let config = Config {
            hetzner: None,
            claude: None,
            codex: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: None,
                location: None,
                image: None,
            }),
            default_provider: None,
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
                auth_method: AuthMethod::Oauth,
                api_key: None,
                oauth_token: Some("oauth-tok-xyz".to_string()),
            }),
            codex: None,
            telegram: None,
            vps: None,
            default_provider: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        let c = deserialized.claude.unwrap();
        assert!(matches!(c.auth_method, AuthMethod::Oauth));
        assert!(c.api_key.is_none());
        assert_eq!(c.oauth_token, Some("oauth-tok-xyz".to_string()));
    }

    #[test]
    fn auth_method_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&AuthMethod::ApiKey).unwrap(),
            "\"api_key\""
        );
        assert_eq!(
            serde_json::to_string(&AuthMethod::Oauth).unwrap(),
            "\"oauth\""
        );
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
            codex: None,
            telegram: Some(TelegramConfig {
                bot_token: "tok".to_string(),
                owner_id: -1,
                mode: TelegramMode::default(),
            }),
            vps: None,
            default_provider: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.telegram.unwrap().owner_id, -1);
    }

    #[test]
    fn ai_provider_default_is_claude() {
        assert_eq!(AiProvider::default(), AiProvider::Claude);
    }

    #[test]
    fn ai_provider_from_str_roundtrip() {
        assert_eq!("claude".parse::<AiProvider>().unwrap(), AiProvider::Claude);
        assert_eq!("codex".parse::<AiProvider>().unwrap(), AiProvider::Codex);
        assert_eq!("Claude".parse::<AiProvider>().unwrap(), AiProvider::Claude);
        assert_eq!("CODEX".parse::<AiProvider>().unwrap(), AiProvider::Codex);
        assert!("unknown".parse::<AiProvider>().is_err());
    }

    #[test]
    fn ai_provider_display() {
        assert_eq!(AiProvider::Claude.to_string(), "claude");
        assert_eq!(AiProvider::Codex.to_string(), "codex");
    }

    #[test]
    fn ai_provider_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&AiProvider::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::Codex).unwrap(),
            "\"codex\""
        );
    }

    #[test]
    fn config_with_codex_roundtrip() {
        let config = Config {
            hetzner: None,
            claude: None,
            codex: Some(CodexConfig {
                auth_method: AuthMethod::ApiKey,
                api_key: Some("sk-openai-key".to_string()),
            }),
            telegram: None,
            vps: None,
            default_provider: Some(AiProvider::Codex),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert!(deserialized.codex.is_some());
        let c = deserialized.codex.unwrap();
        assert!(matches!(c.auth_method, AuthMethod::ApiKey));
        assert_eq!(c.api_key, Some("sk-openai-key".to_string()));
        assert_eq!(deserialized.default_provider, Some(AiProvider::Codex));
    }

    #[test]
    fn backward_compat_old_config_without_codex() {
        // Simulates loading a config.toml written before Codex support was added
        let toml_str = r#"
[hetzner]
api_token = "my-token"

[claude]
auth_method = "api_key"
api_key = "sk-ant-key"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.codex.is_none());
        assert!(config.default_provider.is_none());
        assert!(config.claude.is_some());
    }
}
