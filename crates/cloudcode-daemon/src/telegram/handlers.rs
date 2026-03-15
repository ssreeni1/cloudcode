use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ParseMode;

use super::bot::BotState;
use super::formatter;

pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    // Owner-only filter
    if msg.chat.id != state.owner_id {
        bot.send_message(msg.chat.id, "⛔ Unauthorized.")
            .await?;
        return Ok(());
    }

    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    if text.starts_with('/') {
        handle_command(&bot, &msg, &state, &text).await
    } else {
        handle_free_text(&bot, &msg, &state, &text).await
    }
}

async fn handle_command(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match command.as_str() {
        "/start" | "/help" => {
            let help_text = "🤖 *cloudcode Telegram Bot*\n\n\
                /spawn \\[name\\] — Create a new session\n\
                /list — List active sessions\n\
                /kill \\<name\\> — Kill a session\n\
                /use \\<name\\> — Set default session\n\
                /status — Show daemon status\n\n\
                Send any text to interact with the default session\\.";
            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
        }
        "/spawn" => {
            let name = if args.is_empty() { None } else { Some(args.to_string()) };
            match state.session_mgr.spawn(name).await {
                Ok(session) => {
                    // Auto-set as default if no default exists
                    let mut default = state.default_session.lock().await;
                    if default.is_none() {
                        *default = Some(session.name.clone());
                    }
                    bot.send_message(
                        msg.chat.id,
                        format!("✅ Session '{}' created.", session.name),
                    )
                    .await?;
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                }
            }
        }
        "/list" => {
            match state.session_mgr.list().await {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        bot.send_message(msg.chat.id, "No active sessions.").await?;
                    } else {
                        let default = state.default_session.lock().await;
                        let mut text = String::from("📋 Active sessions:\n");
                        for s in &sessions {
                            let marker = if default.as_deref() == Some(&s.name) { " ← default" } else { "" };
                            text.push_str(&format!("• {} [{}]{}\n", s.name, format!("{:?}", s.state), marker));
                        }
                        bot.send_message(msg.chat.id, text).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                }
            }
        }
        "/kill" => {
            if args.is_empty() {
                bot.send_message(msg.chat.id, "Usage: /kill <session-name>").await?;
            } else {
                match state.session_mgr.kill(args).await {
                    Ok(()) => {
                        // Clear default if this was the default session
                        let mut default = state.default_session.lock().await;
                        if default.as_deref() == Some(args) {
                            *default = None;
                        }
                        bot.send_message(msg.chat.id, format!("✅ Session '{}' killed.", args))
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                    }
                }
            }
        }
        "/use" => {
            if args.is_empty() {
                bot.send_message(msg.chat.id, "Usage: /use <session-name>").await?;
            } else {
                // Verify session exists
                match state.session_mgr.list().await {
                    Ok(sessions) => {
                        if sessions.iter().any(|s| s.name == args) {
                            let mut default = state.default_session.lock().await;
                            *default = Some(args.to_string());
                            bot.send_message(msg.chat.id, format!("✅ Default session set to '{}'.", args))
                                .await?;
                        } else {
                            bot.send_message(msg.chat.id, format!("❌ Session '{}' not found.", args))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                    }
                }
            }
        }
        "/status" => {
            match state.session_mgr.list().await {
                Ok(sessions) => {
                    let text = format!(
                        "📊 Status:\n• Sessions: {}\n• Daemon: running",
                        sessions.len()
                    );
                    bot.send_message(msg.chat.id, text).await?;
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                }
            }
        }
        _ => {
            bot.send_message(msg.chat.id, "Unknown command. Send /help for available commands.")
                .await?;
        }
    }

    Ok(())
}

async fn handle_free_text(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    let default = state.default_session.lock().await;
    let session_name = match default.as_deref() {
        Some(name) => name.to_string(),
        None => {
            bot.send_message(
                msg.chat.id,
                "No default session set. Use /spawn to create one or /use <name> to set one.",
            )
            .await?;
            return Ok(());
        }
    };
    drop(default); // Release the lock before the potentially long send operation

    // Send typing indicator
    bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
        .await?;

    match state.session_mgr.send(&session_name, text).await {
        Ok(output) => {
            // Chunk and send the output
            let chunks = formatter::chunk_message(&output, 4096);
            for chunk in chunks {
                if !chunk.trim().is_empty() {
                    bot.send_message(msg.chat.id, chunk).await?;
                }
            }
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
        }
    }

    Ok(())
}
