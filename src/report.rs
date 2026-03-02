use std::fmt::Write;

use crate::filter::JournalEntry;

/// Format journal entries into a plain-text email report.
pub fn format_report(entries: &[JournalEntry], hostname: &str) -> String {
    let mut report = String::new();
    writeln!(report, "friendly-ghost report for {hostname}").unwrap();
    writeln!(report, "{} log entries matched:", entries.len()).unwrap();
    writeln!(report).unwrap();

    for entry in entries {
        writeln!(
            report,
            "[{}] {} (priority {}): {}",
            entry.timestamp, entry.unit, entry.priority, entry.message,
        )
        .unwrap();
    }

    report
}

/// Build the email subject line.
pub fn format_subject(prefix: &str, count: usize, hostname: &str) -> String {
    format!("{prefix} {count} alerts on {hostname}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<JournalEntry> {
        vec![
            JournalEntry {
                timestamp: "2026-03-02T10:00:00Z".to_string(),
                unit: "nginx".to_string(),
                priority: 3,
                message: "upstream timeout".to_string(),
                cursor: "c1".to_string(),
            },
            JournalEntry {
                timestamp: "2026-03-02T10:01:00Z".to_string(),
                unit: "sshd".to_string(),
                priority: 2,
                message: "auth failure".to_string(),
                cursor: "c2".to_string(),
            },
        ]
    }

    #[test]
    fn report_contains_all_entries() {
        let report = format_report(&sample_entries(), "myhost");
        assert!(report.contains("upstream timeout"));
        assert!(report.contains("auth failure"));
        assert!(report.contains("2 log entries matched"));
        assert!(report.contains("myhost"));
    }

    #[test]
    fn report_empty_entries() {
        let report = format_report(&[], "myhost");
        assert!(report.contains("0 log entries matched"));
    }

    #[test]
    fn subject_format() {
        let subject = format_subject("[friendly-ghost]", 5, "myhost");
        assert_eq!(subject, "[friendly-ghost] 5 alerts on myhost");
    }
}
