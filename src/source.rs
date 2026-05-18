use std::path::Path;

use crate::error::AppError;
use crate::filter::JournalEntry;

/// Result of querying a log source.
#[derive(Debug)]
pub enum LogResult {
    /// First run — baseline marker saved. None if the source had no entries to anchor against.
    FirstRun(Option<String>),
    /// Subsequent run — entries since the last bookmark.
    Entries(Vec<JournalEntry>),
}

/// Dispatch to the configured log source.
///
/// Today only the journal source is wired up; a future commit will accept a
/// `&Config` and pick between `journal` and `loki` based on `SourceConfig`.
pub fn query(cursor_file: &Path) -> Result<LogResult, AppError> {
    crate::journal::query_journal(cursor_file)
}
