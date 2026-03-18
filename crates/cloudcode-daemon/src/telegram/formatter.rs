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

/// Escape HTML special characters for Telegram HTML parse mode.
pub(crate) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Convert markdown-style text to Telegram HTML.
///
/// Handles:
/// - Fenced code blocks (```lang\n...\n```) → <pre><code>...</code></pre>
/// - Inline code (`...`) → <code>...</code>
/// - Bold (**...**) → <b>...</b>
///
/// All other text is HTML-escaped. This is intentionally simple —
/// only the patterns Claude commonly outputs are handled.
pub fn markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        // Fenced code block: ``` at start of line (or start of string)
        if c == '`' && chars.peek() == Some(&'`') {
            chars.next(); // consume second `
            if chars.peek() == Some(&'`') {
                chars.next(); // consume third `
                // Consume optional language tag (until newline)
                let mut lang = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == '\n' {
                        chars.next();
                        break;
                    }
                    lang.push(ch);
                    chars.next();
                }
                let lang = lang.trim().to_string();

                // Collect code block content until closing ```
                let mut code = String::new();
                loop {
                    match chars.next() {
                        None => break,
                        Some('`') if chars.peek() == Some(&'`') => {
                            chars.next(); // second `
                            if chars.peek() == Some(&'`') {
                                chars.next(); // third `
                                break;
                            } else {
                                code.push_str("``");
                            }
                        }
                        Some(ch) => code.push(ch),
                    }
                }

                // Trim trailing newline from code block
                let code = code.trim_end_matches('\n');

                if lang.is_empty() {
                    result.push_str(&format!("<pre>{}</pre>", escape_html(code)));
                } else {
                    result.push_str(&format!(
                        "<pre><code class=\"language-{}\">{}</code></pre>",
                        escape_html(&lang),
                        escape_html(code)
                    ));
                }
                continue;
            } else {
                // Only two backticks — treat as literal
                result.push_str("``");
                continue;
            }
        }

        // Inline code: `...`
        if c == '`' {
            let mut code = String::new();
            let mut found_closing = false;
            while let Some(&ch) = chars.peek() {
                if ch == '`' {
                    chars.next();
                    found_closing = true;
                    break;
                }
                if ch == '\n' {
                    break; // don't span lines for inline code
                }
                code.push(ch);
                chars.next();
            }
            if found_closing && !code.is_empty() {
                result.push_str(&format!("<code>{}</code>", escape_html(&code)));
            } else {
                result.push('`');
                result.push_str(&escape_html(&code));
            }
            continue;
        }

        // Bold: **...**
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next(); // consume second *
            let mut bold = String::new();
            let mut found_closing = false;
            while let Some(&ch) = chars.peek() {
                if ch == '*' {
                    chars.next();
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        found_closing = true;
                        break;
                    } else {
                        bold.push('*');
                    }
                } else {
                    bold.push(ch);
                    chars.next();
                }
            }
            if found_closing && !bold.is_empty() {
                result.push_str(&format!("<b>{}</b>", escape_html(&bold)));
            } else {
                result.push_str("**");
                result.push_str(&escape_html(&bold));
            }
            continue;
        }

        // Regular character — escape HTML
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            _ => result.push(c),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // chunk_message tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // markdown_to_html tests
    // -----------------------------------------------------------------------

    #[test]
    fn plain_text_is_preserved() {
        assert_eq!(markdown_to_html("hello world"), "hello world");
    }

    #[test]
    fn html_chars_are_escaped() {
        assert_eq!(markdown_to_html("a < b > c & d"), "a &lt; b &gt; c &amp; d");
    }

    #[test]
    fn inline_code_converted() {
        assert_eq!(
            markdown_to_html("run `cargo test` now"),
            "run <code>cargo test</code> now"
        );
    }

    #[test]
    fn inline_code_escapes_html_inside() {
        assert_eq!(
            markdown_to_html("use `Vec<String>`"),
            "use <code>Vec&lt;String&gt;</code>"
        );
    }

    #[test]
    fn fenced_code_block_no_lang() {
        let input = "before\n```\nfn main() {}\n```\nafter";
        let result = markdown_to_html(input);
        assert!(result.contains("<pre>fn main() {}</pre>"));
        assert!(result.contains("before\n"));
        assert!(result.contains("\nafter"));
    }

    #[test]
    fn fenced_code_block_with_lang() {
        let input = "```rust\nlet x = 1;\n```";
        let result = markdown_to_html(input);
        assert!(result.contains("<pre><code class=\"language-rust\">let x = 1;</code></pre>"));
    }

    #[test]
    fn fenced_code_block_escapes_html() {
        let input = "```\nif a < b && c > d {}\n```";
        let result = markdown_to_html(input);
        assert!(result.contains("a &lt; b &amp;&amp; c &gt; d"));
    }

    #[test]
    fn bold_converted() {
        assert_eq!(
            markdown_to_html("this is **bold** text"),
            "this is <b>bold</b> text"
        );
    }

    #[test]
    fn bold_escapes_html_inside() {
        assert_eq!(markdown_to_html("**a < b**"), "<b>a &lt; b</b>");
    }

    #[test]
    fn mixed_formatting() {
        let input = "**Step 1**: Run `cargo build`\n```\ncargo build --release\n```\nDone.";
        let result = markdown_to_html(input);
        assert!(result.contains("<b>Step 1</b>"));
        assert!(result.contains("<code>cargo build</code>"));
        assert!(result.contains("<pre>cargo build --release</pre>"));
        assert!(result.contains("Done."));
    }

    #[test]
    fn unclosed_inline_code_treated_as_literal() {
        assert_eq!(markdown_to_html("a `b c"), "a `b c");
    }

    #[test]
    fn unclosed_bold_treated_as_literal() {
        assert_eq!(markdown_to_html("a **b c"), "a **b c");
    }

    #[test]
    fn empty_input() {
        assert_eq!(markdown_to_html(""), "");
    }
}
