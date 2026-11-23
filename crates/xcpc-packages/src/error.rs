//! Error types for `xcpc-packages`.

use thiserror::Error;

pub type Result<T, E = PackageError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error(transparent)]
    Storage(#[from] xcpc_storage::Error),

    #[error(transparent)]
    Judge(#[from] xcpc_judge::JudgeError),

    #[error("invalid archive: {0}")]
    InvalidArchive(String),

    #[error("unsafe path in archive (Zip Slip rejected): {0:?}")]
    ZipSlip(String),

    #[error("archive exceeds safety limit ({actual_bytes} bytes > {limit_bytes} bytes)")]
    ArchiveTooLarge { limit_bytes: u64, actual_bytes: u64 },

    #[error("archive has too many entries ({actual} > {limit})")]
    TooManyEntries { limit: usize, actual: usize },

    #[error("{kind} {id:?} already exists in workspace")]
    TargetExists { kind: &'static str, id: String },

    #[error("problem {slug:?} is not strictly verified: {reason}")]
    NotVerified { slug: String, reason: String },

    #[error("problem {slug:?} is missing built testdata: {detail}")]
    MissingTestdata { slug: String, detail: String },

    #[error("failed to compile PDF for problem {slug:?}: {detail}")]
    PdfCompileFailed { slug: String, detail: String },

    #[error("invalid package format: {0}")]
    InvalidFormat(String),

    #[error("unsupported package feature: {0}")]
    Unsupported(String),

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl PackageError {
    /// Stable machine-readable error code for CLI `--json` and MCP.
    pub fn code(&self) -> &'static str {
        match self {
            PackageError::Storage(e) => e.code(),
            PackageError::Judge(e) => e.code(),
            PackageError::InvalidArchive(_) => "invalid_archive",
            PackageError::ZipSlip(_) => "zip_slip",
            PackageError::ArchiveTooLarge { .. } => "archive_too_large",
            PackageError::TooManyEntries { .. } => "too_many_entries",
            PackageError::TargetExists { .. } => "target_exists",
            PackageError::NotVerified { .. } => "not_verified",
            PackageError::MissingTestdata { .. } => "missing_testdata",
            PackageError::PdfCompileFailed { .. } => "pdf_compile_failed",
            PackageError::InvalidFormat(_) => "invalid_format",
            PackageError::Unsupported(_) => "unsupported",
            PackageError::Io { .. } => "io",
        }
    }

    /// Concise human-readable Chinese message for CLI and desktop UI.
    pub fn human_message(&self) -> String {
        match self {
            PackageError::Storage(e) => e.human_message(),
            PackageError::Judge(e) => e.human_message(),
            PackageError::InvalidArchive(msg) => {
                format!("压缩包格式无效或已损坏：{msg}。请确认选择了正确的 Polygon ZIP 文件。")
            }
            PackageError::ZipSlip(path) => {
                format!("压缩包包含不安全的越界路径「{path}」，已拒绝导入。")
            }
            PackageError::ArchiveTooLarge { limit_bytes, .. } => {
                let mib = limit_bytes / (1024 * 1024);
                format!("压缩包解压体积超过安全上限（{mib} MiB），已拒绝导入。")
            }
            PackageError::TooManyEntries { limit, .. } => {
                format!("压缩包文件数量超过安全上限（{limit} 个），已拒绝导入。")
            }
            PackageError::TargetExists { kind, id } => {
                let label = if *kind == "contest" {
                    "比赛"
                } else {
                    "题目"
                };
                format!("{label}「{id}」已存在。请指定其他目标 ID 后重试。")
            }
            PackageError::NotVerified { slug, reason } => {
                format!(
                    "题目「{slug}」尚未通过严格验收（{reason}）。请先运行 `xcpc package build {slug} --strict`，或使用 `--force` 强制导出。"
                )
            }
            PackageError::MissingTestdata { slug, detail } => {
                format!(
                    "题目「{slug}」缺少已构建的测试数据（{detail}）。请先运行 `xcpc package build {slug} --strict` 生成数据后再导出。"
                )
            }
            PackageError::PdfCompileFailed { slug, detail } => {
                format!("题目「{slug}」的题面 PDF 编译失败：{detail}。请先修复题面错误后再导出。")
            }
            PackageError::InvalidFormat(msg) => format!("题目包内容格式有误：{msg}"),
            PackageError::Unsupported(msg) => format!("当前暂不支持该格式特性：{msg}"),
            PackageError::Io { context, source } => format!("{context}：{source}"),
        }
    }
}

pub(crate) fn io_err(context: impl Into<String>, source: std::io::Error) -> PackageError {
    PackageError::Io {
        context: context.into(),
        source,
    }
}
