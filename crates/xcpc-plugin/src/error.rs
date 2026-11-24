use thiserror::Error;

pub type Result<T> = std::result::Result<T, PluginError>;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("INVALID_MANIFEST: {0}")]
    ManifestInvalid(String),

    #[error("PLUGIN_NOT_FOUND: plugin '{0}' not found")]
    NotFound(String),

    #[error("PLUGIN_DISABLED: plugin '{0}' is disabled")]
    Disabled(String),

    #[error("DOMAIN_NOT_ALLOWED: plugin '{plugin_id}' is not allowed to access host '{host}'")]
    DomainNotAllowed { plugin_id: String, host: String },

    #[error("PERMISSION_DENIED: plugin '{plugin_id}' lacks permission '{permission}'")]
    PermissionDenied {
        plugin_id: String,
        permission: String,
    },

    #[error("TIMEOUT: plugin '{plugin_id}' exceeded execution limit of {timeout_ms}ms")]
    Timeout { plugin_id: String, timeout_ms: u64 },

    #[error("MEMORY_LIMIT: plugin '{plugin_id}' exceeded memory limit of {limit_bytes} bytes")]
    MemoryLimit {
        plugin_id: String,
        limit_bytes: usize,
    },

    #[error("JS_ERROR: {0}")]
    JsError(String),

    #[error("HTTP_ERROR: {0}")]
    Http(String),

    #[error("CREDENTIAL_ERROR: {0}")]
    Credential(String),

    #[error("ZIP_SLIP: unsafe archive path '{0}'")]
    ZipSlip(String),

    #[error("CHECKSUM_MISMATCH: expected sha256 '{expected}', got '{actual}'")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("INVALID_PACKAGE: {0}")]
    InvalidPackage(String),

    #[error("RENDER_ERROR: {0}")]
    Render(String),

    #[error("IO_ERROR: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON_ERROR: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML_ERROR: {0}")]
    Toml(#[from] toml::de::Error),
}

impl PluginError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManifestInvalid(_) => "INVALID_MANIFEST",
            Self::NotFound(_) => "PLUGIN_NOT_FOUND",
            Self::Disabled(_) => "PLUGIN_DISABLED",
            Self::DomainNotAllowed { .. } => "DOMAIN_NOT_ALLOWED",
            Self::PermissionDenied { .. } => "PERMISSION_DENIED",
            Self::Timeout { .. } => "TIMEOUT",
            Self::MemoryLimit { .. } => "MEMORY_LIMIT",
            Self::JsError(_) => "JS_ERROR",
            Self::Http(_) => "HTTP_ERROR",
            Self::Credential(_) => "CREDENTIAL_ERROR",
            Self::ZipSlip(_) => "ZIP_SLIP",
            Self::ChecksumMismatch { .. } => "CHECKSUM_MISMATCH",
            Self::InvalidPackage(_) => "INVALID_PACKAGE",
            Self::Render(_) => "RENDER_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Json(_) => "JSON_ERROR",
            Self::Toml(_) => "TOML_ERROR",
        }
    }
}
