use crate::config::Priority;

/// A normalized journal entry for processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub timestamp: String,
    pub unit: String,
    pub priority: u8,
    pub message: String,
    pub cursor: String,
}

/// Filter entries by configured units and minimum priority level.
/// Lower priority number = higher severity (0=emerg, 7=debug).
/// Entries with priority <= max_priority pass the filter.
pub fn filter_entries(
    mut entries: Vec<JournalEntry>,
    units: &[String],
    max_priority: Priority,
) -> Vec<JournalEntry> {
    let max_level = max_priority.as_level();
    entries.retain(|e| units.contains(&e.unit) && e.priority <= max_level);
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

    #[test]
    fn filters_by_unit() {
        let entries = vec![
            make_entry("nginx", 3, "error in nginx"),
            make_entry("postgres", 3, "error in postgres"),
            make_entry("sshd", 3, "error in sshd"),
        ];
        let result = filter_entries(
            entries,
            &["nginx".to_string(), "sshd".to_string()],
            Priority::Err,
        );
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
        let result = filter_entries(entries, &["nginx".to_string()], Priority::Err);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].message, "error");
        assert_eq!(result[1].message, "emergency");
    }

    #[test]
    fn empty_entries_returns_empty() {
        let result = filter_entries(vec![], &["nginx".to_string()], Priority::Err);
        assert!(result.is_empty());
    }

    #[test]
    fn no_matching_units_returns_empty() {
        let entries = vec![make_entry("postgres", 3, "error")];
        let result = filter_entries(entries, &["nginx".to_string()], Priority::Err);
        assert!(result.is_empty());
    }

    #[test]
    fn all_entries_pass_when_all_match() {
        let entries = vec![
            make_entry("nginx", 0, "emergency"),
            make_entry("nginx", 2, "critical"),
            make_entry("nginx", 3, "error"),
        ];
        let result = filter_entries(entries, &["nginx".to_string()], Priority::Err);
        assert_eq!(result.len(), 3);
    }
}
