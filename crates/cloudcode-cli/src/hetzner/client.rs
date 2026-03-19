use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub ip: String,
}

pub struct HetznerClient {
    client: reqwest::Client,
    api_token: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct ServersResponse {
    #[allow(dead_code)]
    servers: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct CreateSshKeyRequest {
    name: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
struct CreateSshKeyResponse {
    ssh_key: SshKeyData,
}

#[derive(Debug, Deserialize)]
struct SshKeyData {
    id: u64,
}

#[derive(Debug, Serialize)]
struct CreateServerRequest {
    name: String,
    server_type: String,
    image: String,
    location: String,
    ssh_keys: Vec<u64>,
    user_data: String,
}

#[derive(Debug, Deserialize)]
struct CreateServerResponse {
    server: ServerData,
}

#[derive(Debug, Deserialize)]
struct ServerData {
    id: u64,
    name: String,
    status: String,
    public_net: PublicNet,
}

#[derive(Debug, Deserialize)]
struct PublicNet {
    ipv4: Ipv4Info,
}

#[derive(Debug, Deserialize)]
struct Ipv4Info {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct GetServerResponse {
    server: ServerData,
}

pub fn estimate_monthly_cost(server_type: &str) -> Option<f64> {
    // Prices from Hetzner API as of 2026-03. The TUI server picker fetches
    // live prices; these are fallbacks for status display and quick estimates.
    match server_type {
        // Shared x86 (CX)
        "cx23" => Some(3.49),
        "cx33" => Some(5.99),
        "cx43" => Some(9.99),
        "cx53" => Some(18.99),
        // Shared ARM (CAX)
        "cax11" => Some(3.99),
        "cax21" => Some(6.99),
        "cax31" => Some(13.49),
        "cax41" => Some(26.99),
        // Shared AMD (CPX)
        "cpx11" => Some(4.49),
        "cpx12" => Some(4.49),
        "cpx21" => Some(7.99),
        "cpx22" => Some(6.99),
        "cpx31" => Some(14.99),
        "cpx32" => Some(11.99),
        "cpx41" => Some(27.49),
        "cpx42" => Some(21.99),
        "cpx51" => Some(60.49),
        "cpx52" => Some(31.49),
        "cpx62" => Some(42.99),
        // Dedicated (CCX)
        "ccx13" => Some(13.49),
        "ccx23" => Some(26.49),
        "ccx33" => Some(53.49),
        "ccx43" => Some(106.99),
        "ccx53" => Some(213.49),
        "ccx63" => Some(319.99),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerTypeInfo {
    pub name: String,
    pub description: String,
    pub cores: u32,
    pub memory: f64,
    pub disk: u64,
    /// Locations where this server type is available.
    pub available_locations: Vec<String>,
    /// Monthly price (gross) for a specific location, if known.
    pub monthly_price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServerTypesResponse {
    server_types: Vec<ServerTypeRaw>,
}

#[derive(Debug, Deserialize)]
struct ServerTypeRaw {
    name: String,
    description: String,
    cores: u32,
    memory: f64,
    disk: u64,
    #[serde(default)]
    deprecation: Option<serde_json::Value>,
    #[serde(default)]
    prices: Vec<ServerTypePrice>,
}

#[derive(Debug, Deserialize)]
struct ServerTypePrice {
    location: String,
    price_monthly: PriceDetail,
}

#[derive(Debug, Deserialize)]
struct PriceDetail {
    gross: String,
}

impl HetznerClient {
    pub fn new(api_token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_token,
            base_url: "https://api.hetzner.cloud/v1".to_string(),
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_token)
    }

    /// List available server types, filtering out deprecated ones.
    /// If `location` is provided, includes pricing for that location and marks availability.
    pub async fn list_server_types(&self, location: Option<&str>) -> Result<Vec<ServerTypeInfo>> {
        let resp = self
            .client
            .get(format!("{}/server_types?per_page=50", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to fetch server types")?;

        if !resp.status().is_success() {
            bail!("Failed to list server types (HTTP {})", resp.status());
        }

        let data: ServerTypesResponse = resp
            .json()
            .await
            .context("Failed to parse server types response")?;

        let types: Vec<ServerTypeInfo> = data
            .server_types
            .into_iter()
            .filter(|t| t.deprecation.is_none())
            .map(|t| {
                let available_locations: Vec<String> =
                    t.prices.iter().map(|p| p.location.clone()).collect();
                let monthly_price = location.and_then(|loc| {
                    t.prices
                        .iter()
                        .find(|p| p.location == loc)
                        .map(|p| p.price_monthly.gross.clone())
                });
                ServerTypeInfo {
                    name: t.name,
                    description: t.description,
                    cores: t.cores,
                    memory: t.memory,
                    disk: t.disk,
                    available_locations,
                    monthly_price,
                }
            })
            .collect();

        Ok(types)
    }

    pub async fn validate_token(&self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}/servers", self.base_url))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to connect to Hetzner API")?;

        if !resp.status().is_success() {
            bail!(
                "Hetzner API token validation failed (HTTP {})",
                resp.status()
            );
        }

        // Parse to confirm valid JSON response
        resp.json::<ServersResponse>()
            .await
            .context("Invalid response from Hetzner API")?;

        Ok(())
    }

    pub async fn create_ssh_key(&self, name: &str, public_key: &str) -> Result<u64> {
        let body = CreateSshKeyRequest {
            name: name.to_string(),
            public_key: public_key.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/ssh_keys", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("Failed to create SSH key")?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            // Key already exists — find it by name and reuse
            return self.find_ssh_key_by_name(name).await;
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to create SSH key (HTTP {status}): {text}");
        }

        let data: CreateSshKeyResponse = resp
            .json()
            .await
            .context("Failed to parse SSH key response")?;
        Ok(data.ssh_key.id)
    }

    async fn find_ssh_key_by_name(&self, name: &str) -> Result<u64> {
        let resp = self
            .client
            .get(format!("{}/ssh_keys?name={}", self.base_url, name))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to list SSH keys")?;

        if !resp.status().is_success() {
            bail!("Failed to list SSH keys (HTTP {})", resp.status());
        }

        let data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse SSH keys response")?;
        let keys = data["ssh_keys"]
            .as_array()
            .context("Unexpected SSH keys response format")?;

        for key in keys {
            if key["name"].as_str() == Some(name) {
                return key["id"].as_u64().context("SSH key missing id field");
            }
        }

        bail!("SSH key '{}' exists but could not be found by name", name)
    }

    pub async fn delete_ssh_key(&self, id: u64) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/ssh_keys/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to delete SSH key")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to delete SSH key (HTTP {status}): {text}");
        }

        Ok(())
    }

    pub async fn create_server(
        &self,
        name: &str,
        server_type: &str,
        image: &str,
        location: &str,
        ssh_key_ids: Vec<u64>,
        user_data: &str,
    ) -> Result<(u64, String)> {
        let body = CreateServerRequest {
            name: name.to_string(),
            server_type: server_type.to_string(),
            image: image.to_string(),
            location: location.to_string(),
            ssh_keys: ssh_key_ids,
            user_data: user_data.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/servers", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("Failed to create server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to create server (HTTP {status}): {text}");
        }

        let data: CreateServerResponse = resp
            .json()
            .await
            .context("Failed to parse server response")?;
        let ip = data.server.public_net.ipv4.ip;
        Ok((data.server.id, ip))
    }

    pub async fn delete_server(&self, id: u64) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/servers/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to delete server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to delete server (HTTP {status}): {text}");
        }

        Ok(())
    }

    pub async fn get_server(&self, id: u64) -> Result<ServerInfo> {
        let resp = self
            .client
            .get(format!("{}/servers/{}", self.base_url, id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Failed to get server info")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to get server (HTTP {status}): {text}");
        }

        let data: GetServerResponse = resp
            .json()
            .await
            .context("Failed to parse server response")?;
        Ok(ServerInfo {
            id: data.server.id,
            name: data.server.name,
            status: data.server.status,
            ip: data.server.public_net.ipv4.ip,
        })
    }
}
