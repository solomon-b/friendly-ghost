# friendly-ghost 👻

Monitors the systemd journal and sends email alerts when log entries match configured units and severity thresholds.

## Usage

```
friendly-ghost --config /etc/friendly-ghost/config.toml
friendly-ghost --config config.toml --dry-run
```

On first run, saves a cursor to the journal tail. On subsequent runs, reads entries since the last cursor, filters by unit and priority, and emails a report if anything matches.

## Configuration

Copy `config.example.toml` and edit:

```toml
[journal]
units = ["nginx", "sshd"]
priority = "err"  # emerg, alert, crit, err, warning, notice, info, debug

[email]
smtp_host = "mail.example.com"
smtp_port = 587
username = "alerts@example.com"
from = "alerts@example.com"
to = ["admin@example.com"]
subject_prefix = "[friendly-ghost]"

[state]
cursor_file = "/var/lib/friendly-ghost/cursor"
```

SMTP password is set via environment variable:

```
export FRIENDLY_GHOST_SMTP_PASSWORD=secret
```

`FRIENDLY_GHOST_SMTP_HOST` can also override `smtp_host`.

## Building

Requires systemd headers (`systemdLibs` on NixOS, `libsystemd-dev` on Debian).

```
nix build            # via flake
cargo build --release  # via cargo
```

## Development

```
nix develop   # enter dev shell
just check    # fmt + clippy + tests
just dry-run config.example.toml
```
