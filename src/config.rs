use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::AppError;
use crate::filter::{IgnoreMatcher, UnitMatcher};
use crate::llm::BASE_SYSTEM_PROMPT;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub source: SourceConfig,
    pub filter: FilterConfig,
    pub email: EmailConfig,
    pub state: StateConfig,
    pub llm: Option<LlmConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceConfig {
    Journal(JournalSourceConfig),
    Loki(LokiSourceConfig),
}

/// Source-specific knobs for the systemd journal. Empty for now; this
/// section exists in the schema so journal-only options can be added
/// without another breaking change.
#[derive(Debug, Default, Deserialize)]
pub struct JournalSourceConfig {}

#[derive(Debug, Deserialize)]
pub struct LokiSourceConfig {
    pub url: String,
    pub query: String,
    #[serde(default = "default_auth_mode")]
    pub auth: LokiAuthMode,
    #[serde(default)]
    pub tenant_id: Option<String>,

    #[serde(default = "default_unit_label")]
    pub unit_label: String,
    #[serde(default)]
    pub unit_label_fallback: Vec<String>,

    #[serde(default = "default_host_label")]
    pub host_label: String,
    #[serde(default = "default_host_label_fallback")]
    pub host_label_fallback: Vec<String>,

    #[serde(default = "default_level_labels")]
    pub level_labels: Vec<String>,
    #[serde(default = "default_default_priority")]
    pub default_priority: u8,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_max_entries")]
    pub max_entries_per_query: u32,

    #[serde(default = "default_priority_mapping")]
    pub priority_mapping: HashMap<String, u8>,

    #[serde(skip)]
    pub credentials: LokiCredentials,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LokiAuthMode {
    None,
    Bearer,
    Basic,
}

#[derive(Debug, Default, Clone)]
pub struct LokiCredentials {
    pub bearer_token: Option<String>,
    pub basic_user: Option<String>,
    pub basic_password: Option<String>,
}

fn default_auth_mode() -> LokiAuthMode {
    LokiAuthMode::None
}
fn default_unit_label() -> String {
    "service_name".to_string()
}
fn default_host_label() -> String {
    "host".to_string()
}
fn default_host_label_fallback() -> Vec<String> {
    vec![
        "hostname".to_string(),
        "instance".to_string(),
        "nodename".to_string(),
    ]
}
fn default_level_labels() -> Vec<String> {
    vec![
        "level".to_string(),
        "severity".to_string(),
        "lvl".to_string(),
    ]
}
fn default_default_priority() -> u8 {
    6
}
fn default_timeout_seconds() -> u64 {
    30
}
fn default_max_entries() -> u32 {
    5000
}
fn default_priority_mapping() -> HashMap<String, u8> {
    [
        ("emerg", 0),
        ("fatal", 0),
        ("alert", 1),
        ("crit", 2),
        ("critical", 2),
        ("error", 3),
        ("err", 3),
        ("warning", 4),
        ("warn", 4),
        ("notice", 5),
        ("info", 6),
        ("debug", 7),
        ("trace", 7),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[derive(Debug, Deserialize)]
pub struct FilterConfig {
    pub units: Vec<String>,
    pub priority: Priority,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    #[serde(skip)]
    pub unit_matcher: Option<UnitMatcher>,
    #[serde(skip)]
    pub ignore_matcher: Option<IgnoreMatcher>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub subject_prefix: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StateConfig {
    pub cursor_file: PathBuf,
}

fn default_system_prompt() -> String {
    BASE_SYSTEM_PROMPT.to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub api_url: String,
    pub model: String,
    pub system_prompt_file: Option<PathBuf>,
    pub temperature: f64,
    pub max_tokens: u32,
    #[serde(skip)]
    pub api_key: Option<String>,
    #[serde(skip, default = "default_system_prompt")]
    pub system_prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub enum Priority {
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl FromStr for Priority {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, AppError> {
        match s.to_lowercase().as_str() {
            "emerg" => Ok(Self::Emerg),
            "alert" => Ok(Self::Alert),
            "crit" => Ok(Self::Crit),
            "err" => Ok(Self::Err),
            "warning" => Ok(Self::Warning),
            "notice" => Ok(Self::Notice),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            other => Err(AppError::Config(format!("unknown priority: {other}").into())),
        }
    }
}

impl TryFrom<String> for Priority {
    type Error = AppError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl Priority {
    pub fn as_level(self) -> u8 {
        self as u8
    }
}

/// Environment variable overrides for config values.
#[derive(Debug)]
pub struct EnvOverrides {
    pub smtp_password: Option<String>,
    pub smtp_host: Option<String>,
    pub llm_api_key: Option<String>,
    pub loki_bearer_token: Option<String>,
    pub loki_basic_user: Option<String>,
    pub loki_basic_password: Option<String>,
}

impl EnvOverrides {
    pub fn from_env() -> Self {
        Self {
            smtp_password: std::env::var("FRIENDLY_GHOST_SMTP_PASSWORD").ok(),
            smtp_host: std::env::var("FRIENDLY_GHOST_SMTP_HOST").ok(),
            llm_api_key: std::env::var("FRIENDLY_GHOST_LLM_API_KEY").ok(),
            loki_bearer_token: std::env::var("FRIENDLY_GHOST_LOKI_BEARER_TOKEN").ok(),
            loki_basic_user: std::env::var("FRIENDLY_GHOST_LOKI_BASIC_USER").ok(),
            loki_basic_password: std::env::var("FRIENDLY_GHOST_LOKI_BASIC_PASSWORD").ok(),
        }
    }
}

/// Parse config from TOML file, then apply overrides.
pub fn load(path: &Path, overrides: EnvOverrides) -> Result<Config, AppError> {
    let content = std::fs::read_to_string(path).map_err(|e| AppError::ConfigRead {
        path: path.to_owned(),
        source: e,
    })?;

    // Detect the old v0.1 schema before serde's error message obscures the cause.
    let raw: toml::Value = content.parse()?;
    if let Some(table) = raw.as_table() {
        if table.contains_key("journal") && !table.contains_key("source") {
            return Err(AppError::Config(
                "config schema changed in v0.2.0: rename [journal] to [filter] and add a \
                 [source] section with type = \"journal\" or \"loki\". \
                 See config.example.toml for the new layout."
                    .into(),
            ));
        }
    }
    let mut config: Config = raw.try_into()?;

    if config.filter.units.is_empty() {
        return Err(AppError::Config(
            "filter.units must have at least one unit".into(),
        ));
    }
    if config.email.to.is_empty() {
        return Err(AppError::Config(
            "email.to must have at least one recipient".into(),
        ));
    }

    config.filter.unit_matcher = Some(UnitMatcher::new(&config.filter.units)?);
    if !config.filter.ignore_patterns.is_empty() {
        config.filter.ignore_matcher = Some(IgnoreMatcher::new(&config.filter.ignore_patterns)?);
    }

    if let SourceConfig::Loki(ref mut loki) = config.source {
        validate_loki(loki)?;
        loki.credentials = LokiCredentials {
            bearer_token: overrides.loki_bearer_token,
            basic_user: overrides.loki_basic_user,
            basic_password: overrides.loki_basic_password,
        };
        match loki.auth {
            LokiAuthMode::Bearer if loki.credentials.bearer_token.is_none() => {
                return Err(AppError::Config(
                    "[source.loki].auth = \"bearer\" requires FRIENDLY_GHOST_LOKI_BEARER_TOKEN \
                     to be set in the environment"
                        .into(),
                ));
            }
            LokiAuthMode::Basic
                if loki.credentials.basic_user.is_none()
                    || loki.credentials.basic_password.is_none() =>
            {
                return Err(AppError::Config(
                    "[source.loki].auth = \"basic\" requires both \
                     FRIENDLY_GHOST_LOKI_BASIC_USER and FRIENDLY_GHOST_LOKI_BASIC_PASSWORD \
                     to be set in the environment"
                        .into(),
                ));
            }
            _ => {}
        }
    }

    if let Some(pw) = overrides.smtp_password {
        config.email.password = Some(pw);
    }
    if let Some(host) = overrides.smtp_host {
        config.email.smtp_host = host;
    }

    if let Some(ref mut llm) = config.llm {
        llm.api_key = overrides.llm_api_key;

        let mut prompt = BASE_SYSTEM_PROMPT.to_string();
        if let Some(ref path) = llm.system_prompt_file {
            let addendum = std::fs::read_to_string(path).map_err(|e| {
                AppError::Config(format!("failed to read system prompt file {path:?}: {e}").into())
            })?;
            prompt.push_str("\n\nAdditional operator instructions:\n");
            prompt.push_str(&addendum);
        }
        llm.system_prompt = prompt;
    }

    Ok(config)
}

fn validate_loki(loki: &LokiSourceConfig) -> Result<(), AppError> {
    if reqwest::Url::parse(&loki.url).is_err() {
        return Err(AppError::Config(
            format!("[source.loki].url is not a valid URL: {}", loki.url).into(),
        ));
    }
    if loki.query.trim().is_empty() {
        return Err(AppError::Config(
            "[source.loki].query must be a non-empty LogQL selector".into(),
        ));
    }
    if loki.default_priority > 7 {
        return Err(AppError::Config(
            format!(
                "[source.loki].default_priority must be 0..=7, got {}",
                loki.default_priority
            )
            .into(),
        ));
    }
    for (k, v) in &loki.priority_mapping {
        if *v > 7 {
            return Err(AppError::Config(
                format!("[source.loki].priority_mapping.{k} must be 0..=7, got {v}").into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_toml() -> &'static str {
        r#"
[source]
type = "journal"

[filter]
units = ["nginx", "sshd"]
priority = "err"

[email]
smtp_host = "mail.example.com"
smtp_port = 587
username = "alerts@example.com"
from = "alerts@example.com"
to = ["admin@example.com"]
subject_prefix = "[friendly-ghost]"

[state]
cursor_file = "/tmp/friendly-ghost-cursor"
"#
    }

    fn loki_sample_toml() -> &'static str {
        r#"
[source]
type = "loki"
url = "http://loki.example.com:3100"
query = '{job=~"nginx|sshd"}'
auth = "none"

[filter]
units = ["nginx", "sshd"]
priority = "err"

[email]
smtp_host = "mail.example.com"
smtp_port = 587
username = "alerts@example.com"
from = "alerts@example.com"
to = ["admin@example.com"]
subject_prefix = "[friendly-ghost]"

[state]
cursor_file = "/tmp/friendly-ghost-cursor"
"#
    }

    fn no_overrides() -> EnvOverrides {
        EnvOverrides {
            smtp_password: None,
            smtp_host: None,
            llm_api_key: None,
            loki_bearer_token: None,
            loki_basic_user: None,
            loki_basic_password: None,
        }
    }

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp
    }

    #[test]
    fn parses_valid_journal_config() {
        let tmp = write_tmp(sample_toml());
        let config = load(tmp.path(), no_overrides()).unwrap();
        assert_eq!(config.filter.units, vec!["nginx", "sshd"]);
        assert_eq!(config.filter.priority, Priority::Err);
        assert_eq!(config.email.smtp_port, 587);
        assert!(matches!(config.source, SourceConfig::Journal(_)));
    }

    #[test]
    fn parses_valid_loki_config() {
        let tmp = write_tmp(loki_sample_toml());
        let config = load(tmp.path(), no_overrides()).unwrap();
        match config.source {
            SourceConfig::Loki(loki) => {
                assert_eq!(loki.url, "http://loki.example.com:3100");
                assert_eq!(loki.unit_label, "service_name");
                assert_eq!(loki.host_label, "host");
                assert_eq!(loki.default_priority, 6);
                assert!(loki.priority_mapping.contains_key("error"));
            }
            _ => panic!("expected Loki variant"),
        }
    }

    #[test]
    fn old_schema_produces_migration_hint() {
        let old = r#"
[journal]
units = ["nginx"]
priority = "err"

[email]
smtp_host = "x"
smtp_port = 1
username = "u"
from = "x@y.z"
to = ["a@b.c"]
subject_prefix = "x"

[state]
cursor_file = "/tmp/x"
"#;
        let tmp = write_tmp(old);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("rename [journal] to [filter]"), "got: {err}");
        assert!(err.contains("[source]"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_priority() {
        let bad = sample_toml().replace(r#"priority = "err""#, r#"priority = "banana""#);
        let tmp = write_tmp(&bad);
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn rejects_empty_units() {
        let bad = sample_toml().replace(r#"units = ["nginx", "sshd"]"#, "units = []");
        let tmp = write_tmp(&bad);
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn rejects_empty_recipients() {
        let bad = sample_toml().replace(r#"to = ["admin@example.com"]"#, "to = []");
        let tmp = write_tmp(&bad);
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn rejects_loki_invalid_url() {
        let bad = loki_sample_toml().replace(
            r#"url = "http://loki.example.com:3100""#,
            r#"url = "not a url""#,
        );
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("not a valid URL"), "got: {err}");
    }

    #[test]
    fn rejects_loki_empty_query() {
        let bad = loki_sample_toml().replace(r#"query = '{job=~"nginx|sshd"}'"#, r#"query = '   '"#);
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("non-empty LogQL selector"), "got: {err}");
    }

    #[test]
    fn rejects_loki_bearer_without_token() {
        let bad = loki_sample_toml().replace(r#"auth = "none""#, r#"auth = "bearer""#);
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(
            err.contains("FRIENDLY_GHOST_LOKI_BEARER_TOKEN"),
            "got: {err}"
        );
    }

    #[test]
    fn loki_bearer_picks_up_token_override() {
        let toml_text = loki_sample_toml().replace(r#"auth = "none""#, r#"auth = "bearer""#);
        let tmp = write_tmp(&toml_text);
        let overrides = EnvOverrides {
            loki_bearer_token: Some("hunter2".to_string()),
            ..no_overrides()
        };
        let config = load(tmp.path(), overrides).unwrap();
        match config.source {
            SourceConfig::Loki(loki) => {
                assert_eq!(loki.credentials.bearer_token.as_deref(), Some("hunter2"));
            }
            _ => panic!("expected Loki"),
        }
    }

    #[test]
    fn rejects_loki_basic_without_credentials() {
        let bad = loki_sample_toml().replace(r#"auth = "none""#, r#"auth = "basic""#);
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(
            err.contains("FRIENDLY_GHOST_LOKI_BASIC_USER"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_invalid_priority_mapping_value() {
        let bad = format!(
            "{}\n[source.priority_mapping]\nerror = 99\n",
            loki_sample_toml()
        );
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("must be 0..=7"), "got: {err}");
    }

    #[test]
    fn priority_ordering() {
        assert!(Priority::Emerg < Priority::Alert);
        assert!(Priority::Err < Priority::Warning);
    }

    #[test]
    fn priority_levels_match_ordering() {
        let all = [
            (Priority::Emerg, 0),
            (Priority::Alert, 1),
            (Priority::Crit, 2),
            (Priority::Err, 3),
            (Priority::Warning, 4),
            (Priority::Notice, 5),
            (Priority::Info, 6),
            (Priority::Debug, 7),
        ];
        for (priority, expected_level) in &all {
            assert_eq!(priority.as_level(), *expected_level);
        }
        for window in all.windows(2) {
            assert!(window[0].0 < window[1].0);
        }
    }

    #[test]
    fn priority_from_str_case_insensitive() {
        assert_eq!("ERR".parse::<Priority>().unwrap(), Priority::Err);
        assert_eq!("Warning".parse::<Priority>().unwrap(), Priority::Warning);
        assert_eq!("EMERG".parse::<Priority>().unwrap(), Priority::Emerg);
    }

    #[test]
    fn priority_from_str_rejects_numeric() {
        assert!("3".parse::<Priority>().is_err());
        assert!("0".parse::<Priority>().is_err());
    }

    #[test]
    fn override_password() {
        let tmp = write_tmp(sample_toml());
        let overrides = EnvOverrides {
            smtp_password: Some("secret123".to_string()),
            ..no_overrides()
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.email.password, Some("secret123".to_string()));
    }

    #[test]
    fn override_smtp_host() {
        let tmp = write_tmp(sample_toml());
        let overrides = EnvOverrides {
            smtp_host: Some("override.example.com".to_string()),
            ..no_overrides()
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.email.smtp_host, "override.example.com");
    }

    #[test]
    fn builds_unit_matcher_on_load() {
        let tmp = write_tmp(sample_toml());
        let config = load(tmp.path(), no_overrides()).unwrap();
        let matcher = config.filter.unit_matcher.as_ref().unwrap();
        assert!(matcher.is_match("nginx"));
        assert!(matcher.is_match("sshd"));
        assert!(!matcher.is_match("postgres"));
    }

    #[test]
    fn rejects_invalid_regex_in_units() {
        let bad = sample_toml().replace(r#"units = ["nginx", "sshd"]"#, r#"units = ["[invalid"]"#);
        let tmp = write_tmp(&bad);
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("invalid unit pattern"), "got: {err}");
    }

    #[test]
    fn loads_config_with_regex_units() {
        let with_regex =
            sample_toml().replace(r#"units = ["nginx", "sshd"]"#, r#"units = ["nginx", "web-.*"]"#);
        let tmp = write_tmp(&with_regex);
        let config = load(tmp.path(), no_overrides()).unwrap();
        let matcher = config.filter.unit_matcher.as_ref().unwrap();
        assert!(matcher.is_match("nginx"));
        assert!(matcher.is_match("web-frontend"));
        assert!(!matcher.is_match("sshd"));
    }

    #[test]
    fn parses_config_with_llm_section_no_prompt_file() {
        let with_llm = format!(
            r#"{}
[llm]
api_url = "https://api.example.com/v1/chat/completions"
model = "gpt-4"
temperature = 0.1
max_tokens = 4096
"#,
            sample_toml(),
        );
        let tmp = write_tmp(&with_llm);
        let config = load(tmp.path(), no_overrides()).unwrap();
        let llm = config.llm.unwrap();
        assert_eq!(llm.model, "gpt-4");
        assert_eq!(llm.temperature, 0.1);
        assert_eq!(llm.max_tokens, 4096);
        assert_eq!(llm.system_prompt, BASE_SYSTEM_PROMPT);
    }

    #[test]
    fn llm_config_deserializes_with_base_prompt_default() {
        let toml_str = r#"
api_url = "https://api.example.com/v1/chat/completions"
model = "gpt-4"
temperature = 0.1
max_tokens = 4096
"#;
        let llm: LlmConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(llm.system_prompt, BASE_SYSTEM_PROMPT);
        assert!(llm.system_prompt_file.is_none());
    }

    #[test]
    fn parses_config_with_llm_prompt_file_appends() {
        let prompt_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(prompt_file.path(), "You are a watchdog.").unwrap();

        let with_llm = format!(
            r#"{}
[llm]
api_url = "https://api.example.com/v1/chat/completions"
model = "gpt-4"
system_prompt_file = {:?}
temperature = 0.1
max_tokens = 4096
"#,
            sample_toml(),
            prompt_file.path()
        );
        let tmp = write_tmp(&with_llm);
        let config = load(tmp.path(), no_overrides()).unwrap();
        let llm = config.llm.unwrap();
        assert!(llm.system_prompt.starts_with(BASE_SYSTEM_PROMPT));
        assert!(llm.system_prompt.contains("Additional operator instructions:"));
        assert!(llm.system_prompt.contains("You are a watchdog."));
    }

    #[test]
    fn parses_config_without_llm_section() {
        let tmp = write_tmp(sample_toml());
        let config = load(tmp.path(), no_overrides()).unwrap();
        assert!(config.llm.is_none());
    }

    #[test]
    fn override_llm_api_key() {
        let with_llm = format!(
            r#"{}
[llm]
api_url = "https://api.example.com/v1/chat/completions"
model = "gpt-4"
temperature = 0.1
max_tokens = 4096
"#,
            sample_toml(),
        );
        let tmp = write_tmp(&with_llm);
        let overrides = EnvOverrides {
            llm_api_key: Some("sk-test-key".to_string()),
            ..no_overrides()
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.llm.unwrap().api_key, Some("sk-test-key".to_string()));
    }
}
