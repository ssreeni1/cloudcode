use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::handlers;
use crate::session::manager::SessionManager;

pub async fn run(addr: &str, port: u16, session_mgr: Arc<SessionManager>) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", addr, port)).await?;
    eprintln!("Control server listening on {}:{}", addr, port);

    loop {
        let (stream, peer) = listener.accept().await?;
        eprintln!("Client connected from {}", peer);
        let mgr = Arc::clone(&session_mgr);

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str::<DaemonRequest>(&line) {
                    Ok(request) => handlers::handle(request, &mgr).await,
                    Err(e) => DaemonResponse::Error {
                        message: format!("Invalid request: {}", e),
                    },
                };

                let mut resp_json = serde_json::to_string(&response).unwrap_or_else(|e| {
                    serde_json::to_string(&DaemonResponse::Error {
                        message: format!("Serialization error: {}", e),
                    })
                    .unwrap()
                });
                resp_json.push('\n');

                if writer.write_all(resp_json.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}
