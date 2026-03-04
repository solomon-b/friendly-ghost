self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.friendly-ghost;
  inherit (lib) mkEnableOption mkOption mkIf types;

  configFile = (pkgs.formats.toml {}).generate "friendly-ghost.toml" ({
    journal = {
      units = cfg.journal.units;
      priority = cfg.journal.priority;
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
      system_prompt_file = cfg.llm.systemPromptFile;
      temperature = cfg.llm.temperature;
      max_tokens = cfg.llm.maxTokens;
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
        type = types.path;
        description = "Path to the system prompt file.";
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
    };
  };

  config = mkIf cfg.enable {
    systemd.services.friendly-ghost = {
      description = "friendly-ghost journal monitor";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${self.packages.${pkgs.system}.default}/bin/friendly-ghost --config ${configFile}";
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
