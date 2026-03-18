use anyhow::Result;

pub async fn run(no_wait: bool, server_type_override: Option<String>) -> Result<()> {
    crate::deploy::provision::run(no_wait, server_type_override).await
}
