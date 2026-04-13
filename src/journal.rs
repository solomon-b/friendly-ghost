use std::path::Path;
use std::process::Command;

use crate::error::AppError;
use crate::filter::JournalEntry;

/// Result of querying the journal.
#[derive(Debug)]
pub enum JournalResult {
    /// First run — baseline cursor to save. None if the journal is empty.
    FirstRun(Option<String>),
    /// Subsequent run — entries since last cursor.
    Entries(Vec<JournalEntry>),
}

/// Query the systemd journal for entries using a journalctl subprocess.
///
/// Uses `--cursor-file` for cursor management:
/// - First run (no cursor file): establishes baseline with `journalctl -n 0`
/// - Subsequent runs: reads all entries since last cursor with `journalctl --output=json`
pub fn query_journal(cursor_file: &Path) -> Result<JournalResult, AppError> {
    let first_run = !cursor_file.exists();

    if first_run {
        if let Some(parent) = cursor_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::CursorFile {
                path: cursor_file.to_owned(),
                source: e,
            })?;
        }

        let output = Command::new("journalctl")
            .args(["-n", "0", "--output", "cat"])
            .arg("--cursor-file")
            .arg(cursor_file)
            .output()
            .map_err(|e| AppError::Journal(format!("failed to run journalctl: {e}").into()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Journal(
                format!("journalctl failed: {stderr}").into(),
            ));
        }

        if cursor_file.exists() {
            Ok(JournalResult::FirstRun(Some("baseline".to_string())))
        } else {
            Ok(JournalResult::FirstRun(None))
        }
    } else {
        let output = Command::new("journalctl")
            .args(["--output", "json"])
            .arg("--cursor-file")
            .arg(cursor_file)
            .output()
            .map_err(|e| AppError::Journal(format!("failed to run journalctl: {e}").into()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Journal(
                format!("journalctl failed: {stderr}").into(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let entries = parse_journal_json(&stdout)?;
        Ok(JournalResult::Entries(entries))
    }
}

/// Parse journalctl JSON output (one JSON object per line) into JournalEntry structs.
fn parse_journal_json(output: &str) -> Result<Vec<JournalEntry>, AppError> {
    let mut entries = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let obj: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| AppError::Journal(format!("failed to parse journal JSON: {e}").into()))?;

        let timestamp = obj
            .get("_SOURCE_REALTIME_TIMESTAMP")
            .or_else(|| obj.get("__REALTIME_TIMESTAMP"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let mut unit = obj
            .get("_SYSTEMD_UNIT")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if unit.ends_with(".service") {
            unit.truncate(unit.len() - ".service".len());
        }

        let priority = obj
            .get("PRIORITY")
            .and_then(|v| v.as_str())
            .and_then(|p| p.parse::<u8>().ok())
            .unwrap_or(6);

        let message = obj
            .get("MESSAGE")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let cursor = obj
            .get("__CURSOR")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        entries.push(JournalEntry {
            timestamp,
            unit,
            priority,
            message,
            cursor,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_entry() {
        let json = r#"{"_SYSTEMD_UNIT":"nginx.service","PRIORITY":"3","MESSAGE":"segfault","_SOURCE_REALTIME_TIMESTAMP":"1776103665598487","__CURSOR":"s=abc123"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].unit, "nginx");
        assert_eq!(entries[0].priority, 3);
        assert_eq!(entries[0].message, "segfault");
        assert_eq!(entries[0].timestamp, "1776103665598487");
        assert_eq!(entries[0].cursor, "s=abc123");
    }

    #[test]
    fn parse_strips_service_suffix() {
        let json = r#"{"_SYSTEMD_UNIT":"kpbj-web.service","PRIORITY":"6","MESSAGE":"ok","__REALTIME_TIMESTAMP":"123","__CURSOR":"c1"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries[0].unit, "kpbj-web");
    }

    #[test]
    fn parse_falls_back_to_realtime_timestamp() {
        let json = r#"{"_SYSTEMD_UNIT":"app.service","PRIORITY":"4","MESSAGE":"warn","__REALTIME_TIMESTAMP":"999","__CURSOR":"c2"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries[0].timestamp, "999");
    }

    #[test]
    fn parse_prefers_source_realtime_timestamp() {
        let json = r#"{"PRIORITY":"6","MESSAGE":"msg","_SOURCE_REALTIME_TIMESTAMP":"111","__REALTIME_TIMESTAMP":"222","__CURSOR":"c"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries[0].timestamp, "111");
    }

    #[test]
    fn parse_multiple_lines() {
        let json = r#"{"_SYSTEMD_UNIT":"a.service","PRIORITY":"3","MESSAGE":"err1","__REALTIME_TIMESTAMP":"1","__CURSOR":"c1"}
{"_SYSTEMD_UNIT":"b.service","PRIORITY":"4","MESSAGE":"warn1","__REALTIME_TIMESTAMP":"2","__CURSOR":"c2"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].unit, "a");
        assert_eq!(entries[1].unit, "b");
    }

    #[test]
    fn parse_empty_output() {
        let entries = parse_journal_json("").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_missing_fields_uses_defaults() {
        let json = r#"{"MESSAGE":"bare message","__CURSOR":"c3"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries[0].unit, "");
        assert_eq!(entries[0].priority, 6);
        assert_eq!(entries[0].timestamp, "");
        assert_eq!(entries[0].message, "bare message");
    }

    #[test]
    fn parse_skips_blank_lines() {
        let json = "{ \"MESSAGE\":\"msg\",\"__CURSOR\":\"c1\"}\n\n{\"MESSAGE\":\"msg2\",\"__CURSOR\":\"c2\"}";
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn parse_unit_without_service_suffix() {
        let json = r#"{"_SYSTEMD_UNIT":"sshd","PRIORITY":"3","MESSAGE":"err","__REALTIME_TIMESTAMP":"1","__CURSOR":"c"}"#;
        let entries = parse_journal_json(json).unwrap();
        assert_eq!(entries[0].unit, "sshd");
    }
}
