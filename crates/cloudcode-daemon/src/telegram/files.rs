use std::path::{Path, PathBuf};

use teloxide::prelude::*;
use teloxide::types::{ChatId, InputFile};

fn is_image_extension(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp"
    )
}

pub async fn send_result_files(
    bot: &Bot,
    chat_id: ChatId,
    files: &[PathBuf],
) -> ResponseResult<()> {
    for file_path in files {
        let path = Path::new(file_path);
        if !path.exists() {
            continue;
        }

        let file_size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("file");
        let is_image = is_image_extension(ext);
        let max_size = if is_image {
            10 * 1024 * 1024
        } else {
            50 * 1024 * 1024
        };

        if file_size > max_size {
            log::warn!(
                "Skipping file {} ({} bytes, exceeds {} limit)",
                filename,
                file_size,
                if is_image {
                    "10MB photo"
                } else {
                    "50MB document"
                }
            );
            let _ = bot
                .send_message(
                    chat_id,
                    format!(
                        "⚠️ Skipped {}: file too large ({:.1}MB)",
                        filename,
                        file_size as f64 / 1_048_576.0
                    ),
                )
                .await;
            continue;
        }

        log::info!(
            "Sending file to Telegram: {} ({} bytes)",
            filename,
            file_size
        );
        let file = InputFile::file(path.to_path_buf());
        let send_result = if is_image {
            bot.send_photo(chat_id, file)
                .caption(filename)
                .await
                .map(|_| ())
        } else {
            bot.send_document(chat_id, file)
                .caption(filename)
                .await
                .map(|_| ())
        };

        if let Err(err) = send_result {
            log::error!("Failed to send file {}: {}", filename, err);
            let _ = bot
                .send_message(chat_id, format!("⚠️ Failed to send {}: {}", filename, err))
                .await;
        }
    }

    Ok(())
}
