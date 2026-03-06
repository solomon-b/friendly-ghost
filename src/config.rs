use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::AppError;
use crate::filter::{IgnoreMatcher, UnitMatcher};
use crate::llm::BASE_SYSTEM_PROMPT;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub journal: JournalConfig,
    pub email: EmailConfig,
    pub state: StateConfig,
    pub llm: Option<LlmConfig>,
}

#[derive(Debug, Deserialize)]
pub struct JournalConfig {
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
}

impl EnvOverrides {
    pub fn from_env() -> Self {
        Self {
            smtp_password: std::env::var("FRIENDLY_GHOST_SMTP_PASSWORD").ok(),
            smtp_host: std::env::var("FRIENDLY_GHOST_SMTP_HOST").ok(),
            llm_api_key: std::env::var("FRIENDLY_GHOST_LLM_API_KEY").ok(),
        }
    }
}

/// Parse config from TOML file, then apply overrides.
pub fn load(path: &Path, overrides: EnvOverrides) -> Result<Config, AppError> {
    let content = std::fs::read_to_string(path).map_err(|e| AppError::ConfigRead {
        path: path.to_owned(),
        source: e,
    })?;
    let mut config: Config = toml::from_str(&content)?;

    if config.journal.units.is_empty() {
        return Err(AppError::Config(
            "journal.units must have at least one unit".into(),
        ));
    }
    if config.email.to.is_empty() {
        return Err(AppError::Config(
            "email.to must have at least one recipient".into(),
        ));
    }

    config.journal.unit_matcher = Some(UnitMatcher::new(&config.journal.units)?);
    if !config.journal.ignore_patterns.is_empty() {
        config.journal.ignore_matcher =
            Some(IgnoreMatcher::new(&config.journal.ignore_patterns)?);
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
                AppError::Config(
                    format!("failed to read system prompt file {path:?}: {e}").into(),
                )
            })?;
            prompt.push_str("\n\nAdditional operator instructions:\n");
            prompt.push_str(&addendum);
        }
        llm.system_prompt = prompt;
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_toml() -> &'static str {
        r#"
[journal]
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
        }
    }

    #[test]
    fn parses_valid_config() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(sample_toml().as_bytes()).unwrap();
        let config = load(tmp.path(), no_overrides()).unwrap();
        assert_eq!(config.journal.units, vec!["nginx", "sshd"]);
        assert_eq!(config.journal.priority, Priority::Err);
        assert_eq!(config.email.smtp_port, 587);
    }

    #[test]
    fn rejects_invalid_priority() {
        let bad = sample_toml().replace(r#"priority = "err""#, r#"priority = "banana""#);
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bad.as_bytes()).unwrap();
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn rejects_empty_units() {
        let bad = sample_toml().replace(r#"units = ["nginx", "sshd"]"#, "units = []");
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bad.as_bytes()).unwrap();
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn rejects_empty_recipients() {
        let bad = sample_toml().replace(r#"to = ["admin@example.com"]"#, "to = []");
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bad.as_bytes()).unwrap();
        assert!(load(tmp.path(), no_overrides()).is_err());
    }

    #[test]
    fn priority_ordering() {
        assert!(Priority::Emerg < Priority::Alert);
        assert!(Priority::Err < Priority::Warning);
    }

    #[test]
    fn priority_levels_match_ordering() {
        // Pinning test: ensures as_level() discriminants stay consistent with
        // derived Ord (which uses declaration order). If someone reorders the
        // enum variants, this test catches the divergence.
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
        // Verify Ord is consistent with numeric level
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
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(sample_toml().as_bytes()).unwrap();
        let overrides = EnvOverrides {
            smtp_password: Some("secret123".to_string()),
            smtp_host: None,
            llm_api_key: None,
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.email.password, Some("secret123".to_string()));
    }

    #[test]
    fn override_smtp_host() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(sample_toml().as_bytes()).unwrap();
        let overrides = EnvOverrides {
            smtp_password: None,
            smtp_host: Some("override.example.com".to_string()),
            llm_api_key: None,
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.email.smtp_host, "override.example.com");
    }

    #[test]
    fn builds_unit_matcher_on_load() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(sample_toml().as_bytes()).unwrap();
        let config = load(tmp.path(), no_overrides()).unwrap();
        let matcher = config.journal.unit_matcher.as_ref().unwrap();
        assert!(matcher.is_match("nginx"));
        assert!(matcher.is_match("sshd"));
        assert!(!matcher.is_match("postgres"));
    }

    #[test]
    fn rejects_invalid_regex_in_units() {
        let bad = sample_toml().replace(
            r#"units = ["nginx", "sshd"]"#,
            r#"units = ["[invalid"]"#,
        );
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bad.as_bytes()).unwrap();
        let err = load(tmp.path(), no_overrides()).unwrap_err().to_string();
        assert!(err.contains("invalid unit pattern"), "got: {err}");
    }

    #[test]
    fn loads_config_with_regex_units() {
        let with_regex = sample_toml().replace(
            r#"units = ["nginx", "sshd"]"#,
            r#"units = ["nginx", "web-.*"]"#,
        );
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(with_regex.as_bytes()).unwrap();
        let config = load(tmp.path(), no_overrides()).unwrap();
        let matcher = config.journal.unit_matcher.as_ref().unwrap();
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
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(with_llm.as_bytes()).unwrap();
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
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(with_llm.as_bytes()).unwrap();
        let config = load(tmp.path(), no_overrides()).unwrap();
        let llm = config.llm.unwrap();
        assert!(llm.system_prompt.starts_with(BASE_SYSTEM_PROMPT));
        assert!(llm.system_prompt.contains("Additional operator instructions:"));
        assert!(llm.system_prompt.contains("You are a watchdog."));
    }

    #[test]
    fn parses_config_without_llm_section() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(sample_toml().as_bytes()).unwrap();
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
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(with_llm.as_bytes()).unwrap();
        let overrides = EnvOverrides {
            smtp_password: None,
            smtp_host: None,
            llm_api_key: Some("sk-test-key".to_string()),
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.llm.unwrap().api_key, Some("sk-test-key".to_string()));
    }
}
