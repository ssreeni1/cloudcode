use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ParseMode;

use super::bot::BotState;
use super::formatter;

pub async fn handle_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    // Owner-only filter
    if msg.chat.id != state.owner_id {
        bot.send_message(msg.chat.id, "⛔ Unauthorized.").await?;
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

            // Auto-list existing sessions on /start
            if let Ok(sessions) = state.session_mgr.list().await {
                if !sessions.is_empty() {
                    let default = state.default_session.current();
                    let mut session_list = String::from("📋 Existing sessions:\n");
                    for s in &sessions {
                        let marker = if default.as_deref() == Some(&s.name) {
                            " ← default"
                        } else {
                            ""
                        };
                        session_list.push_str(&format!("• {}{}\n", s.name, marker));
                    }
                    session_list.push_str("\nUse /use <name> to set a default session.");
                    bot.send_message(msg.chat.id, session_list).await?;
                }
            }
        }
        "/spawn" => {
            let name = if args.is_empty() {
                None
            } else {
                Some(args.to_string())
            };
            match state.session_mgr.spawn(name).await {
                Ok(session) => {
                    // Always set newly spawned session as default
                    if let Err(err) = state.default_session.set(Some(session.name.clone())) {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚠️ Session '{}' created, but failed to persist default session: {}",
                                session.name, err
                            ),
                        )
                        .await?;
                    }
                    bot.send_message(
                        msg.chat.id,
                        format!("✅ Session '{}' created and set as default. Send any message to start chatting.", session.name),
                    )
                    .await?;
                }
                Err(e) => {
                    let err_str = format!("{}", e);
                    if err_str.contains("auth")
                        || err_str.contains("login")
                        || err_str.contains("OAuth")
                    {
                        bot.send_message(
                            msg.chat.id,
                            "❌ OAuth login has not been completed on the VPS.\n\n\
                             Run `cloudcode open <session>` from your terminal to complete the login flow first.",
                        )
                        .await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                    }
                }
            }
        }
        "/list" => match state.session_mgr.list().await {
            Ok(sessions) => {
                if sessions.is_empty() {
                    bot.send_message(msg.chat.id, "No active sessions.").await?;
                } else {
                    let default = state.default_session.current();
                    let mut text = String::from("📋 Active sessions:\n");
                    for s in &sessions {
                        let marker = if default.as_deref() == Some(&s.name) {
                            " ← default"
                        } else {
                            ""
                        };
                        text.push_str(&format!(
                            "• {} [{}]{}\n",
                            s.name,
                            format!("{:?}", s.state),
                            marker
                        ));
                    }
                    bot.send_message(msg.chat.id, text).await?;
                }
            }
            Err(e) => {
                bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
            }
        },
        "/kill" => {
            if args.is_empty() {
                bot.send_message(msg.chat.id, "Usage: /kill <session-name>")
                    .await?;
            } else {
                match state.session_mgr.kill(args).await {
                    Ok(()) => {
                        // Clear default if this was the default session
                        if state.default_session.current().as_deref() == Some(args) {
                            if let Err(err) = state.default_session.clear() {
                                bot.send_message(
                                    msg.chat.id,
                                    format!(
                                        "⚠️ Session '{}' killed, but failed to clear persisted default session: {}",
                                        args, err
                                    ),
                                )
                                .await?;
                            }
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
                bot.send_message(msg.chat.id, "Usage: /use <session-name>")
                    .await?;
            } else {
                // Verify session exists
                match state.session_mgr.list().await {
                    Ok(sessions) => {
                        if sessions.iter().any(|s| s.name == args) {
                            if let Err(err) = state.default_session.set(Some(args.to_string())) {
                                bot.send_message(
                                    msg.chat.id,
                                    format!(
                                        "⚠️ Default session updated in memory, but failed to persist: {}",
                                        err
                                    ),
                                )
                                .await?;
                            }
                            bot.send_message(
                                msg.chat.id,
                                format!("✅ Default session set to '{}'.", args),
                            )
                            .await?;
                        } else {
                            bot.send_message(
                                msg.chat.id,
                                format!("❌ Session '{}' not found.", args),
                            )
                            .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                    }
                }
            }
        }
        "/status" => match state.session_mgr.list().await {
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
        },
        _ => {
            bot.send_message(
                msg.chat.id,
                "Unknown command. Send /help for available commands.",
            )
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
    let session_name = match state.default_session.current() {
        Some(name) => match state.session_mgr.list().await {
            Ok(sessions) if sessions.iter().any(|s| s.name == name) => Some(name),
            Ok(_) => {
                if let Err(err) = state.default_session.clear() {
                    log::warn!(
                        "Failed to clear stale persisted Telegram default session: {}",
                        err
                    );
                }
                None
            }
            Err(_) => Some(name),
        },
        None => None,
    };
    let session_name = match session_name {
        Some(name) => name,
        None => {
            // No default set — try to auto-resolve
            match state.session_mgr.list().await {
                Ok(sessions) if sessions.len() == 1 => {
                    // Exactly one session: auto-set it as default and proceed
                    let name = sessions[0].name.clone();
                    if let Err(err) = state.default_session.set(Some(name.clone())) {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚠️ Auto-selected session '{}', but failed to persist default session: {}",
                                name, err
                            ),
                        )
                        .await?;
                    }
                    bot.send_message(msg.chat.id, format!("📌 Auto-selected session '{}'.", name))
                        .await?;
                    name
                }
                Ok(sessions) if sessions.is_empty() => {
                    bot.send_message(
                        msg.chat.id,
                        "No sessions available. Use /spawn to create one.",
                    )
                    .await?;
                    return Ok(());
                }
                Ok(sessions) => {
                    // Multiple sessions — list them for the user
                    let mut list = String::from("No default session set. Available sessions:\n");
                    for s in &sessions {
                        list.push_str(&format!("• {}\n", s.name));
                    }
                    list.push_str("\nUse /use <name> to pick one.");
                    bot.send_message(msg.chat.id, list).await?;
                    return Ok(());
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
                    return Ok(());
                }
            }
        }
    };

    // Send periodic typing indicators (every 4 seconds) until send completes
    let typing_bot = bot.clone();
    let typing_chat_id = msg.chat.id;
    let typing_handle = tokio::spawn(async move {
        loop {
            let _ = typing_bot
                .send_chat_action(typing_chat_id, teloxide::types::ChatAction::Typing)
                .await;
            tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
        }
    });

    let send_result = state.session_mgr.send(&session_name, text).await;
    typing_handle.abort();

    match send_result {
        Ok(result) => {
            // Send text response
            if !result.text.is_empty() {
                let chunks = formatter::chunk_message(&result.text, 4096);
                for chunk in chunks {
                    if !chunk.trim().is_empty() {
                        bot.send_message(msg.chat.id, chunk).await?;
                    }
                }
            }

            // Send any files that were created
            for file_path in &result.files {
                let path = std::path::Path::new(file_path);
                if !path.exists() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");

                let file = teloxide::types::InputFile::file(path);
                if matches!(
                    ext.to_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp"
                ) {
                    // Send images as photos
                    bot.send_photo(msg.chat.id, file).caption(filename).await?;
                } else {
                    // Send everything else as documents
                    bot.send_document(msg.chat.id, file)
                        .caption(filename)
                        .await?;
                }
            }
        }
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("auth")
                || err_str.contains("login")
                || err_str.contains("OAuth")
                || err_str.contains("unauthorized")
                || err_str.contains("credentials")
                || err_str.contains("401")
            {
                bot.send_message(
                    msg.chat.id,
                    "❌ OAuth login has not been completed on the VPS.\n\n\
                     To fix this, run from your terminal:\n\
                     1. cloudcode spawn (or /spawn in TUI)\n\
                     2. cloudcode open <session-name>\n\
                     3. Complete the OAuth login flow in your browser\n\n\
                     Telegram will work once OAuth is complete.",
                )
                .await?;
            } else {
                bot.send_message(msg.chat.id, format!("❌ {}", e)).await?;
            }
        }
    }

    Ok(())
}
