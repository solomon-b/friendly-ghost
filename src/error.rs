use std::borrow::Cow;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(Cow<'static, str>),

    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("journal error: {0}")]
    Journal(Cow<'static, str>),

    #[error("cursor file error at {path}: {source}")]
    CursorFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("email error: {0}")]
    Email(Cow<'static, str>),

    #[error("llm error: {0}")]
    Llm(Cow<'static, str>),
}
