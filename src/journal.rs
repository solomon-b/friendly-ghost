use std::path::Path;

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

/// Read the saved cursor from disk. Returns None if file doesn't exist.
pub fn read_cursor(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(mut content) => {
            let end = content.trim_end().len();
            content.truncate(end);
            let start = content.len() - content.trim_start().len();
            if start > 0 {
                content.drain(..start);
            }
            if content.is_empty() {
                Ok(None)
            } else {
                Ok(Some(content))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::CursorFile {
            path: path.to_owned(),
            source: e,
        }),
    }
}

/// Save the cursor to disk atomically (write tmp + rename).
pub fn save_cursor(path: &Path, cursor: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::CursorFile {
            path: path.to_owned(),
            source: e,
        })?;
    }
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, cursor).map_err(|e| AppError::CursorFile {
        path: tmp_path.clone(),
        source: e,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|e| AppError::CursorFile {
        path: path.to_owned(),
        source: e,
    })?;
    Ok(())
}

/// Query the systemd journal for entries since the given cursor.
/// If cursor is None (first run), seeks to the tail and returns `FirstRun`
/// with the baseline cursor position for the next run.
///
/// Reads all entries since the cursor; the caller filters via `filter_entries`.
pub fn query_journal(cursor: Option<&str>) -> Result<JournalResult, AppError> {
    use systemd::journal;

    let mut j = journal::OpenOptions::default()
        .system(true)
        .open()
        .map_err(|e| AppError::Journal(format!("failed to open journal: {e}").into()))?;

    match cursor {
        Some(c) => {
            j.seek_cursor(c)
                .map_err(|e| AppError::Journal(format!("failed to seek to cursor: {e}").into()))?;
            // After seek, advance past the entry at the cursor (already reported)
            j.next()
                .map_err(|e| AppError::Journal(format!("failed to advance past cursor: {e}").into()))?;
        }
        None => {
            // First run: seek near the tail to establish baseline.
            //
            // We avoid seek_tail() + previous() because it returns 0 on real
            // systems where the journal spans multiple files (persistent +
            // runtime + per-service). This is a long-standing libsystemd bug:
            //   https://github.com/systemd/systemd/issues/9934
            //   https://github.com/systemd/systemd/issues/17662
            //
            // Instead, seek to 1 second ago via realtime timestamp, then call
            // previous() to land on the latest entry at or before that point.
            use std::time::{SystemTime, UNIX_EPOCH};
            let one_sec_ago = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before epoch")
                .as_micros() as u64
                - 1_000_000;

            j.seek_realtime_usec(one_sec_ago)
                .map_err(|e| AppError::Journal(format!("failed to seek to recent time: {e}").into()))?;

            match j.previous()
                .map_err(|e| AppError::Journal(format!("failed to seek previous: {e}").into()))?
            {
                0 => {
                    // Nothing in the last second — fall back to seek_tail
                    j.seek_tail()
                        .map_err(|e| AppError::Journal(format!("failed to seek tail: {e}").into()))?;
                    match j.previous()
                        .map_err(|e| AppError::Journal(format!("failed to get previous: {e}").into()))?
                    {
                        0 => return Ok(JournalResult::FirstRun(None)),
                        _ => {}
                    }
                }
                _ => {}
            }

            let tail_cursor = j
                .cursor()
                .map_err(|e| AppError::Journal(format!("failed to get cursor: {e}").into()))?;
            return Ok(JournalResult::FirstRun(Some(tail_cursor)));
        }
    }

    let mut entries = Vec::new();
    loop {
        match j
            .next_entry()
            .map_err(|e| AppError::Journal(format!("failed to read journal entry: {e}").into()))?
        {
            None => break,
            Some(mut record) => {
                let timestamp = record
                    .remove("_SOURCE_REALTIME_TIMESTAMP")
                    .or_else(|| record.remove("__REALTIME_TIMESTAMP"))
                    .unwrap_or_default();
                let mut unit = record
                    .remove("_SYSTEMD_UNIT")
                    .unwrap_or_default();
                if unit.ends_with(".service") {
                    unit.truncate(unit.len() - ".service".len());
                }
                let priority = record
                    .get("PRIORITY")
                    .and_then(|p| p.parse::<u8>().ok())
                    .unwrap_or(6);
                let message = record.remove("MESSAGE").unwrap_or_default();
                let entry_cursor = j
                    .cursor()
                    .map_err(|e| AppError::Journal(format!("failed to get cursor: {e}").into()))?;

                entries.push(JournalEntry {
                    timestamp,
                    unit,
                    priority,
                    message,
                    cursor: entry_cursor,
                });
            }
        }
    }

    Ok(JournalResult::Entries(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cursor_missing_file() {
        let result = read_cursor(Path::new("/tmp/nonexistent-friendly-ghost-cursor"));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn cursor_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor");
        save_cursor(&path, "s=abc123;i=42").unwrap();
        let loaded = read_cursor(&path).unwrap();
        assert_eq!(loaded, Some("s=abc123;i=42".to_string()));
    }

    #[test]
    fn read_cursor_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor");
        std::fs::write(&path, "  \n").unwrap();
        assert!(read_cursor(&path).unwrap().is_none());
    }
}
