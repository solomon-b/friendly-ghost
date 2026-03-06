use std::fmt::Write;

use crate::config::LlmConfig;
use crate::error::AppError;
use crate::filter::JournalEntry;

/// Built-in system prompt that defines the LLM's role and response format.
/// Users can append additional context via `system_prompt_file` in config.
pub const BASE_SYSTEM_PROMPT: &str = r#"You are a server monitoring assistant for friendly-ghost. You receive batches of systemd journal log entries and decide whether they require human attention.

DO NOT alert on:
- Routine bot/scanner probes (random PHP paths, wp-login, .env, etc.)
- Normal service lifecycle events (started, stopped, reloaded)
- Transient network issues that self-resolve (single connection reset, brief DNS timeout)
- Log entries that are informational or expected during normal operation

DO alert on:
- Service crashes, unexpected exits, or repeated restart loops
- Resource exhaustion (disk full, OOM kills, file descriptor limits)
- Evidence of unauthorized access or successful exploitation
- Persistent errors that indicate a degraded service (repeated upstream failures, sustained database errors)
- Security-relevant events (failed auth brute force, certificate expiry, privilege escalation)

Response format — you MUST use exactly one of these:

1. If nothing requires attention, respond with exactly:
NO_ISSUES

2. If something requires attention, respond with:
SUBJECT: <short summary for email subject line>
<body explaining the issue, affected service, relevant log lines, and suggested action>

If you see recurring noise that is not actionable, suggest an `ignore_patterns` regex the operator could add to filter it out in the future."#;

/// The verdict returned by the LLM after analyzing journal entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmVerdict {
    NoIssues,
    Alert { subject: String, body: String },
}

/// Format a slice of journal entries into the user message sent to the LLM.
pub fn format_user_message(entries: &[JournalEntry]) -> String {
    let mut msg = String::new();
    writeln!(msg, "{} log entries:", entries.len()).unwrap();
    for entry in entries {
        writeln!(
            msg,
            "\n[{}] {} (priority {}): {}",
            entry.timestamp, entry.unit, entry.priority, entry.message,
        )
        .unwrap();
    }
    msg
}

/// Parse the raw text response from the LLM into an `LlmVerdict`.
///
/// - Trimmed text equal to `"NO_ISSUES"` → `NoIssues`
/// - First line starts with `"SUBJECT:"` → extract subject (capped at 78 chars),
///   remainder is body → `Alert`
/// - Anything else → `Alert` with generic subject and full trimmed text as body
pub fn parse_verdict(response_text: &str, hostname: &str) -> LlmVerdict {
    let trimmed = response_text.trim();

    if trimmed == "NO_ISSUES" {
        return LlmVerdict::NoIssues;
    }

    let mut lines = trimmed.splitn(2, '\n');
    let first_line = lines.next().unwrap_or("");
    let rest = lines.next().unwrap_or("");

    if let Some(raw_subject) = first_line.strip_prefix("SUBJECT:") {
        let subject = raw_subject.trim().chars().take(78).collect();
        let body = rest.trim_start().to_string();
        return LlmVerdict::Alert { subject, body };
    }

    LlmVerdict::Alert {
        subject: format!("friendly-ghost alert on {hostname}"),
        body: trimmed.to_string(),
    }
}

/// Send journal entries to the configured LLM API and return a verdict.
pub fn analyze(
    entries: &[JournalEntry],
    hostname: &str,
    config: &LlmConfig,
) -> Result<LlmVerdict, AppError> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| AppError::Llm("LLM API key is not configured".into()))?;

    let user_message = format_user_message(entries);

    let payload = serde_json::json!({
        "model": config.model,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
        "messages": [
            {"role": "system", "content": config.system_prompt},
            {"role": "user", "content": user_message},
        ],
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&config.api_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&payload)
        .send()
        .map_err(|e| AppError::Llm(format!("HTTP request failed: {e}").into()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(AppError::Llm(
            format!("LLM API returned {status}: {body}").into(),
        ));
    }

    let json: serde_json::Value = response
        .json()
        .map_err(|e| AppError::Llm(format!("failed to parse LLM response JSON: {e}").into()))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            AppError::Llm(
                "unexpected LLM response structure: missing choices[0].message.content".into(),
            )
        })?;

    Ok(parse_verdict(content, hostname))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(timestamp: &str, unit: &str, priority: u8, message: &str) -> JournalEntry {
        JournalEntry {
            timestamp: timestamp.to_string(),
            unit: unit.to_string(),
            priority,
            message: message.to_string(),
            cursor: "cursor_1".to_string(),
        }
    }

    #[test]
    fn parse_no_issues() {
        assert_eq!(parse_verdict("NO_ISSUES", "myhost"), LlmVerdict::NoIssues);
    }

    #[test]
    fn parse_no_issues_with_whitespace() {
        assert_eq!(
            parse_verdict("  NO_ISSUES\n  ", "myhost"),
            LlmVerdict::NoIssues
        );
    }

    #[test]
    fn parse_subject_and_body() {
        let response = "SUBJECT: nginx is down\nDetails here.";
        assert_eq!(
            parse_verdict(response, "myhost"),
            LlmVerdict::Alert {
                subject: "nginx is down".to_string(),
                body: "Details here.".to_string(),
            }
        );
    }

    #[test]
    fn parse_subject_only_no_body() {
        let response = "SUBJECT: brief alert";
        assert_eq!(
            parse_verdict(response, "myhost"),
            LlmVerdict::Alert {
                subject: "brief alert".to_string(),
                body: "".to_string(),
            }
        );
    }

    #[test]
    fn parse_subject_truncated_at_78_chars() {
        let long_subject = "a".repeat(100);
        let response = format!("SUBJECT: {long_subject}\nSome body.");
        let verdict = parse_verdict(&response, "myhost");
        match verdict {
            LlmVerdict::Alert { subject, body } => {
                assert_eq!(subject.len(), 78);
                assert_eq!(body, "Some body.");
            }
            other => panic!("expected Alert, got {other:?}"),
        }
    }

    #[test]
    fn parse_no_subject_prefix_uses_generic() {
        let response = "Something went terribly wrong.";
        assert_eq!(
            parse_verdict(response, "myhost"),
            LlmVerdict::Alert {
                subject: "friendly-ghost alert on myhost".to_string(),
                body: "Something went terribly wrong.".to_string(),
            }
        );
    }

    #[test]
    fn parse_subject_with_multiline_body() {
        let response = "SUBJECT: disk full\n\nDisk usage is at 95%.\nImmediate action required.\nConsider pruning old logs.";
        assert_eq!(
            parse_verdict(response, "myhost"),
            LlmVerdict::Alert {
                subject: "disk full".to_string(),
                body: "Disk usage is at 95%.\nImmediate action required.\nConsider pruning old logs."
                    .to_string(),
            }
        );
    }

    #[test]
    fn parse_no_issues_with_extra_text_is_alert() {
        let response = "NO_ISSUES detected";
        assert_eq!(
            parse_verdict(response, "myhost"),
            LlmVerdict::Alert {
                subject: "friendly-ghost alert on myhost".to_string(),
                body: "NO_ISSUES detected".to_string(),
            }
        );
    }

    #[test]
    fn format_user_message_includes_all_entries() {
        let entries = vec![
            make_entry("2026-03-02T10:00:00Z", "nginx", 3, "upstream timeout"),
            make_entry("2026-03-02T10:01:00Z", "sshd", 2, "auth failure"),
        ];
        let msg = format_user_message(&entries);
        assert!(msg.contains("2 log entries:"));
        assert!(msg.contains("nginx"));
        assert!(msg.contains("upstream timeout"));
        assert!(msg.contains("sshd"));
        assert!(msg.contains("auth failure"));
        assert!(msg.contains("2026-03-02T10:00:00Z"));
        assert!(msg.contains("2026-03-02T10:01:00Z"));
    }

    #[test]
    fn format_user_message_empty_entries() {
        let msg = format_user_message(&[]);
        assert!(msg.contains("0 log entries:"));
    }
}
