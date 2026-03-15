/// Split a message into chunks that fit within Telegram's message size limit.
/// Tries to split on line boundaries when possible.
pub fn chunk_message(text: &str, max_size: usize) -> Vec<String> {
    if text.len() <= max_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_size {
            chunks.push(remaining.to_string());
            break;
        }

        // Try to find a newline to split on within the limit
        let split_at = find_split_point(remaining, max_size);
        let (chunk, rest) = remaining.split_at(split_at);
        chunks.push(chunk.to_string());

        // Skip the newline character if we split on one
        remaining = rest.strip_prefix('\n').unwrap_or(rest);
    }

    chunks
}

fn find_split_point(text: &str, max_size: usize) -> usize {
    // Look for the last newline within the limit
    if let Some(pos) = text[..max_size].rfind('\n') {
        return pos;
    }

    // No newline found — look for last space
    if let Some(pos) = text[..max_size].rfind(' ') {
        return pos;
    }

    // No good split point — force split at max_size
    max_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_returns_single_chunk() {
        let result = chunk_message("hello", 4096);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn empty_message_returns_single_empty_chunk() {
        let result = chunk_message("", 4096);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn message_exactly_at_limit() {
        let msg = "a".repeat(4096);
        let result = chunk_message(&msg, 4096);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn long_message_splits_on_newlines() {
        let line = "a".repeat(100);
        let msg = format!("{}\n{}\n{}", line, line, line);
        let result = chunk_message(&msg, 210);
        assert!(result.len() >= 2);
        for chunk in &result {
            assert!(chunk.len() <= 210);
        }
    }

    #[test]
    fn splits_on_space_when_no_newline() {
        let msg = format!("{} {}", "a".repeat(50), "b".repeat(50));
        let result = chunk_message(&msg, 60);
        assert!(result.len() >= 2);
        assert!(result[0].len() <= 60);
    }

    #[test]
    fn force_splits_when_no_good_boundary() {
        let msg = "a".repeat(200);
        let result = chunk_message(&msg, 100);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 100);
    }

    #[test]
    fn preserves_all_content() {
        let msg = "hello\nworld\nfoo\nbar\nbaz\nqux";
        let result = chunk_message(msg, 12);
        let joined = result.join("\n");
        assert_eq!(joined, msg);
    }
}
