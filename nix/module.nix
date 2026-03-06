self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.friendly-ghost;
  inherit (lib) mkEnableOption mkOption mkIf types;

  configFile = (pkgs.formats.toml {}).generate "friendly-ghost.toml" ({
    journal = {
      units = cfg.journal.units;
      priority = cfg.journal.priority;
    } // lib.optionalAttrs (cfg.journal.ignorePatterns != []) {
      ignore_patterns = cfg.journal.ignorePatterns;
    };
    email = {
      smtp_host = cfg.email.smtpHost;
      smtp_port = cfg.email.smtpPort;
      username = cfg.email.username;
      from = cfg.email.from;
      to = cfg.email.to;
      subject_prefix = cfg.email.subjectPrefix;
    };
    state = {
      cursor_file = "/var/lib/friendly-ghost/cursor";
    };
  } // lib.optionalAttrs cfg.llm.enable {
    llm = {
      api_url = cfg.llm.apiUrl;
      model = cfg.llm.model;
      temperature = cfg.llm.temperature;
      max_tokens = cfg.llm.maxTokens;
    } // lib.optionalAttrs (cfg.llm.systemPromptFile != null) {
      system_prompt_file = cfg.llm.systemPromptFile;
    };
  });
in
{
  options.services.friendly-ghost = {
    enable = mkEnableOption "friendly-ghost systemd journal monitor";

    interval = mkOption {
      type = types.str;
      default = "*:0/5";
      description = "Systemd OnCalendar expression for how often to run.";
      example = "hourly";
    };

    journal = {
      units = mkOption {
        type = types.listOf types.str;
        description = "Systemd units to monitor. Supports regex patterns (auto-anchored).";
        example = [ "nginx" "sshd" "web-.*" ];
      };

      priority = mkOption {
        type = types.enum [ "emerg" "alert" "crit" "err" "warning" "notice" "info" "debug" ];
        default = "err";
        description = "Minimum priority level to report. Lower = more severe.";
      };

      ignorePatterns = mkOption {
        type = types.listOf types.str;
        default = [];
        description = "Regex patterns matched against log message text. Entries matching any pattern are dropped before analysis.";
        example = [ "Connection reset by peer" "upstream timed out.*retrying" ];
      };
    };

    email = {
      smtpHost = mkOption {
        type = types.str;
        description = "SMTP server hostname.";
        example = "mail.example.com";
      };

      smtpPort = mkOption {
        type = types.port;
        default = 587;
        description = "SMTP server port.";
      };

      username = mkOption {
        type = types.str;
        description = "SMTP username.";
      };

      from = mkOption {
        type = types.str;
        description = "Sender email address.";
        example = "alerts@example.com";
      };

      to = mkOption {
        type = types.listOf types.str;
        description = "Recipient email addresses.";
        example = [ "admin@example.com" ];
      };

      subjectPrefix = mkOption {
        type = types.str;
        default = "[friendly-ghost]";
        description = "Prefix for email subject lines.";
      };

      passwordFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Path to a file containing the SMTP password. The file should contain the raw secret with no trailing newline. Compatible with sops-nix, agenix, and similar secret managers.";
        example = "/run/secrets/friendly-ghost/smtp-password";
      };
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description = "Path to environment file containing FRIENDLY_GHOST_SMTP_PASSWORD and optionally FRIENDLY_GHOST_LLM_API_KEY.";
      example = "/run/secrets/friendly-ghost.env";
    };

    llm = {
      enable = mkEnableOption "LLM-based anomaly detection";

      apiUrl = mkOption {
        type = types.str;
        description = "OpenAI-compatible chat completions API URL.";
        example = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
      };

      model = mkOption {
        type = types.str;
        description = "Model name to use.";
        example = "gemini-2.5-flash";
      };

      systemPromptFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Path to a file with additional instructions appended to the built-in system prompt. Optional.";
        example = "/etc/friendly-ghost/prompt.txt";
      };

      temperature = mkOption {
        type = types.float;
        default = 0.1;
        description = "Sampling temperature for LLM responses.";
      };

      maxTokens = mkOption {
        type = types.int;
        default = 4096;
        description = "Maximum tokens in LLM response.";
      };

      apiKeyFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Path to a file containing the LLM API key. The file should contain the raw secret with no trailing newline. Compatible with sops-nix, agenix, and similar secret managers.";
        example = "/run/secrets/friendly-ghost/llm-api-key";
      };
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = !(cfg.email.passwordFile != null && cfg.environmentFile != null);
        message = "services.friendly-ghost: both email.passwordFile and environmentFile are set. email.passwordFile takes precedence for FRIENDLY_GHOST_SMTP_PASSWORD.";
      }
    ];

    systemd.services.friendly-ghost = {
      description = "friendly-ghost journal monitor";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = let
          wrapper = pkgs.writeShellScript "friendly-ghost-start" ''
            ${lib.optionalString (cfg.email.passwordFile != null) ''
              FRIENDLY_GHOST_SMTP_PASSWORD="$(< ${lib.escapeShellArg cfg.email.passwordFile})"
              export FRIENDLY_GHOST_SMTP_PASSWORD
            ''}
            ${lib.optionalString (cfg.llm.enable && cfg.llm.apiKeyFile != null) ''
              FRIENDLY_GHOST_LLM_API_KEY="$(< ${lib.escapeShellArg cfg.llm.apiKeyFile})"
              export FRIENDLY_GHOST_LLM_API_KEY
            ''}
            exec ${self.packages.${pkgs.system}.default}/bin/friendly-ghost --config ${configFile}
          '';
        in "${wrapper}";
        DynamicUser = true;
        StateDirectory = "friendly-ghost";
        SupplementaryGroups = [ "systemd-journal" ];
      } // lib.optionalAttrs (cfg.environmentFile != null) {
        EnvironmentFile = cfg.environmentFile;
      };
    };

    systemd.timers.friendly-ghost = {
      description = "Run friendly-ghost periodically";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = cfg.interval;
        Persistent = true;
      };
    };
  };
}
