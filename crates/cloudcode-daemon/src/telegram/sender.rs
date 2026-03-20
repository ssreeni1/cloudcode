use anyhow::Result;
use std::path::Path;

use super::formatter;

/// Abstraction over Telegram message sending.
/// TeloxideSender wraps the teloxide Bot; ReqwestSender uses raw HTTP.
#[async_trait::async_trait]
pub trait TelegramSender: Send + Sync {
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<()>;
    async fn send_html(&self, chat_id: i64, html: &str) -> Result<()>;
    async fn send_photo(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()>;
    async fn send_document(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// TeloxideSender — wraps existing teloxide::Bot
// ---------------------------------------------------------------------------

pub struct TeloxideSender {
    bot: teloxide::Bot,
}

impl TeloxideSender {
    pub fn new(bot: teloxide::Bot) -> Self {
        Self { bot }
    }
}

#[async_trait::async_trait]
impl TelegramSender for TeloxideSender {
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<()> {
        use teloxide::prelude::*;
        self.bot
            .send_message(teloxide::types::ChatId(chat_id), text)
            .await
            .map_err(|e| anyhow::anyhow!("send_text failed: {}", e))?;
        Ok(())
    }

    async fn send_html(&self, chat_id: i64, html: &str) -> Result<()> {
        use teloxide::prelude::*;
        use teloxide::types::ParseMode;
        self.bot
            .send_message(teloxide::types::ChatId(chat_id), html)
            .parse_mode(ParseMode::Html)
            .await
            .map_err(|e| anyhow::anyhow!("send_html failed: {}", e))?;
        Ok(())
    }

    async fn send_photo(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()> {
        use teloxide::prelude::*;
        use teloxide::types::InputFile;
        self.bot
            .send_photo(
                teloxide::types::ChatId(chat_id),
                InputFile::file(path.to_path_buf()),
            )
            .caption(caption)
            .await
            .map_err(|e| anyhow::anyhow!("send_photo failed: {}", e))?;
        Ok(())
    }

    async fn send_document(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()> {
        use teloxide::prelude::*;
        use teloxide::types::InputFile;
        self.bot
            .send_document(
                teloxide::types::ChatId(chat_id),
                InputFile::file(path.to_path_buf()),
            )
            .caption(caption)
            .await
            .map_err(|e| anyhow::anyhow!("send_document failed: {}", e))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ReqwestSender — uses raw Telegram Bot API via reqwest (for channels mode)
// ---------------------------------------------------------------------------

pub struct ReqwestSender {
    client: reqwest::Client,
    base_url: String,
}

impl ReqwestSender {
    pub fn new(bot_token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: format!("https://api.telegram.org/bot{}", bot_token),
        }
    }
}

#[async_trait::async_trait]
impl TelegramSender for ReqwestSender {
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<()> {
        let chunks = formatter::chunk_message(text, 4096);
        for chunk in chunks {
            if chunk.trim().is_empty() {
                continue;
            }
            self.client
                .post(format!("{}/sendMessage", self.base_url))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                }))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }

    async fn send_html(&self, chat_id: i64, html: &str) -> Result<()> {
        let chunks = formatter::chunk_message(html, 4096);
        for chunk in chunks {
            if chunk.trim().is_empty() {
                continue;
            }
            let resp = self
                .client
                .post(format!("{}/sendMessage", self.base_url))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "parse_mode": "HTML",
                }))
                .send()
                .await?;
            if !resp.status().is_success() {
                // Fall back to plain text if HTML fails
                self.client
                    .post(format!("{}/sendMessage", self.base_url))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "text": chunk,
                    }))
                    .send()
                    .await?
                    .error_for_status()?;
            }
        }
        Ok(())
    }

    async fn send_photo(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()> {
        let file_bytes = tokio::fs::read(path).await?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo")
            .to_string();
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str("image/png")?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", caption.to_string())
            .part("photo", part);
        self.client
            .post(format!("{}/sendPhoto", self.base_url))
            .multipart(form)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn send_document(&self, chat_id: i64, path: &Path, caption: &str) -> Result<()> {
        let file_bytes = tokio::fs::read(path).await?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", caption.to_string())
            .part("document", part);
        self.client
            .post(format!("{}/sendDocument", self.base_url))
            .multipart(form)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for sending via TelegramSender
// ---------------------------------------------------------------------------

/// Send text with markdown-to-HTML conversion, falling back to plain text.
pub async fn send_markdownish(sender: &dyn TelegramSender, chat_id: i64, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let html = formatter::markdown_to_html(text);
    if sender.send_html(chat_id, &html).await.is_err() {
        sender.send_text(chat_id, text).await?;
    }
    Ok(())
}

/// Send preformatted text wrapped in <pre> tags.
pub async fn send_preformatted(
    sender: &dyn TelegramSender,
    chat_id: i64,
    text: &str,
) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let escaped = formatter::escape_html(text);
    let html = format!("<pre>{}</pre>", escaped);
    if sender.send_html(chat_id, &html).await.is_err() {
        sender.send_text(chat_id, text).await?;
    }
    Ok(())
}

/// Send files from a result set (images as photos, others as documents).
pub async fn send_result_files(
    sender: &dyn TelegramSender,
    chat_id: i64,
    files: &[std::path::PathBuf],
) -> Result<()> {
    for file_path in files {
        if !file_path.exists() {
            continue;
        }

        let file_size = file_path.metadata().map(|m| m.len()).unwrap_or(0);
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let is_image = matches!(
            ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp"
        );
        let max_size = if is_image {
            10 * 1024 * 1024
        } else {
            50 * 1024 * 1024
        };

        if file_size > max_size {
            log::warn!(
                "Skipping file {} ({} bytes, exceeds limit)",
                filename,
                file_size
            );
            let _ = sender
                .send_text(
                    chat_id,
                    &format!(
                        "⚠️ Skipped {}: file too large ({:.1}MB)",
                        filename,
                        file_size as f64 / 1_048_576.0
                    ),
                )
                .await;
            continue;
        }

        let result = if is_image {
            sender.send_photo(chat_id, file_path, filename).await
        } else {
            sender.send_document(chat_id, file_path, filename).await
        };

        if let Err(err) = result {
            log::error!("Failed to send file {}: {}", filename, err);
            let _ = sender
                .send_text(chat_id, &format!("⚠️ Failed to send {}: {}", filename, err))
                .await;
        }
    }
    Ok(())
}
