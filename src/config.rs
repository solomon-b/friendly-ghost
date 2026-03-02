use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub journal: JournalConfig,
    pub email: EmailConfig,
    pub state: StateConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JournalConfig {
    pub units: Vec<String>,
    pub priority: Priority,
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
}

impl EnvOverrides {
    pub fn from_env() -> Self {
        Self {
            smtp_password: std::env::var("FRIENDLY_GHOST_SMTP_PASSWORD").ok(),
            smtp_host: std::env::var("FRIENDLY_GHOST_SMTP_HOST").ok(),
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

    if let Some(pw) = overrides.smtp_password {
        config.email.password = Some(pw);
    }
    if let Some(host) = overrides.smtp_host {
        config.email.smtp_host = host;
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
        };
        let config = load(tmp.path(), overrides).unwrap();
        assert_eq!(config.email.smtp_host, "override.example.com");
    }
}
