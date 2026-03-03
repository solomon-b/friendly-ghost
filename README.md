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
units = ["nginx", "sshd", "web-.*"]  # supports regex patterns
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

## NixOS Module

Add the flake to your inputs and import the module:

```nix
# flake.nix
{
  inputs.friendly-ghost.url = "github:your-user/friendly-ghost";

  outputs = { self, nixpkgs, friendly-ghost, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        friendly-ghost.nixosModules.default
        {
          services.friendly-ghost = {
            enable = true;
            interval = "*:0/5"; # every 5 minutes (default)

            journal = {
              units = [ "nginx" "sshd" "web-.*" ];
              priority = "err";
            };

            email = {
              smtpHost = "mail.example.com";
              smtpPort = 587;
              username = "alerts@example.com";
              from = "alerts@example.com";
              to = [ "admin@example.com" ];
              subjectPrefix = "[friendly-ghost]";
            };

            environmentFile = "/run/secrets/friendly-ghost.env";

            # optional LLM analysis
            llm = {
              enable = true;
              apiUrl = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
              model = "gemini-2.5-flash";
              systemPromptFile = "/etc/friendly-ghost/prompt.txt";
            };
          };
        }
      ];
    };
  };
}
```

The environment file should contain secrets:

```
FRIENDLY_GHOST_SMTP_PASSWORD=secret
```

The module creates a systemd timer and service with `DynamicUser`, `StateDirectory`, and journal read access handled automatically.

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
