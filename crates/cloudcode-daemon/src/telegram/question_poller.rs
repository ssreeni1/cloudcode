use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

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

/// Per-session activity state for completion detection
#[derive(Clone, Debug)]
pub enum ActivityState {
    Idle,
    Active { since: Instant },
    Stabilizing { since: Instant, active_since: Instant },
}

/// Shared flag to coordinate with send_via_tmux
pub type SendingFlags = Arc<std::sync::Mutex<HashMap<String, bool>>>;

pub fn new_sending_flags() -> SendingFlags {
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
    sending_flags: SendingFlags,
) {
    log::info!("Question poller started");

    // Track previous pane content per session for stabilization detection
    let mut prev_content: HashMap<String, String> = HashMap::new();
    let mut stable_polls: HashMap<String, u8> = HashMap::new();
    // Track last sent question hash per session for dedup
    let mut last_sent_hash: HashMap<String, u64> = HashMap::new();
    let mut activity_states: HashMap<String, ActivityState> = HashMap::new();
    let mut completion_file_baselines: HashMap<String, HashMap<PathBuf, (SystemTime, u64)>> = HashMap::new();
    let mut last_completion_hash: HashMap<String, u64> = HashMap::new();

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
        activity_states.retain(|k, _| active_names.contains(k));
        completion_file_baselines.retain(|k, _| active_names.contains(k));
        last_completion_hash.retain(|k, _| active_names.contains(k));

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

            // Skip sessions with active send_via_tmux (prevents double-notification)
            {
                let flags = sending_flags.lock().unwrap();
                if flags.get(&session.name).copied().unwrap_or(false) {
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

            if stabilized && stable_count >= 2 {
                // Check for question pattern
                if let Some(detection) = detect_question(&content) {
                    let hash = content_hash(&detection.question);

                    // Check if we already sent this exact question
                    if last_sent_hash.get(&session.name) != Some(&hash) {
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

            // --- Completion detection ---
            // Track activity state transitions for CLI→TG notifications
            let activity = activity_states
                .entry(session.name.clone())
                .or_insert(ActivityState::Idle);

            if !stabilized {
                // Content is changing — session is active
                match activity {
                    ActivityState::Idle => {
                        *activity = ActivityState::Active { since: Instant::now() };
                        // Take file baseline when entering active state
                        // (for detecting new files on completion)
                    }
                    ActivityState::Stabilizing { active_since, .. } => {
                        // Was stabilizing but content changed again — back to active
                        *activity = ActivityState::Active { since: *active_since };
                    }
                    ActivityState::Active { .. } => {
                        // Already active, keep going
                    }
                }
            } else if stable_count >= 2 {
                match activity.clone() {
                    ActivityState::Active { since: active_since } => {
                        // Just stabilized after being active
                        *activity = ActivityState::Stabilizing {
                            since: Instant::now(),
                            active_since,
                        };
                    }
                    ActivityState::Stabilizing { active_since, since: stab_since } => {
                        // Check if stable long enough AND was active long enough
                        let was_active_long_enough = active_since.elapsed() > std::time::Duration::from_secs(10);
                        let stable_long_enough = stab_since.elapsed() > std::time::Duration::from_secs(4);

                        if was_active_long_enough && stable_long_enough {
                            // Check for idle prompt (completion indicator).
                            // Claude Code's UI has the prompt (❯) on a line
                            // above the status bar, so check the last 5 lines.
                            let recent_lines: Vec<&str> = content
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .rev()
                                .take(5)
                                .collect();

                            let is_idle = recent_lines.iter().any(|line| {
                                let t = line.trim();
                                t == ">"
                                    || t == "❯"
                                    || t == "❯ "
                                    || t.ends_with("$ ")
                                    || t == "$"
                                    || t.contains("bypass permissions")
                                    || t.contains("shift+tab to cycle")
                            });

                            // Filter out restart banners
                            let is_restart = content
                                .lines()
                                .rev()
                                .take(5)
                                .any(|l| l.contains("[cloudcode]"));

                            if is_idle && !is_restart {
                                // Completion detected! Dedup by content hash
                                let completion_hash = content_hash(&content);
                                if last_completion_hash.get(&session.name) != Some(&completion_hash) {
                                    last_completion_hash.insert(session.name.clone(), completion_hash);

                                    log::info!("Task completion detected in session '{}'", session.name);

                                    let msg = format!(
                                        "✅ <b>[{}]</b> Task completed. Use /peek to see output.",
                                        session.name
                                    );
                                    let _ = sender.send_html(owner_id, &msg).await;
                                }
                            }

                            *activity = ActivityState::Idle;
                        }
                    }
                    ActivityState::Idle => {
                        // Already idle, nothing to do
                    }
                }
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

    #[test]
    fn test_activity_state_transitions() {
        // Just verify the enum can be constructed and cloned
        let idle = ActivityState::Idle;
        let active = ActivityState::Active { since: Instant::now() };
        let stabilizing = ActivityState::Stabilizing {
            since: Instant::now(),
            active_since: Instant::now(),
        };
        let _ = idle.clone();
        let _ = active.clone();
        let _ = stabilizing.clone();
    }
}
