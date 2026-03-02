# Changelog

## 0.1.0 — Unreleased

### Added

- Systemd journal monitoring with configurable unit and priority filters.
- TOML configuration file with example at `config.example.toml`.
- Journal cursor persistence for incremental scanning across runs. Cursor is saved atomically (write-tmp + rename).
- First-run detection: establishes a baseline cursor without generating a report.
- Email alerting via SMTP with STARTTLS (lettre).
- Plain-text report formatting with per-entry timestamp, unit, priority, and message.
- `--dry-run` flag to print the report to stdout instead of sending email.
- Environment variable overrides for SMTP password (`FRIENDLY_GHOST_SMTP_PASSWORD`) and host (`FRIENDLY_GHOST_SMTP_HOST`).
- Priority filtering by severity level (emerg through debug). Lower numeric value = higher severity.
- 21 unit tests covering config parsing, priority ordering, entry filtering, cursor round-trips, and report formatting.

### Changed

- Error variants (`Config`, `Journal`, `Email`) wrap `Cow<'static, str>` instead of `String`. Static error messages avoid heap allocation.
- `config::load()` takes `EnvOverrides` by value. SMTP password and host are moved into the config instead of cloned.
- `email::send_report()` takes `EmailConfig` by value and owned `String` for subject and body. Username and password are moved directly into `Credentials` instead of cloned/reallocated.
- Journal entry parsing uses `record.remove()` instead of `.get().cloned()`, taking ownership from the `BTreeMap`. The `.service` suffix is stripped in-place with `truncate()`.
- `filter_entries` uses `retain()` instead of `into_iter().filter().collect()`, reusing the existing `Vec` allocation.
