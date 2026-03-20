use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::sender::TelegramSender;
use crate::session::manager::SessionManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionConfidence {
    High,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionDetection {
    pub question: String,
    pub confidence: DetectionConfidence,
}

/// Per-session question state
#[derive(Clone, Debug)]
pub enum SessionQuestionState {
    Idle,
    WaitingForInput {
        question: String,
        detected_at: Instant,
    },
}

/// Shared question state across poller and handlers
pub type QuestionStates = Arc<std::sync::Mutex<HashMap<String, SessionQuestionState>>>;

pub fn new_question_states() -> QuestionStates {
    Arc::new(std::sync::Mutex::new(HashMap::new()))
}

/// Detect if pane content indicates Claude is asking a question.
/// Only high-confidence prompt patterns near the end of the pane qualify.
pub fn detect_question(pane_content: &str) -> Option<QuestionDetection> {
    let lines: Vec<&str> = pane_content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    let check_lines: Vec<&str> = lines.iter().rev().take(5).copied().collect();
    let has_question_pattern = check_lines.iter().any(|line| {
        let l = line.trim();
        l.contains("(y/n)")
            || l.contains("(yes/no)")
            || l.contains("[y/N]")
            || l.contains("[Y/n]")
            || l == ">"
            || l.ends_with("\n>")
            || (l.starts_with("Do you want to ") && l.ends_with('?'))
            || (l.starts_with("Would you like to ") && l.ends_with('?'))
            || (l.starts_with("Grant permission") && l.ends_with('?'))
            || (l.starts_with("Enter ") && l.ends_with(':'))
            // Plan mode patterns
            || l.contains("plan mode")
            || l.contains("Plan mode")
            || l.contains("ExitPlanMode")
            || l.contains("Do you want me to")
            || l.contains("Shall I")
            || l.contains("Should I")
            || l.contains("Ready to proceed")
            || l.contains("proceed with")
    });

    if !has_question_pattern {
        return None;
    }

    // Return last 20 non-empty lines as the question context
    let context_lines: Vec<&str> = lines
        .iter()
        .rev()
        .take(20)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(QuestionDetection {
        question: context_lines.join("\n"),
        confidence: DetectionConfidence::High,
    })
}

/// Compute a simple hash of a string for deduplication
fn content_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Run the background poller that watches tmux sessions for questions.
/// Accepts a TelegramSender trait object instead of teloxide Bot directly.
pub async fn run_poller(
    session_mgr: Arc<SessionManager>,
    sender: Arc<dyn TelegramSender>,
    owner_id: i64,
    states: QuestionStates,
) {
    log::info!("Question poller started");

    // Track previous pane content per session for stabilization detection
    let mut prev_content: HashMap<String, String> = HashMap::new();
    let mut stable_polls: HashMap<String, u8> = HashMap::new();
    // Track last sent question hash per session for dedup
    let mut last_sent_hash: HashMap<String, u64> = HashMap::new();

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3));

    loop {
        interval.tick().await;

        // Expire stale WaitingForInput states (5 minute timeout)
        {
            let mut states_lock = states.lock().unwrap();
            let expired: Vec<String> = states_lock
                .iter()
                .filter_map(|(name, state)| {
                    if let SessionQuestionState::WaitingForInput { detected_at, .. } = state {
                        if detected_at.elapsed() > std::time::Duration::from_secs(300) {
                            return Some(name.clone());
                        }
                    }
                    None
                })
                .collect();
            for name in expired {
                log::info!("Question state expired for session '{}'", name);
                states_lock.insert(name, SessionQuestionState::Idle);
            }
        }

        // List active sessions
        let sessions = match session_mgr.list().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Clean up tracking for dead sessions
        let active_names: std::collections::HashSet<String> =
            sessions.iter().map(|s| s.name.clone()).collect();
        prev_content.retain(|k, _| active_names.contains(k));
        stable_polls.retain(|k, _| active_names.contains(k));
        last_sent_hash.retain(|k, _| active_names.contains(k));

        for session in &sessions {
            // Skip sessions already in WaitingForInput
            {
                let states_lock = states.lock().unwrap();
                if let Some(SessionQuestionState::WaitingForInput { .. }) =
                    states_lock.get(&session.name)
                {
                    continue;
                }
            }

            // Capture pane content
            let content = match session_mgr.capture_pane(&session.name).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Check if output has stabilized (same content as last poll)
            let prev = prev_content.get(&session.name);
            let stabilized = prev.map_or(false, |p| p == &content);
            let stable_count = if stabilized {
                let entry = stable_polls.entry(session.name.clone()).or_insert(0);
                *entry = entry.saturating_add(1);
                *entry
            } else {
                stable_polls.insert(session.name.clone(), 0);
                0
            };
            prev_content.insert(session.name.clone(), content.clone());

            if !stabilized || stable_count < 2 {
                continue;
            }

            // Check for question pattern
            if let Some(detection) = detect_question(&content) {
                let hash = content_hash(&detection.question);

                // Check if we already sent this exact question
                if last_sent_hash.get(&session.name) == Some(&hash) {
                    continue;
                }

                // New question detected!
                log::info!("Question detected in session '{}'", session.name);
                last_sent_hash.insert(session.name.clone(), hash);

                // Update state to WaitingForInput
                {
                    let mut states_lock = states.lock().unwrap();
                    states_lock.insert(
                        session.name.clone(),
                        SessionQuestionState::WaitingForInput {
                            question: detection.question.clone(),
                            detected_at: Instant::now(),
                        },
                    );
                }

                // Send to Telegram with HTML formatting.
                // Truncate the question to ensure the final message fits
                // in a single 4096-char Telegram message (avoids splitting
                // mid-HTML-tag when chunking).
                let escaped_question = detection
                    .question
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                // Reserve ~200 chars for the wrapper HTML + session names
                let max_question_len = 3800;
                let truncated = if escaped_question.len() > max_question_len {
                    format!("{}…", &escaped_question[..max_question_len])
                } else {
                    escaped_question
                };
                let msg = format!(
                    "\u{1f514} <b>[{}]</b> Claude is waiting for input:\n\n<pre>{}</pre>\n\nUse /reply {} &lt;text&gt; or /type {} &lt;text&gt;",
                    session.name, truncated, session.name, session.name
                );
                // Message is guaranteed to fit in one chunk, but send_html
                // handles chunking safely for plain text fallback
                let _ = sender.send_html(owner_id, &msg).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // detect_question tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_question_with_question_mark() {
        let content = "Some output\nMore output\nDo you want to continue?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_yn_prompt() {
        let content = "Claude wants to run: rm -rf /tmp\nAllow? (y/n)";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_input_prompt() {
        let content = "What should I name the file?\n> ";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_plan_mode() {
        let content = "I have a plan for implementing this feature.\nEnter plan mode to review?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_do_you_want() {
        let content = "Files modified:\n- src/main.rs\nDo you want to proceed with these changes?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_would_you_like() {
        let content = "I found 3 issues.\nWould you like to fix them?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_with_permission() {
        let content = "Claude wants to execute a bash command\nGrant permission to proceed?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_no_question() {
        let content = "Compiling project...\nBuild successful.\n3 tests passed.";
        assert!(detect_question(content).is_none());
    }

    #[test]
    fn test_detect_question_empty_content() {
        assert!(detect_question("").is_none());
        assert!(detect_question("   \n  \n  ").is_none());
    }

    #[test]
    fn test_detect_question_returns_last_20_lines() {
        let mut lines: Vec<String> = (1..=30).map(|i| format!("line {}", i)).collect();
        lines.push("Do you want to continue?".to_string());
        let content = lines.join("\n");
        let result = detect_question(&content).unwrap();
        assert!(result.question.contains("Do you want to continue?"));
        assert!(!result.question.contains("line 1\n"));
    }

    #[test]
    fn test_detect_question_only_checks_last_5_lines() {
        let content = "Is this a question?\nNo pattern here\nJust output\nMore output\nStill going\nDone building\nAll complete";
        assert!(detect_question(content).is_none());
    }

    // -----------------------------------------------------------------------
    // content_hash tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_content_hash_same_input_same_hash() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
    }

    #[test]
    fn test_content_hash_different_input_different_hash() {
        assert_ne!(content_hash("hello"), content_hash("world"));
    }

    // -----------------------------------------------------------------------
    // SessionQuestionState tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_question_states_is_empty() {
        let states = new_question_states();
        let lock = states.lock().unwrap();
        assert!(lock.is_empty());
    }

    #[test]
    fn test_question_state_transitions() {
        let states = new_question_states();

        {
            let mut lock = states.lock().unwrap();
            lock.insert("test".to_string(), SessionQuestionState::Idle);
        }

        {
            let mut lock = states.lock().unwrap();
            lock.insert(
                "test".to_string(),
                SessionQuestionState::WaitingForInput {
                    question: "Do you want?".to_string(),
                    detected_at: Instant::now(),
                },
            );
        }

        {
            let lock = states.lock().unwrap();
            assert!(matches!(
                lock.get("test"),
                Some(SessionQuestionState::WaitingForInput { .. })
            ));
        }

        {
            let mut lock = states.lock().unwrap();
            lock.insert("test".to_string(), SessionQuestionState::Idle);
        }

        {
            let lock = states.lock().unwrap();
            assert!(matches!(lock.get("test"), Some(SessionQuestionState::Idle)));
        }
    }

    #[test]
    fn test_question_state_timeout_detection() {
        let states = new_question_states();

        {
            let mut lock = states.lock().unwrap();
            lock.insert(
                "expired".to_string(),
                SessionQuestionState::WaitingForInput {
                    question: "Old question?".to_string(),
                    detected_at: Instant::now() - std::time::Duration::from_secs(301),
                },
            );
            lock.insert(
                "fresh".to_string(),
                SessionQuestionState::WaitingForInput {
                    question: "New question?".to_string(),
                    detected_at: Instant::now(),
                },
            );
        }

        let expired: Vec<String> = {
            let lock = states.lock().unwrap();
            lock.iter()
                .filter_map(|(name, state)| {
                    if let SessionQuestionState::WaitingForInput { detected_at, .. } = state {
                        if detected_at.elapsed() > std::time::Duration::from_secs(300) {
                            return Some(name.clone());
                        }
                    }
                    None
                })
                .collect()
        };

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "expired");
    }

    #[test]
    fn test_detect_question_approve_pattern() {
        let content = "I need to modify 5 files.\nShall I proceed with these changes?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_proceed_with() {
        let content = "Here's my plan:\n1. Update the API\n2. Add tests\nReady to proceed with implementation?";
        assert!(detect_question(content).is_some());
    }

    #[test]
    fn test_detect_question_do_you_want_me_to() {
        let content = "I found a bug in the auth module.\nDo you want me to fix it?";
        assert!(detect_question(content).is_some());
    }
}
