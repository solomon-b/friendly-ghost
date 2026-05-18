# friendly-ghost 👻

Watches your logs and sends email alerts when anything matches your filters. Reads from the local systemd journal or a remote Grafana Loki server.

## Usage

```
friendly-ghost --config /etc/friendly-ghost/config.toml
friendly-ghost --config config.toml --dry-run
```

On first run, records a bookmark against the source. On subsequent runs, fetches entries since the bookmark, applies your filter rules, and emails a report if anything matches. Designed to run from cron or a systemd timer.

## Log sources

Pick one in `[source]`:

```toml
[source]
type = "journal"   # or "loki"
```

The rest of the pipeline (filter, LLM analysis, email report) is the same for both. Reports are grouped by host, which matters mostly for Loki since a single deployment can watch many machines.

### Systemd journal

The default. Reads the local journal via `journalctl --output=json`. No additional configuration needed under `[source]` — `type = "journal"` is sufficient.

### Grafana Loki

```toml
[source]
type = "loki"
url = "http://loki.example.com:3100"
query = '{job=~"nginx|sshd"}'   # LogQL selector
auth = "bearer"                 # "none" | "bearer" | "basic"
tenant_id = "team-a"            # optional X-Scope-OrgID
```

Loki labels map to friendly-ghost fields:

| field      | Loki label (default)              | fallback chain (configurable)                |
|------------|-----------------------------------|----------------------------------------------|
| `unit`     | `service_name`                    | none by default; set `unit_label_fallback`   |
| `host`     | `host`                            | `hostname`, `instance`, `nodename`           |
| `priority` | from `level`/`severity`/`lvl`     | falls back to `default_priority` (6)         |

The label value maps to a syslog-style priority (0–7) via `[source.priority_mapping]`. Defaults cover the common keywords: `error`/`err` → 3, `warn`/`warning` → 4, `crit`/`critical` → 2, `info` → 6, `debug` → 7, etc.

**Auth secrets always come from environment variables**, never from the TOML:

```
FRIENDLY_GHOST_LOKI_BEARER_TOKEN      # for auth = "bearer"
FRIENDLY_GHOST_LOKI_BASIC_USER        # for auth = "basic"
FRIENDLY_GHOST_LOKI_BASIC_PASSWORD    # for auth = "basic"
```

The state file (`[state].cursor_file`) stores either a journal cursor or a Loki timestamp. Switching `[source].type` against a stale state file from the other source produces a friendly error telling you to delete the file and bootstrap fresh.

## Configuration

Copy `config.example.toml` and edit. The filter rules apply to both source types:

```toml
[filter]
# Regex against the unit name as the source emits it. Journal source carries
# full systemd unit names ("nginx.service", "init.scope"); Loki source uses
# whatever your `unit_label` resolves to. Patterns are auto-anchored.
units = ['nginx\.service', 'sshd\.service', 'web-.*\.service']
priority = "err"                      # emerg, alert, crit, err, warning, notice, info, debug
ignore_patterns = ["Connection reset by peer"]   # optional

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

## LLM analysis (optional)

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

Consider instructing the LLM to include a `Suggested filter:` line with a regex pattern for each finding. If the finding turns out to be a false alarm, you can copy the pattern into `ignore_patterns` to suppress it in future runs.

For Loki sources, entries are rendered as `host/unit` in the prompt so the model can correlate per-host patterns.

Works with any OpenAI-compatible API: OpenAI, Claude, Gemini (via OpenAI compat endpoint), Ollama, OpenRouter, etc.

## NixOS module

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

            source = "journal";  # or "loki"

            filter = {
              units = [ ''nginx\.service'' ''sshd\.service'' ''web-.*\.service'' ];
              priority = "err";
              ignorePatterns = [ "Connection reset by peer" ];
            };

            email = {
              smtpHost = "mail.example.com";
              smtpPort = 587;
              username = "alerts@example.com";
              from = "alerts@example.com";
              to = [ "admin@example.com" ];
              subjectPrefix = "[friendly-ghost]";
            };

            email.passwordFile = "/run/secrets/friendly-ghost/smtp-password";

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

For Loki, fill in the `loki` submodule:

```nix
services.friendly-ghost = {
  source = "loki";

  loki = {
    url = "http://loki.example.com:3100";
    query = ''{job=~"nginx|sshd"}'';
    auth = "bearer";
    bearerTokenFile = "/run/secrets/friendly-ghost/loki-token";
    # tenantId = "team-a";
    # hostLabel = "host";        # override default if your shipper uses something else
  };
};
```

### Secrets

There are two ways to provide secrets. Use one or the other, not both.

**Per-secret files** (recommended — works with sops-nix, agenix, and similar):

```nix
services.friendly-ghost = {
  email.passwordFile = "/run/secrets/friendly-ghost/smtp-password";
  llm.apiKeyFile = "/run/secrets/friendly-ghost/llm-api-key";
  loki.bearerTokenFile = "/run/secrets/friendly-ghost/loki-token";
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

**Environment file** (single file with `KEY=VALUE` lines):

```nix
services.friendly-ghost.environmentFile = "/run/secrets/friendly-ghost.env";
```

```
FRIENDLY_GHOST_SMTP_PASSWORD=secret
FRIENDLY_GHOST_LLM_API_KEY=your-key-here
FRIENDLY_GHOST_LOKI_BEARER_TOKEN=loki-token
```

The module creates a systemd timer and service with `DynamicUser`, `StateDirectory`, and (for the journal source) `systemd-journal` group membership.

## Building

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
