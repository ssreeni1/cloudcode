use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VpsState {
    pub server_id: Option<u64>,
    pub server_ip: Option<String>,
    pub ssh_key_id: Option<u64>,
    pub status: Option<String>,
}

impl VpsState {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".cloudcode").join("state.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).context("Failed to parse state")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Self::path()?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn is_provisioned(&self) -> bool {
        self.server_id.is_some() && self.server_ip.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_none() {
        let state = VpsState::default();
        assert!(state.server_id.is_none());
        assert!(state.server_ip.is_none());
        assert!(state.ssh_key_id.is_none());
        assert!(state.status.is_none());
    }

    #[test]
    fn is_provisioned_false_when_empty() {
        let state = VpsState::default();
        assert!(!state.is_provisioned());
    }

    #[test]
    fn is_provisioned_false_when_only_server_id() {
        let state = VpsState {
            server_id: Some(123),
            server_ip: None,
            ssh_key_id: None,
            status: None,
        };
        assert!(!state.is_provisioned());
    }

    #[test]
    fn is_provisioned_false_when_only_server_ip() {
        let state = VpsState {
            server_id: None,
            server_ip: Some("1.2.3.4".to_string()),
            ssh_key_id: None,
            status: None,
        };
        assert!(!state.is_provisioned());
    }

    #[test]
    fn is_provisioned_true_when_server_id_and_ip_set() {
        let state = VpsState {
            server_id: Some(42),
            server_ip: Some("10.0.0.1".to_string()),
            ssh_key_id: None,
            status: None,
        };
        assert!(state.is_provisioned());
    }

    #[test]
    fn is_provisioned_true_with_all_fields() {
        let state = VpsState {
            server_id: Some(1),
            server_ip: Some("192.168.1.1".to_string()),
            ssh_key_id: Some(99),
            status: Some("running".to_string()),
        };
        assert!(state.is_provisioned());
    }

    #[test]
    fn json_roundtrip_default() {
        let state = VpsState::default();
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();
        assert!(deserialized.server_id.is_none());
        assert!(deserialized.server_ip.is_none());
        assert!(deserialized.ssh_key_id.is_none());
        assert!(deserialized.status.is_none());
    }

    #[test]
    fn json_roundtrip_all_fields() {
        let state = VpsState {
            server_id: Some(12345),
            server_ip: Some("203.0.113.50".to_string()),
            ssh_key_id: Some(678),
            status: Some("running".to_string()),
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_id, Some(12345));
        assert_eq!(deserialized.server_ip.as_deref(), Some("203.0.113.50"));
        assert_eq!(deserialized.ssh_key_id, Some(678));
        assert_eq!(deserialized.status.as_deref(), Some("running"));
    }

    #[test]
    fn json_roundtrip_partial_fields() {
        let state = VpsState {
            server_id: Some(1),
            server_ip: None,
            ssh_key_id: None,
            status: Some("initializing".to_string()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_id, Some(1));
        assert!(deserialized.server_ip.is_none());
        assert!(deserialized.ssh_key_id.is_none());
        assert_eq!(deserialized.status.as_deref(), Some("initializing"));
    }

    #[test]
    fn deserialize_from_handwritten_json() {
        let json = r#"{"server_id":99,"server_ip":"10.0.0.5"}"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert_eq!(state.server_id, Some(99));
        assert_eq!(state.server_ip.as_deref(), Some("10.0.0.5"));
        // Missing fields default to None via serde
        assert!(state.ssh_key_id.is_none());
        assert!(state.status.is_none());
    }

    #[test]
    fn json_field_names_are_snake_case() {
        let state = VpsState {
            server_id: Some(1),
            server_ip: Some("x".to_string()),
            ssh_key_id: Some(2),
            status: Some("s".to_string()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value.get("server_id").is_some());
        assert!(value.get("server_ip").is_some());
        assert!(value.get("ssh_key_id").is_some());
        assert!(value.get("status").is_some());
    }

    #[test]
    fn server_id_zero_is_valid() {
        let state = VpsState {
            server_id: Some(0),
            server_ip: Some("0.0.0.0".to_string()),
            ssh_key_id: None,
            status: None,
        };
        assert!(state.is_provisioned());
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.server_id, Some(0));
    }
}
