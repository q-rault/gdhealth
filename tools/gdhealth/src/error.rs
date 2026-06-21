use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, GdHealthError>;

#[derive(Debug, thiserror::Error)]
pub enum GdHealthError {
    #[error("failed to read file: {path}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write file: {path}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config: {0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("failed to serialize json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid glob pattern `{pattern}`: {message}")]
    InvalidGlob { pattern: String, message: String },
    #[error("invalid severity `{severity}`")]
    InvalidSeverity { severity: String },
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("policy threshold triggered: {message}")]
    FailOnTriggered { message: String },
}
