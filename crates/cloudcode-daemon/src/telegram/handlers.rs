use std::sync::Arc;

use cloudcode_common::provider::AiProvider;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ParseMode};

use super::bot::BotState;
use super::dispatch::{self, DaemonState, DispatchResult, MessageFormat};
use super::files::send_result_files;
use super::replies::{send_markdownish, send_preformatted, send_text};

pub async fn handle_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    // Auth fix: reject non-private chats and check sender user_id.
    let is_private = matches!(msg.chat.kind, teloxide::types::ChatKind::Private(_));
    if !is_private {
        bot.send_message(msg.chat.id, "⛔ Private chat only.")
            .await?;
        return Ok(());
    }

    // Check sender user_id if available (protects against forwarded messages)
    if let Some(from) = &msg.from {
        if teloxide::types::ChatId(from.id.0 as i64) != state.owner_id {
            bot.send_message(msg.chat.id, "⛔ Unauthorized.").await?;
            return Ok(());
        }
    }

    // Also verify chat_id matches owner
    if msg.chat.id != state.owner_id {
        bot.send_message(msg.chat.id, "⛔ Unauthorized.").await?;
        return Ok(());
    }

    let text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    // Build DaemonState from BotState for dispatch logic
    let daemon_state = DaemonState {
        session_mgr: Arc::clone(&state.session_mgr),
        default_session: Arc::clone(&state.default_session),
        question_states: state.question_states.clone(),
    };

    if text.starts_with('/') {
        let result = dispatch::route_command(&daemon_state, &text).await;
        send_dispatch_result(&bot, &msg, &result).await?;
    } else {
        // Show typing indicator during free text processing
        let typing_bot = bot.clone();
        let typing_chat_id = msg.chat.id;
        let typing_handle = tokio::spawn(async move {
            loop {
                let _ = typing_bot
                    .send_chat_action(typing_chat_id, ChatAction::Typing)
                    .await;
                tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
            }
        });

        let result = dispatch::free_text_logic(&daemon_state, &text).await;
        typing_handle.abort();

        // For free text with auto-select, send the auto-select notification
        // This is handled inside the dispatch result messages
        send_dispatch_result(&bot, &msg, &result).await?;
    }

    Ok(())
}

/// Send a DispatchResult back via teloxide Bot
async fn send_dispatch_result(
    bot: &Bot,
    msg: &Message,
    result: &DispatchResult,
) -> ResponseResult<()> {
    for dispatch_msg in &result.messages {
        match dispatch_msg.format {
            MessageFormat::Plain => {
                send_text(bot, msg.chat.id, &dispatch_msg.text).await?;
            }
            MessageFormat::Html => {
                bot.send_message(msg.chat.id, &dispatch_msg.text)
                    .parse_mode(ParseMode::Html)
                    .await?;
            }
            MessageFormat::Markdown => {
                send_markdownish(bot, msg.chat.id, &dispatch_msg.text).await?;
            }
            MessageFormat::Preformatted => {
                send_preformatted(bot, msg.chat.id, &dispatch_msg.text).await?;
            }
        }
    }

    if !result.files.is_empty() {
        send_result_files(bot, msg.chat.id, &result.files).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Auth/provider helpers (pub for use by dispatch.rs)
// ---------------------------------------------------------------------------

pub(crate) struct ProviderStatus {
    pub summary: String,
    pub reason: String,
    pub switchable: bool,
}

pub(crate) fn provider_has_auth(provider: AiProvider) -> bool {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/claude".to_string());
    let meta = provider.meta();
    meta.auth_env_vars.iter().any(|var| std::env::var(var).is_ok())
        || meta.auth_files.iter().any(|f| {
            std::path::Path::new(&format!("{}/{}", home, f)).exists()
        })
}

pub(crate) fn provider_status(provider: AiProvider) -> ProviderStatus {
    match provider {
        AiProvider::Claude => {
            if provider_has_auth(provider) {
                ProviderStatus {
                    summary: "✅ ready".into(),
                    reason: "configured".into(),
                    switchable: true,
                }
            } else {
                ProviderStatus {
                    summary: "❌ not configured".into(),
                    reason: "ANTHROPIC_API_KEY not set and Claude OAuth login not completed".into(),
                    switchable: false,
                }
            }
        }
        AiProvider::Codex => {
            let binary_exists = std::path::Path::new("/usr/local/bin/codex").exists();
            let install_status =
                std::fs::read_to_string("/home/claude/.cloudcode/codex-status.json")
                    .unwrap_or_default();

            if install_status.contains("\"pending\"") || install_status.contains("\"installing\"") {
                return ProviderStatus {
                    summary: "⏳ installing".into(),
                    reason: "Codex CLI install is still in progress".into(),
                    switchable: false,
                };
            }

            if install_status.contains("\"failed\"") || !binary_exists {
                return ProviderStatus {
                    summary: "❌ not installed".into(),
                    reason: "Codex CLI is not installed on the VPS yet".into(),
                    switchable: false,
                };
            }

            if provider_has_auth(provider) {
                return ProviderStatus {
                    summary: "✅ ready".into(),
                    reason: "configured".into(),
                    switchable: true,
                };
            }

            let auth_method = std::fs::read_to_string("/home/claude/.cloudcode/codex-auth-method")
                .unwrap_or_default();
            let (summary, reason): (String, String) = if auth_method.trim() == "oauth" {
                (
                    "⚠️ login required".into(),
                    "Codex OAuth login has not been completed yet".into(),
                )
            } else {
                ("❌ not configured".into(), "OPENAI_API_KEY not set".into())
            };

            ProviderStatus {
                summary,
                reason,
                switchable: false,
            }
        }
        // New providers: generic status based on meta()
        _ => {
            let meta = provider.meta();
            let home = std::env::var("HOME").unwrap_or_else(|_| "/home/claude".to_string());
            let status_file = format!("{}/.cloudcode/{}-status.json", home, provider.as_str());
            let install_status = std::fs::read_to_string(&status_file).unwrap_or_default();

            if install_status.contains("\"pending\"") || install_status.contains("\"installing\"") {
                return ProviderStatus {
                    summary: "⏳ installing".into(),
                    reason: format!("{} CLI install is still in progress", meta.display_name),
                    switchable: false,
                };
            }

            if install_status.contains("\"failed\"") {
                return ProviderStatus {
                    summary: "❌ not installed".into(),
                    reason: format!("{} CLI is not installed on the VPS yet", meta.display_name),
                    switchable: false,
                };
            }

            if provider_has_auth(provider) {
                ProviderStatus {
                    summary: "✅ ready".into(),
                    reason: "configured".into(),
                    switchable: true,
                }
            } else if !meta.stable {
                ProviderStatus {
                    summary: "⚠️ experimental".into(),
                    reason: format!("{} headless auth is unconfirmed", meta.display_name),
                    switchable: false,
                }
            } else {
                ProviderStatus {
                    summary: "❌ not configured".into(),
                    reason: format!("{} auth not detected", meta.display_name),
                    switchable: false,
                }
            }
        }
    }
}

/// Check if an error is likely caused by missing authentication.
pub(crate) fn is_auth_error(
    err: &anyhow::Error,
    _provider: cloudcode_common::provider::AiProvider,
) -> bool {
    let err_str = err.to_string().to_lowercase();

    err_str.contains("auth")
        || err_str.contains("login")
        || err_str.contains("oauth")
        || err_str.contains("unauthorized")
        || err_str.contains("credentials")
        || err_str.contains("401")
        || err_str.contains("not logged in")
        || err_str.contains("api key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn auth_error_detects_explicit_login_failures() {
        let err = anyhow!("codex failed: Not logged in. Run `codex login`.");
        assert!(is_auth_error(&err, AiProvider::Codex));
    }

    #[test]
    fn auth_error_detects_api_key_failures() {
        let err = anyhow!("claude failed: API key missing");
        assert!(is_auth_error(&err, AiProvider::Claude));
    }

    #[test]
    fn auth_error_does_not_mask_generic_provider_failures() {
        let err = anyhow!("codex failed: connection reset by peer");
        assert!(!is_auth_error(&err, AiProvider::Codex));
    }
}
