use anyhow::Result;

use super::{CloudProvider, CreateServerOpts, ProvisionedServer, ServerTypeInfo};
use crate::hetzner::client::HetznerClient;

/// Wraps the existing `HetznerClient` behind the `CloudProvider` trait.
pub struct HetznerProvider {
    client: HetznerClient,
}

impl HetznerProvider {
    pub fn new(token: String) -> Self {
        Self {
            client: HetznerClient::new(token),
        }
    }
}

impl CloudProvider for HetznerProvider {
    async fn validate_credentials(&self) -> Result<()> {
        self.client.validate_token().await
    }

    async fn create_ssh_key(&self, name: &str, public_key: &str) -> Result<String> {
        let id = self.client.create_ssh_key(name, public_key).await?;
        Ok(id.to_string())
    }

    async fn delete_ssh_key(&self, id: &str) -> Result<()> {
        let id: u64 = id.parse().map_err(|_| anyhow::anyhow!("Invalid Hetzner SSH key ID: {id}"))?;
        self.client.delete_ssh_key(id).await
    }

    async fn create_server(&self, opts: CreateServerOpts) -> Result<ProvisionedServer> {
        let ssh_key_ids: Vec<u64> = opts
            .ssh_key_ids
            .iter()
            .map(|s| s.parse::<u64>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| anyhow::anyhow!("Invalid Hetzner SSH key IDs"))?;

        let (id, ip) = self
            .client
            .create_server(
                &opts.name,
                &opts.server_type,
                &opts.image,
                &opts.location,
                ssh_key_ids,
                &opts.user_data,
            )
            .await?;

        Ok(ProvisionedServer {
            id: id.to_string(),
            name: opts.name,
            status: "initializing".to_string(),
            ip,
        })
    }

    async fn delete_server(&self, id: &str) -> Result<()> {
        let id: u64 = id.parse().map_err(|_| anyhow::anyhow!("Invalid Hetzner server ID: {id}"))?;
        self.client.delete_server(id).await
    }

    async fn get_server(&self, id: &str) -> Result<ProvisionedServer> {
        let id: u64 = id.parse().map_err(|_| anyhow::anyhow!("Invalid Hetzner server ID: {id}"))?;
        let info = self.client.get_server(id).await?;
        Ok(ProvisionedServer {
            id: info.id.to_string(),
            name: info.name,
            status: info.status,
            ip: info.ip,
        })
    }

    async fn list_server_types(&self, location: Option<&str>) -> Result<Vec<ServerTypeInfo>> {
        let types = self.client.list_server_types(location).await?;
        Ok(types
            .into_iter()
            .map(|t| ServerTypeInfo {
                name: t.name,
                description: t.description,
                cores: t.cores,
                memory: t.memory,
                disk: t.disk,
                available_locations: t.available_locations,
                monthly_price: t.monthly_price,
            })
            .collect())
    }

    fn target_triple(&self, server_type: &str) -> &'static str {
        if server_type.starts_with("cax") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    }
}
