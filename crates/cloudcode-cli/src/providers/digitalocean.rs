use anyhow::Result;

use super::{CloudProvider, CreateServerOpts, ProvisionedServer, ServerTypeInfo};

/// DigitalOcean cloud provider implementation.
///
/// Stub — full implementation in a follow-up step.
pub struct DigitalOceanProvider {
    #[allow(dead_code)]
    token: String,
}

impl DigitalOceanProvider {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

impl CloudProvider for DigitalOceanProvider {
    async fn validate_credentials(&self) -> Result<()> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn create_ssh_key(&self, _name: &str, _public_key: &str) -> Result<String> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn delete_ssh_key(&self, _id: &str) -> Result<()> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn create_server(&self, _opts: CreateServerOpts) -> Result<ProvisionedServer> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn delete_server(&self, _id: &str) -> Result<()> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn get_server(&self, _id: &str) -> Result<ProvisionedServer> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    async fn list_server_types(&self, _location: Option<&str>) -> Result<Vec<ServerTypeInfo>> {
        anyhow::bail!("DigitalOcean provider not yet implemented")
    }

    fn target_triple(&self, _server_type: &str) -> &'static str {
        "x86_64-unknown-linux-gnu"
    }
}
