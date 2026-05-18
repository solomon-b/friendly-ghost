use crate::config::{Config, SourceConfig};
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
pub fn query(cfg: &Config) -> Result<LogResult, AppError> {
    match &cfg.source {
        SourceConfig::Journal(_) => crate::journal::query_journal(&cfg.state.cursor_file),
        SourceConfig::Loki(loki) => crate::loki::query_loki(loki, &cfg.state.cursor_file),
    }
}
