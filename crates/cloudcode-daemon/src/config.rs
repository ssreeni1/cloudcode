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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Default values
    // -----------------------------------------------------------------------

    #[test]
    fn default_listen_addr_is_localhost() {
        let config = DaemonConfig::default();
        assert_eq!(config.listen_addr, "127.0.0.1");
    }

    #[test]
    fn default_listen_port_is_7700() {
        let config = DaemonConfig::default();
        assert_eq!(config.listen_port, 7700);
    }

    #[test]
    fn default_telegram_is_none() {
        let config = DaemonConfig::default();
        assert!(config.telegram.is_none());
    }

    // -----------------------------------------------------------------------
    // TOML deserialization — all fields present
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_toml_with_all_fields() {
        let toml_str = r#"
            listen_addr = "0.0.0.0"
            listen_port = 9900

            [telegram]
            bot_token = "123:ABC"
            owner_id = 42
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_addr, "0.0.0.0");
        assert_eq!(config.listen_port, 9900);
        let tg = config.telegram.unwrap();
        assert_eq!(tg.bot_token, "123:ABC");
        assert_eq!(tg.owner_id, 42);
    }

    // -----------------------------------------------------------------------
    // TOML deserialization — without telegram section
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_toml_without_telegram() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 7700
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1");
        assert_eq!(config.listen_port, 7700);
        assert!(config.telegram.is_none());
    }

    // -----------------------------------------------------------------------
    // TOML deserialization — with telegram section
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_toml_with_telegram() {
        let toml_str = r#"
            listen_addr = "10.0.0.1"
            listen_port = 8080

            [telegram]
            bot_token = "token-value"
            owner_id = -999
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        let tg = config.telegram.expect("telegram should be Some");
        assert_eq!(tg.bot_token, "token-value");
        assert_eq!(tg.owner_id, -999);
    }

    #[test]
    fn telegram_owner_id_can_be_negative() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 7700

            [telegram]
            bot_token = "tok"
            owner_id = -100200300
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.telegram.unwrap().owner_id, -100200300);
    }

    // -----------------------------------------------------------------------
    // TOML roundtrip via serde
    // -----------------------------------------------------------------------

    #[test]
    fn toml_roundtrip_without_telegram() {
        let config = DaemonConfig {
            listen_addr: "192.168.1.1".to_string(),
            listen_port: 5500,
            telegram: None,
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: DaemonConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.listen_addr, "192.168.1.1");
        assert_eq!(deserialized.listen_port, 5500);
        assert!(deserialized.telegram.is_none());
    }

    #[test]
    fn toml_roundtrip_with_telegram() {
        let config = DaemonConfig {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 443,
            telegram: Some(TelegramConfig {
                bot_token: "abc:xyz".to_string(),
                owner_id: 12345,
            }),
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: DaemonConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.listen_addr, "0.0.0.0");
        assert_eq!(deserialized.listen_port, 443);
        let tg = deserialized.telegram.unwrap();
        assert_eq!(tg.bot_token, "abc:xyz");
        assert_eq!(tg.owner_id, 12345);
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_toml_missing_listen_addr_fails() {
        let toml_str = r#"listen_port = 7700"#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_toml_missing_listen_port_fails() {
        let toml_str = r#"listen_addr = "127.0.0.1""#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_toml_invalid_port_type_fails() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = "not-a-number"
        "#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_toml_port_too_large_fails() {
        // u16 max is 65535; 70000 should overflow
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 70000
        "#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_toml_telegram_missing_bot_token_fails() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 7700

            [telegram]
            owner_id = 42
        "#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_toml_telegram_missing_owner_id_fails() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 7700

            [telegram]
            bot_token = "tok"
        "#;
        let result: Result<DaemonConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Load from file — error path (nonexistent file)
    // -----------------------------------------------------------------------

    #[test]
    fn load_nonexistent_file_returns_error() {
        let result = DaemonConfig::load(Path::new("/tmp/does-not-exist-cloudcode-test.toml"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("Failed to read daemon config"));
    }

    // -----------------------------------------------------------------------
    // Boundary values
    // -----------------------------------------------------------------------

    #[test]
    fn port_zero_is_valid_toml() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 0
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_port, 0);
    }

    #[test]
    fn port_max_u16_is_valid_toml() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            listen_port = 65535
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_port, 65535);
    }

    #[test]
    fn empty_listen_addr_is_accepted() {
        // serde does not validate the address value, just the type
        let toml_str = r#"
            listen_addr = ""
            listen_port = 7700
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_addr, "");
    }
}
