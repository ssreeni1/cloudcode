use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};

use super::dispatch::{DaemonState, DispatchResult, MessageFormat, free_text_logic, route_command};
use super::sender::{TelegramSender, send_markdownish, send_preformatted, send_result_files};

#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    pub chat_id: i64,
    pub text: String,
}

#[derive(Debug, Serialize)]
struct DispatchResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct AppState {
    daemon_state: Arc<DaemonState>,
    sender: Arc<dyn TelegramSender>,
}

async fn handle_dispatch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DispatchRequest>,
) -> (StatusCode, Json<DispatchResponse>) {
    let result = if req.text.starts_with('/') {
        route_command(&state.daemon_state, &req.text).await
    } else {
        free_text_logic(&state.daemon_state, &req.text).await
    };

    if let Err(e) = send_dispatch_result(&*state.sender, req.chat_id, &result).await {
        log::error!("Failed to send dispatch result: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DispatchResponse {
                ok: false,
                error: Some(e.to_string()),
            }),
        );
    }

    (
        StatusCode::OK,
        Json(DispatchResponse {
            ok: true,
            error: None,
        }),
    )
}

async fn send_dispatch_result(
    sender: &dyn TelegramSender,
    chat_id: i64,
    result: &DispatchResult,
) -> anyhow::Result<()> {
    for msg in &result.messages {
        match msg.format {
            MessageFormat::Plain => sender.send_text(chat_id, &msg.text).await?,
            MessageFormat::Html => sender.send_html(chat_id, &msg.text).await?,
            MessageFormat::Markdown => send_markdownish(sender, chat_id, &msg.text).await?,
            MessageFormat::Preformatted => send_preformatted(sender, chat_id, &msg.text).await?,
        }
    }

    if !result.files.is_empty() {
        send_result_files(sender, chat_id, &result.files).await?;
    }

    Ok(())
}

/// Start the dispatch HTTP server on localhost:8789.
/// This is only started in channels mode.
pub async fn run(
    daemon_state: Arc<DaemonState>,
    sender: Arc<dyn TelegramSender>,
) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        daemon_state,
        sender,
    });

    let app = axum::Router::new()
        .route("/dispatch", post(handle_dispatch))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8789").await?;
    log::info!("Dispatch server listening on 127.0.0.1:8789");

    axum::serve(listener, app).await?;
    Ok(())
}
