pub mod archive;
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
pub mod workspace;

pub use archive::{
    CreateZipEntry, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_INPUT_BYTES, MAX_ARCHIVE_UNPACKED_BYTES,
    ZipEntryData, ZipEntryMeta, create_zip, list_zip, read_zip, sha256_encoded,
};
pub use commands::{PluginCommandAction, RegisteredPluginCommand};
pub use credentials::{CredentialStore, MemoryCredentialStore, SystemKeyringStore};
pub use error::{PluginError, Result};
pub use http::{
    HttpClient, HttpRequest, HttpResponse, HttpResponseType, MockHttpClient, UreqHttpClient,
};
pub use io_ext::{
    ContestDraft, DraftSample, ExportContext, ExportFileEntry, ExportOutput, ImportAppliedReport,
    ImportDraft, ImportFileInput, ProblemDraft, RegisteredExporter, RegisteredImporter,
    apply_import_draft, write_export_output,
};
pub use kv::KvStore;
pub use manager::{
    AggregatedRepoCatalog, AggregatedRepoPlugin, InstalledPlugin, LEGACY_OFFICIAL_PLUGIN_REPO_URL,
    LoadedPlugin, LoadedPluginUiHtml, MAX_PLUGIN_FONT_FILE_BYTES, MAX_PLUGIN_UNPACKED_TOTAL_BYTES,
    OFFICIAL_PLUGIN_REPO_ENV, OFFICIAL_PLUGIN_REPO_NAME, OFFICIAL_PLUGIN_REPO_URL,
    OFFICIAL_PLUGIN_SDK_PACKAGE, OFFICIAL_PLUGIN_SDK_VERSION, PluginCheckIssue, PluginCheckReport,
    PluginLinkEntry, PluginManager, PluginOrigin, PluginPackReport, PluginRepoConfig,
    PluginTestReport, RepoCatalog, RepoFetchError, RepoPluginEntry, ScaffoldKind, check_plugin_dir,
    extract_zip_safe, init_plugin_scaffold, init_plugin_scaffold_vite, official_repo_url,
    pack_plugin_dir, sha256_hex, test_plugin_dir, test_plugin_dir_with_writer,
    validate_plugin_package_limits, validate_repo_url,
};
pub use manifest::{
    CommandContext, CommandSpec, CredentialFieldSpec, ExporterSpec, ImporterSpec, PluginCapability,
    PluginManifest, PluginPermissionSummary, PluginPermissions, TemplateSpec, ToolInputSpec,
    ToolSpec, is_domain_allowed, requires_trust,
};
pub use oj::{
    OjAccountSync, OjContest, OjContext, OjPluginClient, OjProblemDetail, OjRatingChange, OjSample,
    OjSubmission, OjSubmissionsBatch, OjSummarySync, OjVerifyResult, ParsedProblemUrl,
};
pub use render::{
    HighlightLine, HighlightResult, HighlightToken, RenderMathOptions, RenderTypstOptions,
    RenderedRgba, RenderedTypstOutput, highlight_code, render_math_snippet, render_typst_snippet,
    svg_to_png_base64, svg_to_rgba,
};
pub use runtime::{CallResult, HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
pub use tool::{GraphEdge, GraphNode, PluginToolGroup, ToolExecutionResult, ToolOutputBlock};
pub use workspace::{
    CreateProblemInput, MAX_PLUGIN_SAMPLE_BYTES, MAX_PLUGIN_SAMPLES, SetSamplesResult,
    StorageWorkspaceBridge, UpdateProblemPatch, UpsertSnippetInput, WorkspaceContestDetail,
    WorkspaceContestItem, WorkspaceContestSummary, WorkspaceProblemDetail, WorkspaceProblemSource,
    WorkspaceProblemSummary, WorkspaceReader, WorkspaceSample, WorkspaceSnippetDetail,
    WorkspaceSnippetSummary, WorkspaceTrdDetail, WorkspaceTrdSection, WorkspaceTrdSummary,
    WorkspaceWriter,
};

#[cfg(test)]
mod tests;
