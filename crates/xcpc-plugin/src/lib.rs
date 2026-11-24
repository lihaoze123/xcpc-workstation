pub mod builtin;
pub mod credentials;
pub mod error;
pub mod http;
pub mod kv;
pub mod manager;
pub mod manifest;
pub mod oj;
pub mod render;
pub mod runtime;
pub mod workspace;

pub use credentials::{CredentialStore, MemoryCredentialStore, SystemKeyringStore};
pub use error::{PluginError, Result};
pub use http::{HttpClient, HttpRequest, HttpResponse, MockHttpClient, UreqHttpClient};
pub use kv::KvStore;
pub use manager::{
    InstalledPlugin, LoadedPlugin, PluginManager, PluginOrigin, PluginRepoConfig, PluginTestReport,
    RepoCatalog, RepoPluginEntry, ScaffoldKind, extract_zip_safe, init_plugin_scaffold, sha256_hex,
    test_plugin_dir,
};
pub use manifest::{
    CommandContext, CommandSpec, CredentialFieldSpec, ExporterSpec, ImporterSpec, PluginCapability,
    PluginManifest, PluginPermissionSummary, PluginPermissions, ToolInputSpec, ToolSpec,
    is_domain_allowed,
};
pub use oj::{
    OjAccountSync, OjContest, OjContext, OjPluginClient, OjProblemDetail, OjRatingChange, OjSample,
    OjSubmission, OjSubmissionsBatch, OjSummarySync, OjVerifyResult, ParsedProblemUrl,
};
pub use render::svg_to_png_base64;
pub use runtime::{CallResult, HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
pub use workspace::{WorkspaceProblemDetail, WorkspaceProblemSummary, WorkspaceReader};

#[cfg(test)]
mod tests;
