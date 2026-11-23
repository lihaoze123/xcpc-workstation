//! Native C++ toolchain detection, thin `Language` abstraction, and cached compilation.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::assets::ResolvedTestlib;
use crate::cache::JudgeCache;
use crate::error::{JudgeError, Result, io_err};

/// Compiler family detected on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompilerKind {
    Gpp,
    Clangpp,
    Other,
}

/// One detected compiler binary on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedCompiler {
    pub name: String,
    pub path: String,
    pub version: String,
    pub kind: CompilerKind,
}

/// Structured toolchain detection result for GUI Settings -> Toolchain and CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainStatus {
    /// All compilers discovered on `PATH` (plus any explicit override).
    pub compilers: Vec<DetectedCompiler>,
    /// The compiler that will be used for builds, or `None` if none found.
    pub active: Option<DetectedCompiler>,
    /// Explicit override path passed by caller (e.g. from GUI Settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_path: Option<String>,
    /// Value of `XCPC_CXX` environment variable if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_override: Option<String>,
    /// Platform-specific installation instructions when no compiler is available.
    pub install_guide: String,
}

/// Supported C++ language standards, selected by file suffix (default C++20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CppStd {
    Cpp17,
    Cpp20,
    Cpp23,
}

impl CppStd {
    pub fn flag(self) -> &'static str {
        match self {
            CppStd::Cpp17 => "-std=c++17",
            CppStd::Cpp20 => "-std=c++20",
            CppStd::Cpp23 => "-std=c++23",
        }
    }
}

/// Thin language abstraction layer (Phase 3A focuses on C++).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourceLanguage {
    Cpp { std: CppStd },
}

impl SourceLanguage {
    /// Infers the source language and standard from a file path.
    ///
    /// Suffixes:
    /// - `.17.cpp` / `.17.cc` -> C++17
    /// - `.20.cpp` / `.20.cc` -> C++20
    /// - `.23.cpp` / `.23.cc` -> C++23
    /// - `.cpp` / `.cc` / `.cxx` -> C++20 (default)
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let lower = name.to_ascii_lowercase();
        let is_cpp = lower.ends_with(".cpp") || lower.ends_with(".cc") || lower.ends_with(".cxx");
        if !is_cpp {
            return None;
        }
        let stem = lower
            .strip_suffix(".cpp")
            .or_else(|| lower.strip_suffix(".cc"))
            .or_else(|| lower.strip_suffix(".cxx"))
            .unwrap_or(&lower);
        let std = if stem.ends_with(".17") {
            CppStd::Cpp17
        } else if stem.ends_with(".23") {
            CppStd::Cpp23
        } else {
            CppStd::Cpp20
        };
        Some(SourceLanguage::Cpp { std })
    }

    /// Extracts the logical target name from a source filename (stripping `.17.cpp`, `.cpp`, etc.).
    pub fn logical_stem(path: &Path) -> Option<String> {
        let name = path.file_name()?.to_str()?;
        let lower = name.to_ascii_lowercase();
        let ext_len = if lower.ends_with(".17.cpp")
            || lower.ends_with(".20.cpp")
            || lower.ends_with(".23.cpp")
            || lower.ends_with(".17.cxx")
            || lower.ends_with(".20.cxx")
            || lower.ends_with(".23.cxx")
        {
            7
        } else if lower.ends_with(".17.cc")
            || lower.ends_with(".20.cc")
            || lower.ends_with(".23.cc")
        {
            6
        } else if lower.ends_with(".cpp") || lower.ends_with(".cxx") {
            4
        } else if lower.ends_with(".cc") {
            3
        } else {
            return None;
        };
        let stem = &name[..name.len() - ext_len];
        (!stem.is_empty()).then(|| stem.to_owned())
    }

    /// Returns fixed compiler flags for this language (`-O2`, `-std=...`).
    pub fn compile_flags(self) -> Vec<String> {
        match self {
            SourceLanguage::Cpp { std } => vec!["-O2".to_owned(), std.flag().to_owned()],
        }
    }
}

/// Structured compiler diagnostic parsed from compiler stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileDiagnostic {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub severity: String,
    pub message: String,
}

/// Result of compiling a single source target.
#[derive(Debug, Clone)]
pub struct CompileResult {
    pub binary_path: PathBuf,
    pub binary_hash: String,
    pub cache_hit: bool,
    pub stderr: String,
}

/// Structured compile failure for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileFailure {
    pub target: String,
    pub source_path: String,
    pub stderr: String,
    pub diagnostics: Vec<CompileDiagnostic>,
}

/// Detects available C++ compilers on the host.
///
/// Selection precedence for `active`:
/// 1. Explicit `cxx_override` argument (e.g. GUI Settings -> Toolchain)
/// 2. `XCPC_CXX` environment variable
/// 3. First detected compiler on `PATH` (`g++`, then `clang++`)
pub fn detect_toolchain(cxx_override: Option<&Path>) -> ToolchainStatus {
    let env_override = std::env::var("XCPC_CXX")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let override_path = cxx_override
        .map(|p| p.to_string_lossy().trim().to_owned())
        .filter(|s| !s.is_empty());

    let mut compilers = Vec::new();
    let mut seen = HashSet::new();

    let mut try_add = |candidate: &Path| {
        if let Some(info) = probe_compiler(candidate) {
            let key = fs::canonicalize(&info.path).unwrap_or_else(|_| PathBuf::from(&info.path));
            if seen.insert(key) {
                compilers.push(info);
            }
        }
    };

    let explicit_active = if let Some(ref ov) = override_path {
        let p = PathBuf::from(ov);
        let probed = resolve_in_path(&p).and_then(|found| probe_compiler(&found));
        if let Some(ref c) = probed {
            try_add(Path::new(&c.path));
        }
        probed
    } else if let Some(ref env_cxx) = env_override {
        let p = PathBuf::from(env_cxx);
        let probed = resolve_in_path(&p).and_then(|found| probe_compiler(&found));
        if let Some(ref c) = probed {
            try_add(Path::new(&c.path));
        }
        probed
    } else {
        None
    };

    // Scan PATH for g++ and clang++.
    let names: &[&str] = if cfg!(windows) {
        &["g++.exe", "clang++.exe"]
    } else {
        &["g++", "clang++"]
    };
    if let Some(path_var) = std::env::var_os("PATH") {
        for name in names {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    try_add(&candidate);
                }
            }
        }
    }

    let active = if override_path.is_some() || env_override.is_some() {
        explicit_active
    } else {
        compilers.first().cloned()
    };

    ToolchainStatus {
        compilers,
        active,
        override_path,
        env_override,
        install_guide: platform_install_guide().to_owned(),
    }
}

/// Resolves the active compiler or returns an error if missing / invalid override.
pub fn resolve_active_compiler(cxx_override: Option<&Path>) -> Result<DetectedCompiler> {
    let status = detect_toolchain(cxx_override);
    if let Some(active) = status.active {
        return Ok(active);
    }
    if let Some(ov) = status.override_path {
        return Err(JudgeError::CompilerNotFound(ov));
    }
    if let Some(env_ov) = status.env_override {
        return Err(JudgeError::CompilerNotFound(env_ov));
    }
    Err(JudgeError::NoCompiler)
}

fn resolve_in_path(p: &Path) -> Option<PathBuf> {
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(p);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{}.exe", p.to_string_lossy()));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

fn probe_compiler(path: &Path) -> Option<DetectedCompiler> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim().to_owned();
    let lower = first_line.to_ascii_lowercase();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("c++")
        .to_owned();
    let kind = if lower.contains("clang") || file_name.to_ascii_lowercase().contains("clang") {
        CompilerKind::Clangpp
    } else if lower.contains("g++")
        || lower.contains("gcc")
        || file_name.to_ascii_lowercase().contains("g++")
    {
        CompilerKind::Gpp
    } else {
        CompilerKind::Other
    };
    Some(DetectedCompiler {
        name: file_name,
        path: path.to_string_lossy().into_owned(),
        version: first_line,
        kind,
    })
}

fn platform_install_guide() -> &'static str {
    if cfg!(target_os = "macos") {
        "运行 `xcode-select --install` 安装 Apple Clang，或通过 Homebrew 安装 GCC：`brew install gcc`。"
    } else if cfg!(windows) {
        "推荐安装 MSYS2（https://www.msys2.org/）后执行 `pacman -S mingw-w64-ucrt-x86_64-gcc`，并将 `C:\\msys64\\ucrt64\\bin` 加入 PATH，或在上方手动指定 `g++.exe` 路径。"
    } else {
        "在 Linux 上安装 GCC 或 Clang：Ubuntu/Debian 运行 `sudo apt install g++`，Arch 运行 `sudo pacman -S gcc`，Fedora 运行 `sudo dnf install gcc-c++`。"
    }
}

/// Compiles a C++ source file (or in-memory source bytes for a built-in checker)
/// using the Level 1 content-hash cache.
#[allow(clippy::too_many_arguments)]
pub fn compile_cached(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    language: SourceLanguage,
    target_label: &str,
    source_path_for_display: &str,
    source_bytes: &[u8],
    source_disk_path: Option<&Path>,
    testlib: &ResolvedTestlib,
) -> std::result::Result<CompileResult, CompileFailure> {
    let flags = language.compile_flags();
    let mut hasher = Sha256::new();
    hasher.update(b"xcpc-judge-compile-v1\0");
    hasher.update(compiler.path.as_bytes());
    hasher.update(b"\0");
    hasher.update(compiler.version.as_bytes());
    hasher.update(b"\0");
    for f in &flags {
        hasher.update(f.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(testlib.content_hash().as_bytes());
    hasher.update(b"\0");
    hasher.update(source_bytes);
    let key = format!("{:x}", hasher.finalize());

    let bin_path = cache.binary_path(&key);
    if bin_path.is_file() {
        return Ok(CompileResult {
            binary_path: bin_path,
            binary_hash: key,
            cache_hit: true,
            stderr: String::new(),
        });
    }

    let inc_dir = match testlib {
        ResolvedTestlib::UserFile { include_dir, .. } => include_dir.clone(),
        ResolvedTestlib::Bundled { .. } => {
            cache
                .ensure_bundled_testlib_dir()
                .map_err(|e| CompileFailure {
                    target: target_label.to_owned(),
                    source_path: source_path_for_display.to_owned(),
                    stderr: e.to_string(),
                    diagnostics: vec![],
                })?
        }
    };

    // If source is in-memory (e.g. bundled standard checker), materialize it into a temp dir.
    let tmp_dir = tempfile::tempdir().map_err(|e| CompileFailure {
        target: target_label.to_owned(),
        source_path: source_path_for_display.to_owned(),
        stderr: format!("create temp dir: {e}"),
        diagnostics: vec![],
    })?;

    let src_to_compile = if let Some(p) = source_disk_path {
        p.to_path_buf()
    } else {
        let tmp_src = tmp_dir.path().join(format!("{target_label}.cpp"));
        fs::write(&tmp_src, source_bytes).map_err(|e| CompileFailure {
            target: target_label.to_owned(),
            source_path: source_path_for_display.to_owned(),
            stderr: format!("write temp source: {e}"),
            diagnostics: vec![],
        })?;
        tmp_src
    };

    let tmp_out = bin_path.with_extension(format!("{}.tmp", std::process::id()));
    let mut cmd = Command::new(&compiler.path);
    for f in &flags {
        cmd.arg(f);
    }
    cmd.arg("-I").arg(&inc_dir);
    if let Some(parent) = src_to_compile.parent() {
        cmd.arg("-I").arg(parent);
    }
    cmd.arg(&src_to_compile).arg("-o").arg(&tmp_out);

    let output = cmd.output().map_err(|e| CompileFailure {
        target: target_label.to_owned(),
        source_path: source_path_for_display.to_owned(),
        stderr: io_err(format!("spawn compiler {}", compiler.path), e).to_string(),
        diagnostics: vec![],
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let _ = fs::remove_file(&tmp_out);
        let diagnostics = parse_compiler_diagnostics(&stderr);
        return Err(CompileFailure {
            target: target_label.to_owned(),
            source_path: source_path_for_display.to_owned(),
            stderr,
            diagnostics,
        });
    }

    if let Err(e) = fs::rename(&tmp_out, &bin_path) {
        // Another thread may have compiled the exact same hash concurrently.
        if !bin_path.is_file() {
            let _ = fs::remove_file(&tmp_out);
            return Err(CompileFailure {
                target: target_label.to_owned(),
                source_path: source_path_for_display.to_owned(),
                stderr: format!("cache binary rename failed: {e}"),
                diagnostics: vec![],
            });
        }
        let _ = fs::remove_file(&tmp_out);
    }

    Ok(CompileResult {
        binary_path: bin_path,
        binary_hash: key,
        cache_hit: false,
        stderr,
    })
}

/// Parses GCC / Clang diagnostics of the form `<file>:<line>:<col>: <severity>: <message>`.
pub fn parse_compiler_diagnostics(stderr: &str) -> Vec<CompileDiagnostic> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Match `<file>:<line>:<col>: <severity>: <msg>` or `<file>:<line>: <severity>: <msg>`
        for sev in ["fatal error", "error", "warning", "note"] {
            let marker = format!(": {sev}: ");
            if let Some((loc, msg)) = trimmed.split_once(&marker) {
                let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
                let (file, line_no, col_no) = match parts.as_slice() {
                    [col_s, line_s, file_s]
                        if col_s.chars().all(|c| c.is_ascii_digit())
                            && line_s.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        (
                            Some((*file_s).to_owned()),
                            line_s.parse::<u32>().ok(),
                            col_s.parse::<u32>().ok(),
                        )
                    }
                    [line_s, file_s] if line_s.chars().all(|c| c.is_ascii_digit()) => {
                        (Some((*file_s).to_owned()), line_s.parse::<u32>().ok(), None)
                    }
                    _ => (Some(loc.to_owned()), None, None),
                };
                out.push(CompileDiagnostic {
                    file,
                    line: line_no,
                    column: col_no,
                    severity: sev.to_owned(),
                    message: msg.trim().to_owned(),
                });
                break;
            }
        }
    }
    out
}
