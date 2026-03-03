use regex::RegexSet;

use crate::config::Priority;
use crate::error::AppError;

/// A normalized journal entry for processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub timestamp: String,
    pub unit: String,
    pub priority: u8,
    pub message: String,
    pub cursor: String,
}

/// Matches unit names against compiled regex patterns.
///
/// All entries are compiled into a `RegexSet` and auto-anchored with
/// `^(?:...)$`. Plain names like `"nginx"` match exactly; patterns like
/// `"web-.*"` match dynamically.
#[derive(Debug)]
pub struct UnitMatcher {
    patterns: RegexSet,
}

impl UnitMatcher {
    /// Build from the raw config strings. Each entry is auto-anchored
    /// with `^(?:...)$` and compiled into a `RegexSet`.
    pub fn new(units: &[String]) -> Result<Self, AppError> {
        let anchored: Vec<String> = units
            .iter()
            .map(|u| format!("^(?:{u})$"))
            .collect();
        let patterns = RegexSet::new(&anchored)
            .map_err(|e| AppError::Config(format!("invalid unit pattern: {e}").into()))?;
        Ok(Self { patterns })
    }

    pub fn is_match(&self, unit: &str) -> bool {
        self.patterns.is_match(unit)
    }
}

/// Filter entries by configured units and minimum priority level.
/// Lower priority number = higher severity (0=emerg, 7=debug).
/// Entries with priority <= max_priority pass the filter.
pub fn filter_entries(
    mut entries: Vec<JournalEntry>,
    matcher: &UnitMatcher,
    max_priority: Priority,
) -> Vec<JournalEntry> {
    let max_level = max_priority.as_level();
    entries.retain(|e| matcher.is_match(&e.unit) && e.priority <= max_level);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(unit: &str, priority: u8, msg: &str) -> JournalEntry {
        JournalEntry {
            timestamp: "2026-03-02T10:00:00Z".to_string(),
            unit: unit.to_string(),
            priority,
            message: msg.to_string(),
            cursor: "cursor_1".to_string(),
        }
    }

    fn matcher(units: &[&str]) -> UnitMatcher {
        let owned: Vec<String> = units.iter().map(|s| s.to_string()).collect();
        UnitMatcher::new(&owned).unwrap()
    }

    #[test]
    fn filters_by_unit() {
        let entries = vec![
            make_entry("nginx", 3, "error in nginx"),
            make_entry("postgres", 3, "error in postgres"),
            make_entry("sshd", 3, "error in sshd"),
        ];
        let m = matcher(&["nginx", "sshd"]);
        let result = filter_entries(entries, &m, Priority::Err);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].unit, "nginx");
        assert_eq!(result[1].unit, "sshd");
    }

    #[test]
    fn filters_by_priority() {
        let entries = vec![
            make_entry("nginx", 3, "error"),
            make_entry("nginx", 6, "info message"),
            make_entry("nginx", 0, "emergency"),
        ];
        let m = matcher(&["nginx"]);
        let result = filter_entries(entries, &m, Priority::Err);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].message, "error");
        assert_eq!(result[1].message, "emergency");
    }

    #[test]
    fn empty_entries_returns_empty() {
        let m = matcher(&["nginx"]);
        let result = filter_entries(vec![], &m, Priority::Err);
        assert!(result.is_empty());
    }

    #[test]
    fn no_matching_units_returns_empty() {
        let entries = vec![make_entry("postgres", 3, "error")];
        let m = matcher(&["nginx"]);
        let result = filter_entries(entries, &m, Priority::Err);
        assert!(result.is_empty());
    }

    #[test]
    fn all_entries_pass_when_all_match() {
        let entries = vec![
            make_entry("nginx", 0, "emergency"),
            make_entry("nginx", 2, "critical"),
            make_entry("nginx", 3, "error"),
        ];
        let m = matcher(&["nginx"]);
        let result = filter_entries(entries, &m, Priority::Err);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn regex_matches_expected_units() {
        let m = matcher(&["web-.*"]);
        assert!(m.is_match("web-frontend"));
        assert!(m.is_match("web-backend"));
        assert!(m.is_match("web-api"));
    }

    #[test]
    fn regex_does_not_match_non_matching() {
        let m = matcher(&["web-.*"]);
        assert!(!m.is_match("nginx"));
        assert!(!m.is_match("sshd"));
        assert!(!m.is_match("webserver"));
    }

    #[test]
    fn mixed_exact_and_regex() {
        let m = matcher(&["nginx", "web-.*"]);
        assert!(m.is_match("nginx"));
        assert!(m.is_match("web-frontend"));
        assert!(!m.is_match("sshd"));
    }

    #[test]
    fn auto_anchoring_prevents_substring_matches() {
        let m = matcher(&["web"]);
        assert!(m.is_match("web"));
        assert!(!m.is_match("web-frontend"));
        assert!(!m.is_match("cobweb"));
    }

    #[test]
    fn invalid_regex_produces_error() {
        let units = vec!["[invalid".to_string()];
        let result = UnitMatcher::new(&units);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid unit pattern"), "got: {err}");
    }

    #[test]
    fn multiple_regex_patterns() {
        let m = matcher(&["web-.*", "db-.*"]);
        assert!(m.is_match("web-frontend"));
        assert!(m.is_match("db-primary"));
        assert!(m.is_match("db-replica"));
        assert!(!m.is_match("nginx"));
    }

    #[test]
    fn regex_with_escaped_special_chars() {
        let m = matcher(&["my\\.app"]);
        assert!(m.is_match("my.app"));
        assert!(!m.is_match("my-app"));
        assert!(!m.is_match("myXapp"));
    }

    #[test]
    fn regex_filter_entries_integration() {
        let entries = vec![
            make_entry("web-frontend", 3, "frontend error"),
            make_entry("web-backend", 3, "backend error"),
            make_entry("nginx", 3, "nginx error"),
            make_entry("sshd", 3, "sshd error"),
        ];
        let m = matcher(&["nginx", "web-.*"]);
        let result = filter_entries(entries, &m, Priority::Err);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].unit, "web-frontend");
        assert_eq!(result[1].unit, "web-backend");
        assert_eq!(result[2].unit, "nginx");
    }
}
