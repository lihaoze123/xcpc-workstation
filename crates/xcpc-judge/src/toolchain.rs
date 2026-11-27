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
    /// Copyable install commands for this system only (the distro's package manager,
    /// plus Nix when it is installed).
    #[serde(default)]
    pub install_commands: Vec<InstallCommand>,
}

/// One copyable way to install a C++ compiler on this system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallCommand {
    /// Where it applies: 「Arch Linux」「Nix」「macOS」.
    pub label: String,
    pub command: String,
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
    ///
    /// On Windows, also passes `-DFOR_LINUX` so `testlib.h` validators/checkers accept
    /// canonical Unix LF (`\n`) line endings used across XCPC Workstation test data.
    pub fn compile_flags(self) -> Vec<String> {
        match self {
            SourceLanguage::Cpp { std } => {
                #[allow(unused_mut)]
                let mut flags = vec!["-O2".to_owned(), std.flag().to_owned()];
                #[cfg(windows)]
                flags.push("-DFOR_LINUX".to_owned());
                flags
            }
        }
    }

    /// Returns ASan + UBSan instrumentation compiler flags for this language.
    pub fn sanitizer_compile_flags(self) -> Vec<String> {
        match self {
            SourceLanguage::Cpp { std } => {
                #[allow(unused_mut)]
                let mut flags = vec![
                    "-O1".to_owned(),
                    std.flag().to_owned(),
                    "-fsanitize=address,undefined".to_owned(),
                    "-fno-omit-frame-pointer".to_owned(),
                    "-g".to_owned(),
                    "-fno-sanitize-recover=undefined".to_owned(),
                ];
                #[cfg(windows)]
                flags.push("-DFOR_LINUX".to_owned());
                flags
            }
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
    let mut seen_paths = HashSet::new();
    let mut seen_name_versions = HashSet::new();

    let mut try_add = |candidate: &Path| {
        if let Some(info) = probe_compiler(candidate) {
            let path_key =
                fs::canonicalize(&info.path).unwrap_or_else(|_| PathBuf::from(&info.path));
            let nv_key = (info.name.clone(), info.version.clone());
            if seen_paths.insert(path_key) && seen_name_versions.insert(nv_key) {
                compilers.push(info);
            }
        }
    };

    // Scan PATH first so the first occurrence in PATH (e.g. NixOS gcc-wrapper before unwrapped
    // gcc) is preserved when deduplicating by (name, version).
    let mut search_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // macOS `.app` bundles launched from Finder/Dock do not inherit Homebrew's PATH.
    if cfg!(target_os = "macos") {
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let dir = PathBuf::from(extra);
            if !search_dirs.contains(&dir) && dir.is_dir() {
                search_dirs.push(dir);
            }
        }
    }

    // On macOS, Homebrew installs GNU GCC as versioned `g++-<N>` (e.g. `g++-15`, `g++-14`)
    // while `/usr/bin/g++` is Apple Clang (which lacks `<bits/stdc++.h>`). Scan versioned
    // `g++-<N>` (highest version first) before falling back to `g++` / `clang++`.
    if cfg!(target_os = "macos") {
        for dir in &search_dirs {
            for candidate in versioned_gpp_candidates(dir) {
                try_add(&candidate);
            }
        }
    }

    let names: &[&str] = if cfg!(windows) {
        &["g++.exe", "clang++.exe"]
    } else {
        &["g++", "clang++"]
    };
    for name in names {
        for dir in &search_dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                try_add(&candidate);
            }
        }
    }

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

    // On macOS, prefer real GNU GCC (Homebrew) over Apple Clang when both are present,
    // since competitive programming solutions commonly `#include <bits/stdc++.h>`.
    if cfg!(target_os = "macos") {
        compilers.sort_by_key(|c| match c.kind {
            CompilerKind::Gpp => 0,
            CompilerKind::Clangpp => 1,
            CompilerKind::Other => 2,
        });
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
        install_commands: platform_install_commands(),
    }
}

fn platform_install_commands() -> Vec<InstallCommand> {
    if cfg!(target_os = "macos") {
        return vec![
            InstallCommand {
                label: "Homebrew (推荐)".to_owned(),
                command: "brew install gcc".to_owned(),
            },
            InstallCommand {
                label: "macOS Command Line Tools".to_owned(),
                command: "xcode-select --install".to_owned(),
            },
        ];
    }
    if cfg!(windows) {
        return vec![InstallCommand {
            label: "MSYS2".to_owned(),
            command: "winget install MSYS2.MSYS2\npacman -S mingw-w64-ucrt-x86_64-gcc".to_owned(),
        }];
    }
    let os_release = fs::read_to_string("/etc/os-release").ok();
    let has_nix = Path::new("/nix/store").is_dir();
    linux_install_commands(os_release.as_deref(), has_nix)
}

/// The install commands for a Linux system described by `/etc/os-release` (`ID`,
/// `ID_LIKE`, `NAME`); every common one when the distro is unknown.
pub fn linux_install_commands(os_release: Option<&str>, has_nix: bool) -> Vec<InstallCommand> {
    let field = |key: &str| -> String {
        os_release
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(|v| v.trim().trim_matches('"').to_ascii_lowercase())
            .unwrap_or_default()
    };
    let id = field("ID");
    let like = field("ID_LIKE");
    let is = |names: &[&str]| {
        names
            .iter()
            .any(|n| id == *n || like.split_whitespace().any(|l| l == *n))
    };
    let cmd = |label: &str, command: &str| InstallCommand {
        label: label.to_owned(),
        command: command.to_owned(),
    };
    let nix = cmd("Nix", "nix profile install nixpkgs#gcc");
    let distro = if id == "nixos" {
        return vec![nix];
    } else if is(&["arch"]) {
        Some(cmd("Arch Linux", "sudo pacman -S gcc"))
    } else if is(&["debian", "ubuntu"]) {
        Some(cmd("Debian / Ubuntu", "sudo apt install g++"))
    } else if is(&["fedora", "rhel", "centos"]) {
        Some(cmd("Fedora", "sudo dnf install gcc-c++"))
    } else if is(&["opensuse", "suse", "opensuse-tumbleweed", "opensuse-leap"]) {
        Some(cmd("openSUSE", "sudo zypper install gcc-c++"))
    } else if is(&["alpine"]) {
        Some(cmd("Alpine", "sudo apk add g++"))
    } else {
        None
    };
    let mut out = match distro {
        Some(d) => vec![d],
        None => vec![
            cmd("Debian / Ubuntu", "sudo apt install g++"),
            cmd("Arch Linux", "sudo pacman -S gcc"),
            cmd("Fedora", "sudo dnf install gcc-c++"),
        ],
    };
    if has_nix {
        out.push(nix);
    }
    out
}

/// Validates that `path` points to an existing, runnable C++ compiler and returns its
/// detected metadata. Rejects non-existent, non-executable, or non-compiler paths.
pub fn validate_compiler_path(path: &Path) -> Result<DetectedCompiler> {
    let display = path.to_string_lossy().trim().to_owned();
    if display.is_empty() {
        return Err(JudgeError::CompilerNotFound(display));
    }
    let resolved = resolve_in_path(Path::new(&display))
        .ok_or_else(|| JudgeError::CompilerNotFound(display.clone()))?;
    let info =
        probe_compiler(&resolved).ok_or_else(|| JudgeError::CompilerNotFound(display.clone()))?;
    let v_lower = info.version.to_ascii_lowercase();
    let looks_like_cxx = matches!(info.kind, CompilerKind::Gpp | CompilerKind::Clangpp)
        || v_lower.contains("c++")
        || v_lower.contains("mingw")
        || v_lower.contains("icpx")
        || v_lower.contains("zig");
    if !looks_like_cxx {
        return Err(JudgeError::CompilerNotFound(display));
    }
    Ok(info)
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

/// Discovers versioned Homebrew/GNU `g++-<major>` binaries inside `dir`, ordered from
/// newest major version to oldest (e.g. `g++-15`, `g++-14`, `g++-13`).
pub(crate) fn versioned_gpp_candidates(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(suffix) = name.strip_prefix("g++-")
            && !suffix.is_empty()
            && suffix.bytes().all(|b| b.is_ascii_digit())
            && let Ok(major) = suffix.parse::<u32>()
        {
            found.push((major, path));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, p)| p).collect()
}

fn resolve_in_path(p: &Path) -> Option<PathBuf> {
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let dir = PathBuf::from(extra);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    for dir in dirs {
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
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    crate::runner::configure_no_window(&mut cmd);
    let output = cmd.output().ok()?;
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
        "推荐通过 Homebrew 安装 GNU GCC（`brew install gcc`，完整支持 `<bits/stdc++.h>`），或运行 `xcode-select --install` 安装 Apple Clang。"
    } else if cfg!(windows) {
        "推荐安装 MSYS2（https://www.msys2.org/）后执行 `pacman -S mingw-w64-ucrt-x86_64-gcc`，并将 `C:\\msys64\\ucrt64\\bin` 加入 PATH，或在上方手动指定 `g++.exe` 路径。"
    } else {
        "在 Linux 上安装 GCC 或 Clang：Ubuntu/Debian 运行 `sudo apt install g++`，Arch 运行 `sudo pacman -S gcc`，Fedora 运行 `sudo dnf install gcc-c++`。"
    }
}

/// Probes whether `compiler` can compile and execute a minimal binary with
/// `-fsanitize=address,undefined -fno-omit-frame-pointer -g`.
///
/// Returns `Ok(())` if supported, or `Err(reason)` if the toolchain lacks ASan/UBSan runtime
/// support on this host.
pub fn probe_sanitizer_support(compiler: &DetectedCompiler) -> std::result::Result<(), String> {
    let tmp_dir = tempfile::tempdir().map_err(|e| format!("create temp dir: {e}"))?;
    let src = xcpc_storage::strip_verbatim_prefix(&tmp_dir.path().join("probe_san.cpp"));
    let bin = xcpc_storage::strip_verbatim_prefix(&tmp_dir.path().join(if cfg!(windows) {
        "probe_san.exe"
    } else {
        "probe_san"
    }));
    fs::write(&src, "int main() { volatile int x = 0; return x; }\n")
        .map_err(|e| format!("write sanitizer probe source: {e}"))?;

    let flags = SourceLanguage::Cpp { std: CppStd::Cpp20 }.sanitizer_compile_flags();
    let mut cmd = Command::new(&compiler.path);
    crate::runner::configure_no_window(&mut cmd);
    for f in &flags {
        cmd.arg(f);
    }
    cmd.arg(&src).arg("-o").arg(&bin);

    let output = cmd
        .output()
        .map_err(|e| format!("spawn compiler {} for sanitizer probe: {e}", compiler.path))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let first = stderr.lines().next().unwrap_or("sanitizer compile failed");
        return Err(format!("编译器不支持 ASan/UBSan：{first}"));
    }

    let mut run_cmd = Command::new(&bin);
    crate::runner::configure_no_window(&mut run_cmd);
    let run = run_cmd
        .env(
            "ASAN_OPTIONS",
            "detect_leaks=0:halt_on_error=1:abort_on_error=0",
        )
        .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1")
        .output()
        .map_err(|e| format!("execute sanitizer probe binary: {e}"))?;
    if !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr).trim().to_owned();
        let first = stderr.lines().next().unwrap_or("sanitizer runtime failed");
        return Err(format!("ASan/UBSan 运行库不可用：{first}"));
    }
    Ok(())
}

/// Compiles a C++ source file with ASan + UBSan into the isolated sanitizer cache namespace
/// (`<cache>/sanitize/bin/`), never touching normal `-O2` binaries.
#[allow(clippy::too_many_arguments)]
pub fn compile_cached_sanitized(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    language: SourceLanguage,
    target_label: &str,
    source_path_for_display: &str,
    source_bytes: &[u8],
    source_disk_path: Option<&Path>,
    testlib: &ResolvedTestlib,
) -> std::result::Result<CompileResult, CompileFailure> {
    let flags = language.sanitizer_compile_flags();
    compile_cached_inner(
        cache,
        compiler,
        &flags,
        b"xcpc-judge-sanitize-v1\0",
        true,
        target_label,
        source_path_for_display,
        source_bytes,
        source_disk_path,
        testlib,
        &[],
        &[],
    )
}

/// Compiles a C++ source file directly into `output_binary_path` without touching the shared
/// judge cache (used for zero-pollution external tester submissions).
#[allow(clippy::too_many_arguments)]
pub fn compile_to_path(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    language: SourceLanguage,
    target_label: &str,
    source_disk_path: &Path,
    output_binary_path: &Path,
    testlib: &ResolvedTestlib,
) -> std::result::Result<String, CompileFailure> {
    let flags = language.compile_flags();
    let display = source_disk_path.to_string_lossy().into_owned();
    let inc_dir = match testlib {
        ResolvedTestlib::UserFile { include_dir, .. } => include_dir.clone(),
        ResolvedTestlib::Bundled { .. } => {
            cache
                .ensure_bundled_testlib_dir()
                .map_err(|e| CompileFailure {
                    target: target_label.to_owned(),
                    source_path: display.clone(),
                    stderr: e.to_string(),
                    diagnostics: vec![],
                })?
        }
    };

    let inc_dir = xcpc_storage::strip_verbatim_prefix(&inc_dir);
    let src_arg = xcpc_storage::strip_verbatim_prefix(source_disk_path);
    let out_arg = xcpc_storage::strip_verbatim_prefix(output_binary_path);

    let mut cmd = Command::new(&compiler.path);
    crate::runner::configure_no_window(&mut cmd);
    for f in &flags {
        cmd.arg(f);
    }
    cmd.arg("-I").arg(&inc_dir);
    if let Some(parent) = src_arg.parent() {
        cmd.arg("-I").arg(parent);
    }
    cmd.arg(&src_arg).arg("-o").arg(&out_arg);

    let output = cmd.output().map_err(|e| CompileFailure {
        target: target_label.to_owned(),
        source_path: display.clone(),
        stderr: io_err(format!("spawn compiler {}", compiler.path), e).to_string(),
        diagnostics: vec![],
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let diagnostics = parse_compiler_diagnostics(&stderr);
        return Err(CompileFailure {
            target: target_label.to_owned(),
            source_path: display,
            stderr,
            diagnostics,
        });
    }
    Ok(stderr)
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
    compile_cached_with_includes(
        cache,
        compiler,
        language,
        target_label,
        source_path_for_display,
        source_bytes,
        source_disk_path,
        testlib,
        &[],
        &[],
    )
}

/// Compiles a C++ source file using the Level 1 content-hash cache with additional
/// `-I` include directories and an optional `extra_hash_salt` (e.g. transitive header hash).
#[allow(clippy::too_many_arguments)]
pub fn compile_cached_with_includes(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    language: SourceLanguage,
    target_label: &str,
    source_path_for_display: &str,
    source_bytes: &[u8],
    source_disk_path: Option<&Path>,
    testlib: &ResolvedTestlib,
    extra_include_dirs: &[PathBuf],
    extra_hash_salt: &[u8],
) -> std::result::Result<CompileResult, CompileFailure> {
    let flags = language.compile_flags();
    compile_cached_inner(
        cache,
        compiler,
        &flags,
        b"xcpc-judge-compile-v1\0",
        false,
        target_label,
        source_path_for_display,
        source_bytes,
        source_disk_path,
        testlib,
        extra_include_dirs,
        extra_hash_salt,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_cached_inner(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    flags: &[String],
    namespace: &[u8],
    use_sanitizer_dir: bool,
    target_label: &str,
    source_path_for_display: &str,
    source_bytes: &[u8],
    source_disk_path: Option<&Path>,
    testlib: &ResolvedTestlib,
    extra_include_dirs: &[PathBuf],
    extra_hash_salt: &[u8],
) -> std::result::Result<CompileResult, CompileFailure> {
    let mut hasher = Sha256::new();
    hasher.update(namespace);
    hasher.update(compiler.path.as_bytes());
    hasher.update(b"\0");
    hasher.update(compiler.version.as_bytes());
    hasher.update(b"\0");
    for f in flags {
        hasher.update(f.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(testlib.content_hash().as_bytes());
    hasher.update(b"\0");
    if !extra_hash_salt.is_empty() {
        hasher.update(extra_hash_salt);
        hasher.update(b"\0");
    }
    hasher.update(source_bytes);
    let key = format!("{:x}", hasher.finalize());

    let bin_path = if use_sanitizer_dir {
        cache.sanitizer_binary_path(&key)
    } else {
        cache.binary_path(&key)
    };
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

    let src_to_compile = xcpc_storage::strip_verbatim_prefix(&src_to_compile);
    let inc_dir = xcpc_storage::strip_verbatim_prefix(&inc_dir);
    let tmp_ext = if cfg!(windows) {
        format!("{}.tmp.exe", std::process::id())
    } else {
        format!("{}.tmp", std::process::id())
    };
    let tmp_out = xcpc_storage::strip_verbatim_prefix(&bin_path.with_extension(tmp_ext));
    let mut cmd = Command::new(&compiler.path);
    crate::runner::configure_no_window(&mut cmd);
    for f in flags {
        cmd.arg(f);
    }
    cmd.arg("-I").arg(&inc_dir);
    for dir in extra_include_dirs {
        cmd.arg("-I").arg(xcpc_storage::strip_verbatim_prefix(dir));
    }
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

#[cfg(test)]
mod install_command_tests {
    use super::linux_install_commands;

    fn labels(os: &str, nix: bool) -> Vec<String> {
        linux_install_commands(Some(os), nix)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn picks_the_distro_and_adds_nix_only_when_present() {
        assert_eq!(labels("ID=cachyos\nID_LIKE=arch\n", false), ["Arch Linux"]);
        assert_eq!(
            labels("ID=ubuntu\nID_LIKE=debian\n", true),
            ["Debian / Ubuntu", "Nix"]
        );
        assert_eq!(labels("ID=\"fedora\"\n", false), ["Fedora"]);
        assert_eq!(labels("ID=nixos\n", true), ["Nix"]);
        assert_eq!(linux_install_commands(None, false).len(), 3);
    }

    #[test]
    fn versioned_gpp_candidates_orders_newest_first_and_ignores_non_numeric() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "g++-12",
            "g++-15",
            "g++-14",
            "g++-foo",
            "g++-",
            "g++",
            "clang++-18",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let found: Vec<String> = super::versioned_gpp_candidates(dir.path())
            .into_iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect();
        assert_eq!(found, ["g++-15", "g++-14", "g++-12"]);
    }
}
