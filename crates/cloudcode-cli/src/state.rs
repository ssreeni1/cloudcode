use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpsStatus {
    Creating,
    Initializing,
    Running,
    Error,
}

impl VpsStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Initializing => "initializing",
            Self::Running => "running",
            Self::Error => "error",
        }
    }
}

/// Deserialize a value that may be either a JSON string or a JSON number (u64)
/// into an `Option<String>`. This provides backward compatibility with old
/// state.json files that stored `server_id` and `ssh_key_id` as u64.
fn deserialize_string_or_u64<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrU64Visitor;

    impl<'de> Visitor<'de> for StringOrU64Visitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string, a u64, or null")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: Deserializer<'de>>(
            self,
            deserializer: D2,
        ) -> Result<Self::Value, D2::Error> {
            deserializer.deserialize_any(StringOrU64InnerVisitor)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
    }

    struct StringOrU64InnerVisitor;

    impl<'de> Visitor<'de> for StringOrU64InnerVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or a u64")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
    }

    deserializer.deserialize_option(StringOrU64Visitor)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VpsState {
    #[serde(default, deserialize_with = "deserialize_string_or_u64")]
    pub server_id: Option<String>,
    pub server_ip: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_u64")]
    pub ssh_key_id: Option<String>,
    pub status: Option<VpsStatus>,
    /// Cloud infrastructure provider (e.g., "hetzner", "digitalocean").
    /// Defaults to "hetzner" for backward compatibility with old state files.
    pub cloud_provider: Option<String>,
    /// Server type as provisioned (e.g., "cx23", "s-1vcpu-1gb").
    /// Stored at provision time so status can report accurately even if config changes.
    pub server_type: Option<String>,
    /// Datacenter location as provisioned (e.g., "nbg1", "nyc1").
    /// Stored at provision time so status can report accurately even if config changes.
    pub location: Option<String>,
}

impl VpsState {
    pub fn path() -> Result<PathBuf> {
        crate::paths::state_file()
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
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        let content = serde_json::to_string_pretty(self)?;
        // Write to a tmp file first, then atomically rename to prevent corruption
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, content)?;
        fs::rename(&tmp_path, &path).context("Failed to atomically replace state.json")?;
        // Set 0600 permissions on the final file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
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

    pub fn status_name(&self) -> Option<&'static str> {
        self.status.as_ref().map(VpsStatus::as_str)
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
        assert!(state.cloud_provider.is_none());
        assert!(state.server_type.is_none());
        assert!(state.location.is_none());
    }

    #[test]
    fn is_provisioned_false_when_empty() {
        let state = VpsState::default();
        assert!(!state.is_provisioned());
    }

    #[test]
    fn is_provisioned_false_when_only_server_id() {
        let state = VpsState {
            server_id: Some("123".to_string()),
            server_ip: None,
            ssh_key_id: None,
            status: None,
            cloud_provider: None,
            server_type: None,
            location: None,
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
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        assert!(!state.is_provisioned());
    }

    #[test]
    fn is_provisioned_true_when_server_id_and_ip_set() {
        let state = VpsState {
            server_id: Some("42".to_string()),
            server_ip: Some("10.0.0.1".to_string()),
            ssh_key_id: None,
            status: None,
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        assert!(state.is_provisioned());
    }

    #[test]
    fn is_provisioned_true_with_all_fields() {
        let state = VpsState {
            server_id: Some("1".to_string()),
            server_ip: Some("192.168.1.1".to_string()),
            ssh_key_id: Some("99".to_string()),
            status: Some(VpsStatus::Running),
            cloud_provider: Some("hetzner".to_string()),
            server_type: Some("cx23".to_string()),
            location: Some("nbg1".to_string()),
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
        assert!(deserialized.cloud_provider.is_none());
        assert!(deserialized.server_type.is_none());
        assert!(deserialized.location.is_none());
    }

    #[test]
    fn json_roundtrip_all_fields() {
        let state = VpsState {
            server_id: Some("12345".to_string()),
            server_ip: Some("203.0.113.50".to_string()),
            ssh_key_id: Some("678".to_string()),
            status: Some(VpsStatus::Running),
            cloud_provider: Some("hetzner".to_string()),
            server_type: Some("cx23".to_string()),
            location: Some("nbg1".to_string()),
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_id.as_deref(), Some("12345"));
        assert_eq!(deserialized.server_ip.as_deref(), Some("203.0.113.50"));
        assert_eq!(deserialized.ssh_key_id.as_deref(), Some("678"));
        assert!(matches!(deserialized.status, Some(VpsStatus::Running)));
        assert_eq!(deserialized.cloud_provider.as_deref(), Some("hetzner"));
        assert_eq!(deserialized.server_type.as_deref(), Some("cx23"));
        assert_eq!(deserialized.location.as_deref(), Some("nbg1"));
    }

    #[test]
    fn json_roundtrip_partial_fields() {
        let state = VpsState {
            server_id: Some("1".to_string()),
            server_ip: None,
            ssh_key_id: None,
            status: Some(VpsStatus::Initializing),
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_id.as_deref(), Some("1"));
        assert!(deserialized.server_ip.is_none());
        assert!(deserialized.ssh_key_id.is_none());
        assert!(matches!(deserialized.status, Some(VpsStatus::Initializing)));
    }

    #[test]
    fn deserialize_from_handwritten_json() {
        let json = r#"{"server_id":"99","server_ip":"10.0.0.5"}"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert_eq!(state.server_id.as_deref(), Some("99"));
        assert_eq!(state.server_ip.as_deref(), Some("10.0.0.5"));
        // Missing fields default to None via serde
        assert!(state.ssh_key_id.is_none());
        assert!(state.status.is_none());
        assert!(state.cloud_provider.is_none());
    }

    #[test]
    fn json_field_names_are_snake_case() {
        let state = VpsState {
            server_id: Some("1".to_string()),
            server_ip: Some("x".to_string()),
            ssh_key_id: Some("2".to_string()),
            status: Some(VpsStatus::Running),
            cloud_provider: Some("hetzner".to_string()),
            server_type: Some("cx23".to_string()),
            location: Some("nbg1".to_string()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value.get("server_id").is_some());
        assert!(value.get("server_ip").is_some());
        assert!(value.get("ssh_key_id").is_some());
        assert!(value.get("status").is_some());
        assert!(value.get("cloud_provider").is_some());
        assert!(value.get("server_type").is_some());
        assert!(value.get("location").is_some());
    }

    // -----------------------------------------------------------------------
    // Partial state tests (for incremental saves / orphaned key cleanup)
    // -----------------------------------------------------------------------

    #[test]
    fn partial_state_ssh_key_only() {
        let state = VpsState {
            server_id: None,
            server_ip: None,
            ssh_key_id: Some("42".to_string()),
            status: Some(VpsStatus::Creating),
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        assert!(!state.is_provisioned());
        assert!(state.ssh_key_id.is_some());
        // This is the partial state that `down` should handle
    }

    #[test]
    fn partial_state_needs_cleanup_check() {
        // After SSH key creation but before server creation
        let state = VpsState {
            server_id: None,
            server_ip: None,
            ssh_key_id: Some("99".to_string()),
            status: Some(VpsStatus::Creating),
            cloud_provider: Some("hetzner".to_string()),
            server_type: None,
            location: None,
        };
        // down should check: !is_provisioned() && ssh_key_id.is_some()
        assert!(!state.is_provisioned() && state.ssh_key_id.is_some());
    }

    #[test]
    fn partial_state_no_resources_at_all() {
        let state = VpsState::default();
        // down should bail: !is_provisioned() && ssh_key_id.is_none()
        assert!(!state.is_provisioned() && state.ssh_key_id.is_none());
    }

    #[test]
    fn full_state_is_provisioned() {
        let state = VpsState {
            server_id: Some("123".to_string()),
            server_ip: Some("10.0.0.1".to_string()),
            ssh_key_id: Some("456".to_string()),
            status: Some(VpsStatus::Running),
            cloud_provider: Some("hetzner".to_string()),
            server_type: Some("cx23".to_string()),
            location: Some("nbg1".to_string()),
        };
        assert!(state.is_provisioned());
        // down should proceed with full deprovision
    }

    #[test]
    fn json_roundtrip_partial_state_ssh_key_only() {
        let state = VpsState {
            server_id: None,
            server_ip: None,
            ssh_key_id: Some("42".to_string()),
            status: Some(VpsStatus::Creating),
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();
        assert!(deserialized.server_id.is_none());
        assert!(deserialized.server_ip.is_none());
        assert_eq!(deserialized.ssh_key_id.as_deref(), Some("42"));
        assert!(matches!(deserialized.status, Some(VpsStatus::Creating)));
    }

    #[test]
    fn vps_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&VpsStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&VpsStatus::Initializing).unwrap(),
            "\"initializing\""
        );
    }

    #[test]
    fn server_id_zero_is_valid() {
        let state = VpsState {
            server_id: Some("0".to_string()),
            server_ip: Some("0.0.0.0".to_string()),
            ssh_key_id: None,
            status: None,
            cloud_provider: None,
            server_type: None,
            location: None,
        };
        assert!(state.is_provisioned());
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.server_id.as_deref(), Some("0"));
    }

    // -----------------------------------------------------------------------
    // Backward compatibility: old JSON with numeric IDs deserializes into String
    // -----------------------------------------------------------------------

    #[test]
    fn backward_compat_old_json_numeric_server_id() {
        // Old state.json format had server_id as u64
        let json = r#"{"server_id":12345,"server_ip":"203.0.113.50","ssh_key_id":678,"status":"running"}"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert_eq!(state.server_id.as_deref(), Some("12345"));
        assert_eq!(state.server_ip.as_deref(), Some("203.0.113.50"));
        assert_eq!(state.ssh_key_id.as_deref(), Some("678"));
        assert!(matches!(state.status, Some(VpsStatus::Running)));
        // New fields default to None when missing from old format
        assert!(state.cloud_provider.is_none());
        assert!(state.server_type.is_none());
        assert!(state.location.is_none());
    }

    #[test]
    fn backward_compat_old_json_numeric_ssh_key_only() {
        // Partial old state: SSH key created but server not yet provisioned
        let json = r#"{"ssh_key_id":42,"status":"creating"}"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert!(state.server_id.is_none());
        assert_eq!(state.ssh_key_id.as_deref(), Some("42"));
        assert!(matches!(state.status, Some(VpsStatus::Creating)));
    }

    #[test]
    fn backward_compat_old_json_zero_server_id() {
        let json = r#"{"server_id":0,"server_ip":"0.0.0.0"}"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert_eq!(state.server_id.as_deref(), Some("0"));
        assert!(state.is_provisioned());
    }

    #[test]
    fn new_format_string_ids_roundtrip() {
        // New format: IDs are strings (e.g., DigitalOcean droplet IDs, UUIDs)
        let state = VpsState {
            server_id: Some("droplet-abc-123".to_string()),
            server_ip: Some("10.0.0.1".to_string()),
            ssh_key_id: Some("key-xyz-789".to_string()),
            status: Some(VpsStatus::Running),
            cloud_provider: Some("digitalocean".to_string()),
            server_type: Some("s-1vcpu-1gb".to_string()),
            location: Some("nyc1".to_string()),
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: VpsState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_id.as_deref(), Some("droplet-abc-123"));
        assert_eq!(deserialized.ssh_key_id.as_deref(), Some("key-xyz-789"));
        assert_eq!(
            deserialized.cloud_provider.as_deref(),
            Some("digitalocean")
        );
        assert_eq!(deserialized.server_type.as_deref(), Some("s-1vcpu-1gb"));
        assert_eq!(deserialized.location.as_deref(), Some("nyc1"));
    }

    #[test]
    fn mixed_old_and_new_fields() {
        // Hypothetical: old numeric IDs + new fields present
        let json = r#"{
            "server_id": 99999,
            "server_ip": "10.0.0.1",
            "ssh_key_id": 42,
            "status": "running",
            "cloud_provider": "hetzner",
            "server_type": "cx23",
            "location": "nbg1"
        }"#;
        let state: VpsState = serde_json::from_str(json).unwrap();
        assert_eq!(state.server_id.as_deref(), Some("99999"));
        assert_eq!(state.ssh_key_id.as_deref(), Some("42"));
        assert_eq!(state.cloud_provider.as_deref(), Some("hetzner"));
        assert_eq!(state.server_type.as_deref(), Some("cx23"));
        assert_eq!(state.location.as_deref(), Some("nbg1"));
    }
}
