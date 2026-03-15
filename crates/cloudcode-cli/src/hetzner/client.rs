use anyhow::{bail, Context, Result};
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

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Failed to create SSH key (HTTP {status}): {text}");
        }

        let data: CreateSshKeyResponse = resp.json().await.context("Failed to parse SSH key response")?;
        Ok(data.ssh_key.id)
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

        let data: CreateServerResponse =
            resp.json().await.context("Failed to parse server response")?;
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

        let data: GetServerResponse =
            resp.json().await.context("Failed to parse server response")?;
        Ok(ServerInfo {
            id: data.server.id,
            name: data.server.name,
            status: data.server.status,
            ip: data.server.public_net.ipv4.ip,
        })
    }
}
