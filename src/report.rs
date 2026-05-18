use std::collections::BTreeMap;
use std::fmt::Write;

use crate::filter::JournalEntry;

const UNKNOWN_HOST_LABEL: &str = "(unknown host)";

/// Format log entries into a plain-text email report.
///
/// Entries are grouped by host. When all entries are from the same host,
/// the report omits per-host section headers (single section is redundant).
pub fn format_report(entries: &[JournalEntry]) -> String {
    let groups = group_by_host(entries);
    let total = entries.len();
    let mut report = String::new();

    match groups.len() {
        0 => {
            writeln!(report, "friendly-ghost report").unwrap();
            writeln!(report, "0 log entries matched.").unwrap();
        }
        1 => {
            let host = display_host(groups.keys().next().unwrap());
            writeln!(report, "friendly-ghost report for {host}").unwrap();
            writeln!(report, "{total} log entries matched:").unwrap();
            writeln!(report).unwrap();
            for entry in entries {
                write_entry(&mut report, entry);
            }
        }
        n => {
            writeln!(report, "friendly-ghost report").unwrap();
            writeln!(report, "{total} log entries matched across {n} hosts:").unwrap();
            for (host, host_entries) in &groups {
                writeln!(report).unwrap();
                writeln!(
                    report,
                    "=== {} ({} entries) ===",
                    display_host(host),
                    host_entries.len()
                )
                .unwrap();
                for entry in host_entries {
                    write_entry(&mut report, entry);
                }
            }
        }
    }

    report
}

/// Build the email subject line. Single-host → "on <host>"; multi-host → "across N hosts".
pub fn format_subject(prefix: &str, entries: &[JournalEntry]) -> String {
    let count = entries.len();
    let mut hosts: Vec<&str> = entries.iter().map(|e| e.host.as_str()).collect();
    hosts.sort();
    hosts.dedup();
    match hosts.len() {
        0 => format!("{prefix} {count} alerts"),
        1 => format!("{prefix} {count} alerts on {}", display_host(hosts[0])),
        n => format!("{prefix} {count} alerts across {n} hosts"),
    }
}

fn group_by_host(entries: &[JournalEntry]) -> BTreeMap<&str, Vec<&JournalEntry>> {
    let mut groups: BTreeMap<&str, Vec<&JournalEntry>> = BTreeMap::new();
    for entry in entries {
        groups.entry(entry.host.as_str()).or_default().push(entry);
    }
    groups
}

fn display_host(host: &str) -> &str {
    if host.is_empty() {
        UNKNOWN_HOST_LABEL
    } else {
        host
    }
}

fn write_entry(out: &mut String, entry: &JournalEntry) {
    writeln!(
        out,
        "[{}] {} (priority {}): {}",
        entry.timestamp, entry.unit, entry.priority, entry.message,
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str, ts: &str, unit: &str, prio: u8, msg: &str) -> JournalEntry {
        JournalEntry {
            timestamp: ts.to_string(),
            host: host.to_string(),
            unit: unit.to_string(),
            priority: prio,
            message: msg.to_string(),
            cursor: "c".to_string(),
        }
    }

    fn single_host_entries() -> Vec<JournalEntry> {
        vec![
            entry("myhost", "2026-03-02T10:00:00Z", "nginx", 3, "upstream timeout"),
            entry("myhost", "2026-03-02T10:01:00Z", "sshd", 2, "auth failure"),
        ]
    }

    fn multi_host_entries() -> Vec<JournalEntry> {
        vec![
            entry("web01", "2026-03-02T10:00:00Z", "nginx", 3, "upstream timeout"),
            entry("web02", "2026-03-02T10:01:00Z", "sshd", 2, "auth failure"),
            entry("web01", "2026-03-02T10:02:00Z", "postgres", 2, "conn refused"),
        ]
    }

    #[test]
    fn single_host_report_contains_all_entries() {
        let report = format_report(&single_host_entries());
        assert!(report.contains("upstream timeout"));
        assert!(report.contains("auth failure"));
        assert!(report.contains("2 log entries matched"));
        assert!(report.contains("myhost"));
    }

    #[test]
    fn single_host_report_has_no_section_header() {
        let report = format_report(&single_host_entries());
        assert!(!report.contains("=== "));
    }

    #[test]
    fn multi_host_report_groups_by_host() {
        let report = format_report(&multi_host_entries());
        assert!(report.contains("3 log entries matched across 2 hosts"));
        assert!(report.contains("=== web01 (2 entries) ==="));
        assert!(report.contains("=== web02 (1 entries) ==="));
        let web01_pos = report.find("=== web01").unwrap();
        let web02_pos = report.find("=== web02").unwrap();
        assert!(web01_pos < web02_pos, "hosts should be alphabetical");
    }

    #[test]
    fn report_empty_entries() {
        let report = format_report(&[]);
        assert!(report.contains("0 log entries matched"));
    }

    #[test]
    fn report_with_blank_host_uses_placeholder() {
        let entries = vec![entry("", "t", "nginx", 3, "msg")];
        let report = format_report(&entries);
        assert!(report.contains("(unknown host)"));
    }

    #[test]
    fn subject_single_host() {
        let subject = format_subject("[friendly-ghost]", &single_host_entries());
        assert_eq!(subject, "[friendly-ghost] 2 alerts on myhost");
    }

    #[test]
    fn subject_multi_host() {
        let subject = format_subject("[friendly-ghost]", &multi_host_entries());
        assert_eq!(subject, "[friendly-ghost] 3 alerts across 2 hosts");
    }

    #[test]
    fn subject_no_entries() {
        let subject = format_subject("[friendly-ghost]", &[]);
        assert_eq!(subject, "[friendly-ghost] 0 alerts");
    }

    #[test]
    fn subject_unknown_host() {
        let entries = vec![entry("", "t", "nginx", 3, "msg")];
        let subject = format_subject("[friendly-ghost]", &entries);
        assert_eq!(subject, "[friendly-ghost] 1 alerts on (unknown host)");
    }
}
