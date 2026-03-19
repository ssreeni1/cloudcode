use std::sync::Arc;

use cloudcode_common::provider::AiProvider;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ParseMode};

use super::bot::BotState;
use super::files::send_result_files;
use super::replies::{send_markdownish, send_preformatted, send_text};
use super::session_resolution::{
    FreeTextSessionTarget, ReplyTarget, clear_waiting_state, resolve_command_session,
    resolve_free_text_session, resolve_reply_target, resolve_type_target, session_exists,
    waiting_sessions,
};

const HELP_TEXT: &str = "🤖 *cloudcode Telegram Bot*\n\n\
    /spawn \\[name\\] — Create a new session\n\
    /list — List active sessions\n\
    /kill \\<name\\> — Kill a session\n\
    /use \\<name\\> — Set default session\n\
    /provider \\[claude\\|codex\\] — Check or switch AI provider\n\
    /waiting — List sessions waiting for input\n\
    /reply \\[session\\] \\<text\\> — Reply to a waiting session\n\
    /context \\[session\\] — View session context\n\
    /peek \\[session\\] — View raw tmux pane\n\
    /type \\[session\\] \\<text\\> — Type into tmux session\n\
    /status — Show daemon status\n\n\
    Waiting prompts are routed with /reply, not ordinary chat text\\.\n\
    Send any text to interact with the default session\\.";

pub async fn handle_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    if msg.chat.id != state.owner_id {
        bot.send_message(msg.chat.id, "⛔ Unauthorized.").await?;
        return Ok(());
    }

    let text = match msg.text() {
        Some(text) => text.to_string(),
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
    let args = parts.get(1).map(|value| value.trim()).unwrap_or("");

    match command.as_str() {
        "/start" | "/help" => handle_help(bot, msg, state).await?,
        "/spawn" => handle_spawn(bot, msg, state, args).await?,
        "/list" => handle_list(bot, msg, state).await?,
        "/kill" => handle_kill(bot, msg, state, args).await?,
        "/use" => handle_use(bot, msg, state, args).await?,
        "/status" => handle_status(bot, msg, state).await?,
        "/provider" => handle_provider(bot, msg, state, args).await?,
        "/waiting" => handle_waiting(bot, msg, state).await?,
        "/reply" => handle_reply(bot, msg, state, args).await?,
        "/context" => handle_context(bot, msg, state, args).await?,
        "/peek" => handle_peek(bot, msg, state, args).await?,
        "/type" => handle_type(bot, msg, state, args).await?,
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

async fn handle_help(bot: &Bot, msg: &Message, state: &Arc<BotState>) -> ResponseResult<()> {
    bot.send_message(msg.chat.id, HELP_TEXT)
        .parse_mode(ParseMode::MarkdownV2)
        .await?;

    if let Ok(sessions) = state.session_mgr.list().await {
        if !sessions.is_empty() {
            let default = state.default_session.current();
            let mut session_list = String::from("📋 Existing sessions:\n");
            for session in &sessions {
                let marker = if default.as_deref() == Some(&session.name) {
                    " ← default"
                } else {
                    ""
                };
                session_list.push_str(&format!("• {}{}\n", session.name, marker));
            }
            session_list.push_str("\nUse /use <name> to set a default session.");
            bot.send_message(msg.chat.id, session_list).await?;
        }
    }

    Ok(())
}

async fn handle_spawn(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    let name = if args.is_empty() {
        None
    } else {
        Some(args.to_string())
    };

    match state.session_mgr.spawn(name).await {
        Ok(session) => {
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
                format!(
                    "✅ Session '{}' created and set as default. Send any message to start chatting.",
                    session.name
                ),
            )
            .await?;
        }
        Err(err) => {
            if is_oauth_error(&err) {
                bot.send_message(
                    msg.chat.id,
                    "❌ OAuth login has not been completed on the VPS.\n\n\
                     Run `cloudcode open <session>` from your terminal to complete the login flow first.",
                )
                .await?;
            } else {
                bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
            }
        }
    }

    Ok(())
}

async fn handle_list(bot: &Bot, msg: &Message, state: &Arc<BotState>) -> ResponseResult<()> {
    match state.session_mgr.list().await {
        Ok(sessions) => {
            if sessions.is_empty() {
                bot.send_message(msg.chat.id, "No active sessions.").await?;
            } else {
                let default = state.default_session.current();
                let mut text = String::from("📋 Active sessions:\n");
                for session in &sessions {
                    let marker = if default.as_deref() == Some(&session.name) {
                        " ← default"
                    } else {
                        ""
                    };
                    text.push_str(&format!(
                        "• {} [{}]{}\n",
                        session.name,
                        format!("{:?}", session.state),
                        marker
                    ));
                }
                bot.send_message(msg.chat.id, text).await?;
            }
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_kill(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    if args.is_empty() {
        bot.send_message(msg.chat.id, "Usage: /kill <session-name>")
            .await?;
        return Ok(());
    }

    match state.session_mgr.kill(args).await {
        Ok(()) => {
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
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_use(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    if args.is_empty() {
        bot.send_message(msg.chat.id, "Usage: /use <session-name>")
            .await?;
        return Ok(());
    }

    match session_exists(state, args).await {
        Ok(true) => {
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
        }
        Ok(false) => {
            bot.send_message(msg.chat.id, format!("❌ Session '{}' not found.", args))
                .await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_provider(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    if args.is_empty() {
        let current = state.session_mgr.current_provider();
        let claude_status = provider_status(AiProvider::Claude);
        let codex_status = provider_status(AiProvider::Codex);
        let text = format!(
            "🤖 Current provider: {}\n\n\
             Claude: {}\n\
             Codex: {}\n\n\
             Use /provider claude or /provider codex to switch.",
            current.display_name(),
            claude_status.summary,
            codex_status.summary,
        );
        bot.send_message(msg.chat.id, text).await?;
    } else {
        let target: AiProvider = match args.parse() {
            Ok(p) => p,
            Err(_) => {
                bot.send_message(
                    msg.chat.id,
                    "Unknown provider. Use /provider claude or /provider codex",
                )
                .await?;
                return Ok(());
            }
        };

        let status = provider_status(target);
        if !status.switchable {
            bot.send_message(
                msg.chat.id,
                format!(
                    "❌ Cannot switch to {}: {}.\nRun `cloudcode init --reauth` to configure.",
                    target.display_name(),
                    status.reason
                ),
            )
            .await?;
            return Ok(());
        }

        state.session_mgr.set_provider(target);
        bot.send_message(
            msg.chat.id,
            format!(
                "✅ Switched to {}. New sessions will use this provider.",
                target.display_name()
            ),
        )
        .await?;
    }

    Ok(())
}

struct ProviderStatus {
    summary: &'static str,
    reason: &'static str,
    switchable: bool,
}

fn provider_status(provider: AiProvider) -> ProviderStatus {
    match provider {
        AiProvider::Claude => {
            let has_api_key = std::env::var("ANTHROPIC_API_KEY").is_ok();
            let has_oauth = std::path::Path::new("/home/claude/.claude/credentials.json").exists();
            match (has_api_key, has_oauth) {
                (true, _) | (_, true) => ProviderStatus {
                    summary: "✅ ready",
                    reason: "configured",
                    switchable: true,
                },
                (false, false) => ProviderStatus {
                    summary: "❌ not configured",
                    reason: "ANTHROPIC_API_KEY not set and Claude OAuth login not completed",
                    switchable: false,
                },
            }
        }
        AiProvider::Codex => {
            let binary_exists = std::path::Path::new("/usr/local/bin/codex").exists();
            let has_api_key = std::env::var("OPENAI_API_KEY").is_ok();
            let has_oauth = std::path::Path::new("/home/claude/.codex/auth.json").exists();
            let install_status =
                std::fs::read_to_string("/home/claude/.cloudcode/codex-status.json")
                    .unwrap_or_default();

            if install_status.contains("\"pending\"") || install_status.contains("\"installing\"") {
                return ProviderStatus {
                    summary: "⏳ installing",
                    reason: "Codex CLI install is still in progress",
                    switchable: false,
                };
            }

            if install_status.contains("\"failed\"") || !binary_exists {
                return ProviderStatus {
                    summary: "❌ not installed",
                    reason: "Codex CLI is not installed on the VPS yet",
                    switchable: false,
                };
            }

            if has_api_key || has_oauth {
                return ProviderStatus {
                    summary: "✅ ready",
                    reason: "configured",
                    switchable: true,
                };
            }

            let auth_method = std::fs::read_to_string("/home/claude/.cloudcode/codex-auth-method")
                .unwrap_or_default();
            let (summary, reason) = if auth_method.trim() == "oauth" {
                (
                    "⚠️ login required",
                    "Codex OAuth login has not been completed yet",
                )
            } else {
                ("❌ not configured", "OPENAI_API_KEY not set")
            };

            ProviderStatus {
                summary,
                reason,
                switchable: false,
            }
        }
    }
}

async fn handle_status(bot: &Bot, msg: &Message, state: &Arc<BotState>) -> ResponseResult<()> {
    match state.session_mgr.list().await {
        Ok(sessions) => {
            let text = format!(
                "📊 Status:\n• Sessions: {}\n• Daemon: running",
                sessions.len()
            );
            bot.send_message(msg.chat.id, text).await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_waiting(bot: &Bot, msg: &Message, state: &Arc<BotState>) -> ResponseResult<()> {
    let waiting = waiting_sessions(state);
    if waiting.is_empty() {
        bot.send_message(msg.chat.id, "No sessions are waiting for input.")
            .await?;
        return Ok(());
    }

    let mut text = String::from("⏳ Waiting sessions:\n");
    for (name, question) in waiting {
        let summary = question.lines().last().unwrap_or("").trim();
        text.push_str(&format!("• {} — {}\n", name, summary));
    }
    text.push_str("\nUse /reply <session> <text> to answer.");
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

async fn handle_reply(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    if args.is_empty() {
        bot.send_message(msg.chat.id, "Usage: /reply [session] <text>")
            .await?;
        return Ok(());
    }

    match resolve_reply_target(state, args).await {
        Ok(ReplyTarget::NoneWaiting) => {
            bot.send_message(msg.chat.id, "No sessions are currently waiting for input.")
                .await?;
        }
        Ok(ReplyTarget::Ready {
            session_name,
            reply_text,
        }) => match state
            .session_mgr
            .send_keys(&session_name, &reply_text)
            .await
        {
            Ok(()) => {
                clear_waiting_state(state, &session_name);
                bot.send_message(msg.chat.id, format!("✅ Replied to '{}'.", session_name))
                    .await?;
            }
            Err(err) => {
                bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
            }
        },
        Ok(ReplyTarget::Ambiguous(waiting)) => {
            let mut text =
                String::from("Multiple sessions are waiting. Use /reply <session> <text>.\n");
            for (name, question) in waiting {
                let summary = question.lines().last().unwrap_or("").trim();
                text.push_str(&format!("• {} — {}\n", name, summary));
            }
            bot.send_message(msg.chat.id, text).await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_context(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    let explicit = (!args.is_empty()).then_some(args);
    match resolve_command_session(state, explicit).await {
        Ok(Some(name)) => {
            let context_path = format!("/home/claude/.cloudcode/contexts/context_{}.md", name);
            match tokio::fs::read_to_string(&context_path).await {
                Ok(content) if content.trim().is_empty() => {
                    bot.send_message(
                        msg.chat.id,
                        format!("Context file for '{}' is empty.", name),
                    )
                    .await?;
                }
                Ok(content) => {
                    send_markdownish(bot, msg.chat.id, &content).await?;
                }
                Err(_) => {
                    bot.send_message(
                        msg.chat.id,
                        format!("No context file for session '{}' yet.", name),
                    )
                    .await?;
                }
            }
        }
        Ok(None) => {
            bot.send_message(
                msg.chat.id,
                "No default session. Use /context <session> or /use <session> first.",
            )
            .await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_peek(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    let explicit = (!args.is_empty()).then_some(args);
    match resolve_command_session(state, explicit).await {
        Ok(Some(name)) => match state.session_mgr.capture_pane(&name).await {
            Ok(content) if content.trim().is_empty() => {
                bot.send_message(msg.chat.id, "(pane is empty)").await?;
            }
            Ok(content) => {
                send_preformatted(bot, msg.chat.id, &content).await?;
            }
            Err(err) => {
                bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
            }
        },
        Ok(None) => {
            bot.send_message(
                msg.chat.id,
                "No default session. Use /peek <session> or /use <session> first.",
            )
            .await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
        }
    }

    Ok(())
}

async fn handle_type(
    bot: &Bot,
    msg: &Message,
    state: &Arc<BotState>,
    args: &str,
) -> ResponseResult<()> {
    if args.is_empty() {
        bot.send_message(msg.chat.id, "Usage: /type [session] <text>")
            .await?;
        return Ok(());
    }

    match resolve_type_target(state, args).await {
        Ok(Some((name, text_to_type))) => {
            match state.session_mgr.send_keys(&name, &text_to_type).await {
                Ok(()) => {
                    clear_waiting_state(state, &name);
                    bot.send_message(msg.chat.id, format!("✅ Typed into '{}'.", name))
                        .await?;
                }
                Err(err) => {
                    bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
                }
            }
        }
        Ok(None) => {
            bot.send_message(
                msg.chat.id,
                "No default session. Use /type <session> <text> or /use <session> first.",
            )
            .await?;
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
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
    let session_name = match resolve_free_text_session(state).await {
        Ok(FreeTextSessionTarget::Selected {
            name,
            auto_selected,
        }) => {
            if auto_selected {
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
            }
            name
        }
        Ok(FreeTextSessionTarget::NoSessions) => {
            bot.send_message(
                msg.chat.id,
                "No sessions available. Use /spawn to create one.",
            )
            .await?;
            return Ok(());
        }
        Ok(FreeTextSessionTarget::MultipleSessions(sessions)) => {
            let mut list = String::from("No default session set. Available sessions:\n");
            for session in sessions {
                list.push_str(&format!("• {}\n", session));
            }
            list.push_str("\nUse /use <name> to pick one.");
            bot.send_message(msg.chat.id, list).await?;
            return Ok(());
        }
        Err(err) => {
            bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
            return Ok(());
        }
    };

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

    // Retry transient execution errors (rate limits, network blips) up to 3 times.
    // Keep the typing indicator alive between retries.
    let mut send_result = Err(anyhow::anyhow!("not started"));
    for attempt in 0..3 {
        send_result = state.session_mgr.send(&session_name, text).await;
        match &send_result {
            Ok(_) => break,
            Err(err) => {
                let err_str = err.to_string();
                // Don't retry non-transient errors
                if is_oauth_error(err)
                    || err_str.contains("does not exist")
                    || err_str.contains("timed out")
                {
                    break;
                }
                if attempt < 2 {
                    log::warn!(
                        "Send attempt {} failed ({}), retrying in 5s...",
                        attempt + 1,
                        err_str
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
    typing_handle.abort();

    match send_result {
        Ok(result) => {
            send_markdownish(bot, msg.chat.id, &result.text).await?;
            send_result_files(bot, msg.chat.id, &result.files).await?;
        }
        Err(err) => {
            if is_oauth_error(&err) {
                send_text(
                    bot,
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
                bot.send_message(msg.chat.id, format!("❌ {}", err)).await?;
            }
        }
    }

    Ok(())
}

fn is_oauth_error(err: &anyhow::Error) -> bool {
    let err_str = err.to_string();
    err_str.contains("auth")
        || err_str.contains("login")
        || err_str.contains("OAuth")
        || err_str.contains("unauthorized")
        || err_str.contains("credentials")
        || err_str.contains("401")
}
