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

## LLM Analysis (optional)

friendly-ghost can optionally send filtered log entries to an OpenAI-compatible LLM for anomaly detection. The LLM writes the email body as prose instead of the default plain-text format.

Add an `[llm]` section to your config:

```toml
[llm]
api_url = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
model = "gemini-2.5-flash"
system_prompt_file = "/etc/friendly-ghost/prompt.txt"
temperature = 0.1
max_tokens = 4096
```

Set the API key via environment variable:

```
export FRIENDLY_GHOST_LLM_API_KEY=your-key-here
```

The system prompt file tells the LLM what to flag and what to ignore. It should instruct the LLM to respond with `NO_ISSUES` when everything is normal, or `SUBJECT: <summary>` followed by a detailed analysis when issues are found.

Works with any OpenAI-compatible API: OpenAI, Claude, Gemini (via OpenAI compat endpoint), Ollama, OpenRouter, etc.

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

            # option 1: per-secret files (works with sops-nix, agenix, etc.)
            email.passwordFile = "/run/secrets/friendly-ghost/smtp-password";

            # option 2: single env file with KEY=VALUE lines
            # environmentFile = "/run/secrets/friendly-ghost.env";

            # optional LLM analysis
            llm = {
              enable = true;
              apiUrl = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
              model = "gemini-2.5-flash";
              systemPromptFile = "/etc/friendly-ghost/prompt.txt";
              apiKeyFile = "/run/secrets/friendly-ghost/llm-api-key";
            };
          };
        }
      ];
    };
  };
}
```

### Secrets

There are two ways to provide secrets. Use one or the other, not both.

**Per-secret files** (recommended — works with sops-nix, agenix, and similar):

```nix
services.friendly-ghost = {
  email.passwordFile = "/run/secrets/friendly-ghost/smtp-password";
  llm.apiKeyFile = "/run/secrets/friendly-ghost/llm-api-key";
};
```

Each file should contain just the raw secret value, no trailing newline.

**Example with sops-nix:**

```nix
sops.secrets."friendly-ghost/smtp-password" = {};
sops.secrets."friendly-ghost/llm-api-key" = {};

services.friendly-ghost = {
  email.passwordFile = config.sops.secrets."friendly-ghost/smtp-password".path;
  llm.apiKeyFile = config.sops.secrets."friendly-ghost/llm-api-key".path;
};
```

**Environment file** (legacy — single file with `KEY=VALUE` lines):

```nix
services.friendly-ghost.environmentFile = "/run/secrets/friendly-ghost.env";
```

```
FRIENDLY_GHOST_SMTP_PASSWORD=secret
FRIENDLY_GHOST_LLM_API_KEY=your-key-here
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
