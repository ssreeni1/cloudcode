pub mod digitalocean;
pub mod hetzner;

use anyhow::Result;

/// Options for creating a new cloud server.
pub struct CreateServerOpts {
    pub name: String,
    pub server_type: String,
    pub image: String,
    pub location: String,
    pub ssh_key_ids: Vec<String>,
    pub user_data: String,
}

/// A server that has been provisioned by a cloud provider.
pub struct ProvisionedServer {
    pub id: String,
    pub name: String,
    pub status: String,
    pub ip: String,
}

/// Abstraction over cloud VPS providers (Hetzner, DigitalOcean, etc.).
///
/// Uses native async fn in trait (Rust 2024 edition) — no `async-trait` crate needed.
pub trait CloudProvider: Send + Sync {
    /// Validate that the stored API credentials are valid.
    async fn validate_credentials(&self) -> Result<()>;

    /// Upload an SSH public key, returning the provider's key ID.
    async fn create_ssh_key(&self, name: &str, public_key: &str) -> Result<String>;

    /// Delete an SSH key by its provider ID.
    async fn delete_ssh_key(&self, id: &str) -> Result<()>;

    /// Create a new server with the given options, returning the provisioned server.
    async fn create_server(&self, opts: CreateServerOpts) -> Result<ProvisionedServer>;

    /// Delete a server by its provider ID.
    async fn delete_server(&self, id: &str) -> Result<()>;

    /// Get info about an existing server.
    async fn get_server(&self, id: &str) -> Result<ProvisionedServer>;

    /// List available server types/sizes for a given location.
    async fn list_server_types(&self, location: Option<&str>) -> Result<Vec<ServerTypeInfo>>;

    /// The Rust target triple for binaries on this provider's servers (e.g. `x86_64-unknown-linux-gnu`).
    fn target_triple(&self, server_type: &str) -> &'static str;
}

/// Generic server type info shared across providers.
pub struct ServerTypeInfo {
    pub name: String,
    pub description: String,
    pub cores: u32,
    pub memory: f64,
    pub disk: u64,
    pub available_locations: Vec<String>,
    pub monthly_price: Option<String>,
}

/// Enum dispatch for cloud providers. Avoids dyn-compatibility issues with async trait methods.
pub enum CloudProviderKind {
    Hetzner(hetzner::HetznerProvider),
    DigitalOcean(digitalocean::DigitalOceanProvider),
}

impl CloudProviderKind {
    pub async fn validate_credentials(&self) -> Result<()> {
        match self {
            Self::Hetzner(p) => p.validate_credentials().await,
            Self::DigitalOcean(p) => p.validate_credentials().await,
        }
    }

    pub async fn create_ssh_key(&self, name: &str, public_key: &str) -> Result<String> {
        match self {
            Self::Hetzner(p) => p.create_ssh_key(name, public_key).await,
            Self::DigitalOcean(p) => p.create_ssh_key(name, public_key).await,
        }
    }

    pub async fn delete_ssh_key(&self, id: &str) -> Result<()> {
        match self {
            Self::Hetzner(p) => p.delete_ssh_key(id).await,
            Self::DigitalOcean(p) => p.delete_ssh_key(id).await,
        }
    }

    pub async fn create_server(&self, opts: CreateServerOpts) -> Result<ProvisionedServer> {
        match self {
            Self::Hetzner(p) => p.create_server(opts).await,
            Self::DigitalOcean(p) => p.create_server(opts).await,
        }
    }

    pub async fn delete_server(&self, id: &str) -> Result<()> {
        match self {
            Self::Hetzner(p) => p.delete_server(id).await,
            Self::DigitalOcean(p) => p.delete_server(id).await,
        }
    }

    pub async fn get_server(&self, id: &str) -> Result<ProvisionedServer> {
        match self {
            Self::Hetzner(p) => p.get_server(id).await,
            Self::DigitalOcean(p) => p.get_server(id).await,
        }
    }

    pub async fn list_server_types(&self, location: Option<&str>) -> Result<Vec<ServerTypeInfo>> {
        match self {
            Self::Hetzner(p) => p.list_server_types(location).await,
            Self::DigitalOcean(p) => p.list_server_types(location).await,
        }
    }

    pub fn target_triple(&self, server_type: &str) -> &'static str {
        match self {
            Self::Hetzner(p) => p.target_triple(server_type),
            Self::DigitalOcean(p) => p.target_triple(server_type),
        }
    }
}

/// Factory function: construct a CloudProviderKind from a kind string and API token.
pub fn cloud_provider(kind: &str, token: String) -> Result<CloudProviderKind> {
    match kind {
        "hetzner" => Ok(CloudProviderKind::Hetzner(hetzner::HetznerProvider::new(token))),
        "digitalocean" | "do" => Ok(CloudProviderKind::DigitalOcean(
            digitalocean::DigitalOceanProvider::new(token),
        )),
        _ => anyhow::bail!("Unknown cloud provider: {kind}"),
    }
}
