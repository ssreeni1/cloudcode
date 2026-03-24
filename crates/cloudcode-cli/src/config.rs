use anyhow::{Context, Result};
pub use cloudcode_common::provider::AiProvider;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// CloudKind — which infrastructure provider is active
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudKind {
    Hetzner,
    #[serde(alias = "digitalocean")]
    DigitalOcean,
}

impl fmt::Display for CloudKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hetzner => write!(f, "hetzner"),
            Self::DigitalOcean => write!(f, "digitalocean"),
        }
    }
}

impl FromStr for CloudKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "hetzner" => Ok(Self::Hetzner),
            "digitalocean" | "do" => Ok(Self::DigitalOcean),
            other => anyhow::bail!("unknown cloud provider: {other}"),
        }
    }
}

// ---------------------------------------------------------------------------
// DOConfig — DigitalOcean credentials
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct DOConfig {
    pub api_token: String,
}

// ---------------------------------------------------------------------------
// CloudConfig — unified cloud infrastructure config
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudConfig {
    pub provider: CloudKind,
    pub hetzner: Option<HetznerConfig>,
    pub digitalocean: Option<DOConfig>,
}

// ---------------------------------------------------------------------------
// AiProviderConfig — single reusable struct for all AI provider credentials
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AiProviderConfig {
    pub auth_method: AuthMethod,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CodexConfig {
    pub auth_method: AuthMethod,
    pub api_key: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Legacy top-level hetzner section — kept for backward compatibility.
    /// New configs should use `cloud.hetzner` instead.
    pub hetzner: Option<HetznerConfig>,
    pub claude: Option<ClaudeConfig>,
    pub codex: Option<CodexConfig>,
    pub amp: Option<AiProviderConfig>,
    pub opencode: Option<AiProviderConfig>,
    pub pi: Option<AiProviderConfig>,
    pub cursor: Option<AiProviderConfig>,
    pub telegram: Option<TelegramConfig>,
    pub vps: Option<VpsConfig>,
    pub default_provider: Option<AiProvider>,
    /// New unified cloud config (v2 format).
    pub cloud: Option<CloudConfig>,
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

    /// Load config, tolerating both v1 (`[hetzner]`) and v2 (`[cloud.hetzner]`) formats.
    ///
    /// If `[cloud]` is present, it takes precedence. If only the legacy top-level
    /// `[hetzner]` section exists, it is kept as-is so callers can read it without
    /// requiring migration. Migration only happens explicitly via `migrate_v1_to_v2()`.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Self = toml::from_str(&content).context("Failed to parse config")?;
        Ok(config)
    }

    /// Returns the effective cloud provider kind.
    ///
    /// Checks `cloud.provider` first, then falls back to `Hetzner` if the legacy
    /// top-level `[hetzner]` section is present. Returns `None` if no cloud
    /// provider is configured at all.
    pub fn effective_cloud_kind(&self) -> Option<CloudKind> {
        if let Some(ref cloud) = self.cloud {
            return Some(cloud.provider);
        }
        if self.hetzner.is_some() {
            return Some(CloudKind::Hetzner);
        }
        None
    }

    /// Returns the effective Hetzner config, checking `cloud.hetzner` first,
    /// then falling back to the legacy top-level `[hetzner]` section.
    pub fn effective_hetzner(&self) -> Option<&HetznerConfig> {
        self.cloud
            .as_ref()
            .and_then(|c| c.hetzner.as_ref())
            .or(self.hetzner.as_ref())
    }

    /// Migrate v1 (top-level `[hetzner]`) config to v2 (`[cloud]` section).
    ///
    /// This is idempotent — if `cloud` is already set, it's a no-op.
    /// Only call from `init` / `up` commands, never from read-only paths.
    pub fn migrate_v1_to_v2(&mut self) {
        if self.cloud.is_some() {
            return;
        }
        if let Some(ref hetzner) = self.hetzner {
            self.cloud = Some(CloudConfig {
                provider: CloudKind::Hetzner,
                hetzner: Some(HetznerConfig {
                    api_token: hetzner.api_token.clone(),
                }),
                digitalocean: None,
            });
        }
    }

    /// Save config with a `.v1.bak` backup of the existing file.
    ///
    /// Use this when performing destructive operations like migration so the
    /// user can recover if something goes wrong.
    pub fn save_with_backup(&self) -> Result<()> {
        let path = Self::path()?;
        if path.exists() {
            let backup = path.with_extension("toml.v1.bak");
            fs::copy(&path, &backup)
                .with_context(|| format!("Failed to create backup at {}", backup.display()))?;
        }
        self.save()
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
        assert!(config.cloud.is_none());
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
        assert!(deserialized.cloud.is_none());
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
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
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: None,
            default_provider: None,
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: Some("cx23".to_string()),
                location: None,
                image: None,
            }),
            default_provider: None,
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: Some(VpsConfig {
                server_type: None,
                location: None,
                image: None,
            }),
            default_provider: None,
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: None,
            default_provider: None,
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: Some(TelegramConfig {
                bot_token: "tok".to_string(),
                owner_id: -1,
                mode: TelegramMode::default(),
            }),
            vps: None,
            default_provider: None,
            cloud: None,
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
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: None,
            default_provider: Some(AiProvider::Codex),
            cloud: None,
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

    // -----------------------------------------------------------------------
    // CloudKind tests
    // -----------------------------------------------------------------------

    #[test]
    fn cloud_kind_from_str() {
        assert_eq!("hetzner".parse::<CloudKind>().unwrap(), CloudKind::Hetzner);
        assert_eq!("Hetzner".parse::<CloudKind>().unwrap(), CloudKind::Hetzner);
        assert_eq!("HETZNER".parse::<CloudKind>().unwrap(), CloudKind::Hetzner);
        assert_eq!(
            "digitalocean".parse::<CloudKind>().unwrap(),
            CloudKind::DigitalOcean
        );
        assert_eq!(
            "DigitalOcean".parse::<CloudKind>().unwrap(),
            CloudKind::DigitalOcean
        );
        assert_eq!("do".parse::<CloudKind>().unwrap(), CloudKind::DigitalOcean);
        assert!("aws".parse::<CloudKind>().is_err());
    }

    #[test]
    fn cloud_kind_display() {
        assert_eq!(CloudKind::Hetzner.to_string(), "hetzner");
        assert_eq!(CloudKind::DigitalOcean.to_string(), "digitalocean");
    }

    #[test]
    fn cloud_kind_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&CloudKind::Hetzner).unwrap(),
            "\"hetzner\""
        );
        assert_eq!(
            serde_json::to_string(&CloudKind::DigitalOcean).unwrap(),
            "\"digital_ocean\""
        );
        // Deserialize both snake_case and alias
        assert_eq!(
            serde_json::from_str::<CloudKind>("\"digital_ocean\"").unwrap(),
            CloudKind::DigitalOcean
        );
        assert_eq!(
            serde_json::from_str::<CloudKind>("\"digitalocean\"").unwrap(),
            CloudKind::DigitalOcean
        );
    }

    // -----------------------------------------------------------------------
    // CloudConfig / dual-read tests
    // -----------------------------------------------------------------------

    #[test]
    fn dual_read_old_hetzner_format() {
        // v1 config: top-level [hetzner] only, no [cloud]
        let toml_str = r#"
[hetzner]
api_token = "old-token"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.cloud.is_none());
        assert_eq!(config.hetzner.as_ref().unwrap().api_token, "old-token");
        // effective_cloud_kind falls back to legacy
        assert_eq!(config.effective_cloud_kind(), Some(CloudKind::Hetzner));
        // effective_hetzner falls back to legacy
        assert_eq!(config.effective_hetzner().unwrap().api_token, "old-token");
    }

    #[test]
    fn dual_read_new_cloud_format() {
        let toml_str = r#"
[cloud]
provider = "hetzner"

[cloud.hetzner]
api_token = "new-token"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.hetzner.is_none());
        assert_eq!(config.effective_cloud_kind(), Some(CloudKind::Hetzner));
        assert_eq!(config.effective_hetzner().unwrap().api_token, "new-token");
    }

    #[test]
    fn dual_read_cloud_takes_precedence_over_legacy() {
        // Both old [hetzner] and new [cloud.hetzner] present — cloud wins
        let toml_str = r#"
[hetzner]
api_token = "old-token"

[cloud]
provider = "hetzner"

[cloud.hetzner]
api_token = "new-token"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.effective_cloud_kind(), Some(CloudKind::Hetzner));
        assert_eq!(config.effective_hetzner().unwrap().api_token, "new-token");
    }

    #[test]
    fn cloud_digitalocean_config() {
        let toml_str = r#"
[cloud]
provider = "digital_ocean"

[cloud.digitalocean]
api_token = "do-token-123"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.effective_cloud_kind(),
            Some(CloudKind::DigitalOcean)
        );
        let cloud = config.cloud.unwrap();
        assert_eq!(cloud.digitalocean.unwrap().api_token, "do-token-123");
        assert!(cloud.hetzner.is_none());
    }

    #[test]
    fn no_cloud_provider_configured() {
        let config = Config::default();
        assert_eq!(config.effective_cloud_kind(), None);
        assert!(config.effective_hetzner().is_none());
    }

    // -----------------------------------------------------------------------
    // Migration tests
    // -----------------------------------------------------------------------

    #[test]
    fn migrate_v1_to_v2_moves_hetzner() {
        let mut config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "legacy-tok".to_string(),
            }),
            cloud: None,
            ..Config::default()
        };

        config.migrate_v1_to_v2();

        let cloud = config.cloud.as_ref().unwrap();
        assert_eq!(cloud.provider, CloudKind::Hetzner);
        assert_eq!(cloud.hetzner.as_ref().unwrap().api_token, "legacy-tok");
        assert!(cloud.digitalocean.is_none());
        // Legacy field is preserved (not cleared) — callers that read it still work
        assert!(config.hetzner.is_some());
    }

    #[test]
    fn migrate_v1_to_v2_is_idempotent() {
        let mut config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "legacy-tok".to_string(),
            }),
            cloud: Some(CloudConfig {
                provider: CloudKind::DigitalOcean,
                hetzner: None,
                digitalocean: Some(DOConfig {
                    api_token: "do-tok".to_string(),
                }),
            }),
            ..Config::default()
        };

        config.migrate_v1_to_v2();

        // cloud is untouched — DO config preserved
        let cloud = config.cloud.as_ref().unwrap();
        assert_eq!(cloud.provider, CloudKind::DigitalOcean);
        assert_eq!(cloud.digitalocean.as_ref().unwrap().api_token, "do-tok");
    }

    #[test]
    fn migrate_v1_to_v2_noop_when_no_hetzner() {
        let mut config = Config::default();
        config.migrate_v1_to_v2();
        assert!(config.cloud.is_none());
    }

    // -----------------------------------------------------------------------
    // AiProviderConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn ai_provider_config_roundtrip() {
        let cfg = AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("sk-test".to_string()),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: AiProviderConfig = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized.auth_method, AuthMethod::ApiKey));
        assert_eq!(deserialized.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn cloud_config_full_toml_roundtrip() {
        let config = Config {
            hetzner: None,
            claude: None,
            codex: None,
            amp: None,
            opencode: None,
            pi: None,
            cursor: None,
            telegram: None,
            vps: None,
            default_provider: None,
            cloud: Some(CloudConfig {
                provider: CloudKind::Hetzner,
                hetzner: Some(HetznerConfig {
                    api_token: "hz-tok".to_string(),
                }),
                digitalocean: None,
            }),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        let cloud = deserialized.cloud.unwrap();
        assert_eq!(cloud.provider, CloudKind::Hetzner);
        assert_eq!(cloud.hetzner.unwrap().api_token, "hz-tok");
    }
}
