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
- Regex unit matching: `units` entries are compiled as regex patterns (auto-anchored). Plain names like `"nginx"` match exactly; patterns like `"web-.*"` match dynamically. Invalid patterns fail at config load time. Adds `regex` crate dependency.
- NixOS module (`nixosModules.default` flake output). Typed options under `services.friendly-ghost` for all config fields, systemd timer+service pair, `DynamicUser` isolation, and `environmentFile` support for secrets.
- NixOS module: `email.passwordFile` and `llm.apiKeyFile` options for per-secret file support. Compatible with sops-nix, agenix, and similar file-based secret managers. The service uses a wrapper script to read secret files into environment variables at startup.
- Optional LLM-based anomaly detection via OpenAI-compatible API. Filtered entries are sent to the LLM which writes the email body as prose. Responds `NO_ISSUES` to suppress email. Configured via `[llm]` TOML section and `FRIENDLY_GHOST_LLM_API_KEY` env var.
- `ignore_patterns` option in `[journal]` config: regex patterns matched against log message text. Entries matching any pattern are dropped before LLM analysis or plain-text report generation. Useful for suppressing known false alarms.
- Built-in base system prompt for LLM analysis. Covers role definition, alert/ignore heuristics, and response format (`NO_ISSUES` / `SUBJECT:` + body). `system_prompt_file` is now optional — when provided, its contents are appended as additional operator instructions.

### Fixed

- First-run journal detection on real systems with multiple journal files. `seek_tail()` + `previous()` returned 0 due to a long-standing libsystemd bug ([systemd#9934](https://github.com/systemd/systemd/issues/9934), [systemd#17662](https://github.com/systemd/systemd/issues/17662)). Replaced with `seek_realtime_usec` to a recent timestamp, with `seek_tail` as fallback.
- Pin `psm` to 0.1.27 to support Rust versions older than 1.87 (e.g. nixos-25.05). `psm` 0.1.28+ pulls in `ar_archive_writer` which uses `if let` chains, stabilized in Rust 1.87.

### Changed

- Error variants (`Config`, `Journal`, `Email`) wrap `Cow<'static, str>` instead of `String`. Static error messages avoid heap allocation.
- `config::load()` takes `EnvOverrides` by value. SMTP password and host are moved into the config instead of cloned.
- `email::send_report()` takes `EmailConfig` by value and owned `String` for subject and body. Username and password are moved directly into `Credentials` instead of cloned/reallocated.
- Journal entry parsing uses `record.remove()` instead of `.get().cloned()`, taking ownership from the `BTreeMap`. The `.service` suffix is stripped in-place with `truncate()`.
- `filter_entries` uses `retain()` instead of `into_iter().filter().collect()`, reusing the existing `Vec` allocation.
