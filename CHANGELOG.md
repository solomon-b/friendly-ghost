# Changelog

## 0.2.0 — Unreleased

### Added

- **Grafana Loki as an alternative log source.** Pick the source via the new `[source]` table: `type = "journal"` (the existing behavior) or `type = "loki"`. Loki source reads via `/loki/api/v1/query_range`, paginates by nanosecond timestamps, and supports three auth modes (`none`, `bearer`, `basic`) plus a `tenant_id` (`X-Scope-OrgID`) header. Auth secrets come from `FRIENDLY_GHOST_LOKI_BEARER_TOKEN` / `FRIENDLY_GHOST_LOKI_BASIC_USER` / `FRIENDLY_GHOST_LOKI_BASIC_PASSWORD`.
- Configurable Loki label mappings: `unit_label` (default `service_name`), `host_label` (default `host`), `level_labels` (default `["level", "severity", "lvl"]`). Each unit/host has an optional fallback chain. Level values resolve through a configurable `priority_mapping` with sensible defaults covering `error`/`warn`/`info`/etc.
- `host: String` field on every log entry. Journal source extracts `_HOSTNAME` from journalctl JSON; Loki source extracts from `host_label`. The local `/etc/hostname` is the fallback when the source-level value is missing.
- Multi-host report layout. Single-host reports keep the historical "for `<host>`" header. Multi-host reports group entries by host into `=== <host> (N entries) ===` sections. Subject lines are host-aware: "N alerts on web01" for one host, "N alerts across M hosts" otherwise. LLM user messages render entries as `host/unit` for per-host correlation.
- `src/loki.rs` (Loki source) and `src/source.rs` (`LogResult` enum + `source::query` dispatcher).
- Cross-source state-file detection. If the configured `state.cursor_file` looks like the other source's format (a Loki state file at a journal path or vice versa), the error explains the cause instead of producing a cryptic parse failure.

### Changed

- **BREAKING: config schema.** `[journal]` is renamed to `[filter]` (its fields — `units`, `priority`, `ignore_patterns` — are source-agnostic filter rules) and a `[source]` discriminator table is required. Migration: rename the section header and add a `[source]` block above it with `type = "journal"`. `config::load` detects the old layout and produces a migration hint pointing at the new schema.
- **BREAKING: Nix module.** `services.friendly-ghost.journal` is renamed to `services.friendly-ghost.filter`. New options: `services.friendly-ghost.source` (`"journal"` or `"loki"`), `services.friendly-ghost.loki` (URL, query, auth, secret file paths).
- `JournalResult` renamed to `LogResult` and moved to `src/source.rs`. The type is source-agnostic; the rename makes that explicit.
- `report::format_report` and `report::format_subject` derive host context from the entries themselves and no longer take an explicit `hostname` parameter.

### Migration example

Old (v0.1):

```toml
[journal]
units = ["nginx", "sshd"]
priority = "err"
```

New (v0.2):

```toml
[source]
type = "journal"

[filter]
units = ["nginx", "sshd"]
priority = "err"
```

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

- Journal reading on NixOS systems. The `systemd` Rust crate (0.10.x) could not read journal entries — every seek/iteration method returned `Ok(0)` despite matching libsystemd versions and root access. Replaced with `journalctl` subprocess. See Changed below.

### Changed

- **Journal backend: replaced `systemd` crate with `journalctl` subprocess.** The crate's FFI bindings silently returned zero entries on real NixOS systems. `journalctl` reads the same journal correctly. Cursor persistence is now handled natively by `journalctl --cursor-file`, removing the manual `read_cursor`/`save_cursor` functions and atomic write-tmp-rename logic. First run uses `journalctl -n 0 --cursor-file=...` to establish a baseline without reading entries; subsequent runs use `journalctl --output=json --cursor-file=...` and parse one JSON object per line.
- Removed `systemd` and `psm` crate dependencies. Removed `pkg-config` and `systemdLibs` from Nix build inputs.
- Error variants (`Config`, `Journal`, `Email`) wrap `Cow<'static, str>` instead of `String`. Static error messages avoid heap allocation.
- `config::load()` takes `EnvOverrides` by value. SMTP password and host are moved into the config instead of cloned.
- `email::send_report()` takes `EmailConfig` by value and owned `String` for subject and body. Username and password are moved directly into `Credentials` instead of cloned/reallocated.
- `filter_entries` uses `retain()` instead of `into_iter().filter().collect()`, reusing the existing `Vec` allocation.
