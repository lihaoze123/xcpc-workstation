pub mod builtin;
pub mod commands;
pub mod credentials;
pub mod error;
pub mod http;
pub mod io_ext;
pub mod kv;
pub mod manager;
pub mod manifest;
pub mod oj;
pub mod render;
pub mod runtime;
pub mod tool;
pub mod ui_session;
pub mod workspace;

pub use commands::{PluginCommandAction, RegisteredPluginCommand};
pub use credentials::{CredentialStore, MemoryCredentialStore, SystemKeyringStore};
pub use error::{PluginError, Result};
pub use http::{HttpClient, HttpRequest, HttpResponse, MockHttpClient, UreqHttpClient};
pub use io_ext::{
    ContestDraft, DraftSample, ExportContext, ExportFileEntry, ExportOutput, ImportAppliedReport,
    ImportDraft, ImportFileInput, ProblemDraft, RegisteredExporter, RegisteredImporter,
    apply_import_draft, write_export_output,
};
pub use kv::KvStore;
pub use manager::{
    InstalledPlugin, LoadedPlugin, OFFICIAL_PLUGIN_REPO_ENV, OFFICIAL_PLUGIN_REPO_NAME,
    OFFICIAL_PLUGIN_REPO_URL, PluginCheckIssue, PluginCheckReport, PluginLinkEntry, PluginManager,
    PluginOrigin, PluginPackReport, PluginRepoConfig, PluginTestReport, RepoCatalog,
    RepoPluginEntry, ScaffoldKind, check_plugin_dir, extract_zip_safe, init_plugin_scaffold,
    init_plugin_scaffold_vite, official_repo_url, pack_plugin_dir, sha256_hex, test_plugin_dir,
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
pub use render::{
    HighlightLine, HighlightResult, HighlightToken, RenderedRgba, highlight_code,
    svg_to_png_base64, svg_to_rgba,
};
pub use runtime::{CallResult, HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
pub use tool::{GraphEdge, GraphNode, PluginToolGroup, ToolExecutionResult, ToolOutputBlock};
pub use ui_session::{
    PluginUiSession, UI_SESSION_MEMORY_LIMIT_BYTES, UI_SESSION_TURN_TIMEOUT_MS, UiHostEffect,
    UiRenderOutput, UiShortcutSpec, UiWorkspaceBridge,
};
pub use workspace::{WorkspaceProblemDetail, WorkspaceProblemSummary, WorkspaceReader};

#[cfg(test)]
mod tests;
