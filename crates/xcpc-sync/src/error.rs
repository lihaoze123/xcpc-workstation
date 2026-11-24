use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("plugin error: {0}")]
    Plugin(#[from] xcpc_plugin::PluginError),

    #[error("storage error: {0}")]
    Storage(#[from] xcpc_storage::Error),

    #[error("platform not found or disabled: {0}")]
    PlatformNotFound(String),

    #[error("invalid data directory: {0}")]
    InvalidDataDir(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, SyncError>;
