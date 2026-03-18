use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode};

use super::formatter;

pub async fn send_text(bot: &Bot, chat_id: ChatId, text: impl Into<String>) -> ResponseResult<()> {
    bot.send_message(chat_id, text.into()).await?;
    Ok(())
}

pub async fn send_chunked_plain(bot: &Bot, chat_id: ChatId, text: &str) -> ResponseResult<()> {
    for chunk in formatter::chunk_message(text, 4096) {
        if !chunk.trim().is_empty() {
            bot.send_message(chat_id, chunk).await?;
        }
    }
    Ok(())
}

pub async fn send_markdownish(bot: &Bot, chat_id: ChatId, text: &str) -> ResponseResult<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    let html = formatter::markdown_to_html(text);
    for chunk in formatter::chunk_message(&html, 4096) {
        if chunk.trim().is_empty() {
            continue;
        }

        if bot
            .send_message(chat_id, &chunk)
            .parse_mode(ParseMode::Html)
            .await
            .is_err()
        {
            return send_chunked_plain(bot, chat_id, text).await;
        }
    }

    Ok(())
}

pub async fn send_preformatted(bot: &Bot, chat_id: ChatId, text: &str) -> ResponseResult<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    let escaped = formatter::escape_html(text);
    let html = format!("<pre>{}</pre>", escaped);
    for chunk in formatter::chunk_message(&html, 4096) {
        if chunk.trim().is_empty() {
            continue;
        }

        if bot
            .send_message(chat_id, &chunk)
            .parse_mode(ParseMode::Html)
            .await
            .is_err()
        {
            return send_chunked_plain(bot, chat_id, text).await;
        }
    }

    Ok(())
}
