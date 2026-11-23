//! Bundled `testlib.h` and 9 standard Polygon checkers.

use std::path::{Path, PathBuf};

/// Latest upstream `testlib.h` bundled directly in the binary.
pub const TESTLIB_H: &str = include_str!("../assets/testlib.h");

/// Supported built-in standard checker names.
pub const STANDARD_CHECKERS: &[&str] = &[
    "wcmp", "ncmp", "fcmp", "lcmp", "rcmp4", "rcmp6", "rcmp9", "yesno", "nyesno",
];

/// Returns the C++ source of a bundled standard checker by name.
pub fn standard_checker_source(name: &str) -> Option<&'static str> {
    match name {
        "wcmp" => Some(include_str!("../assets/checkers/wcmp.cpp")),
        "ncmp" => Some(include_str!("../assets/checkers/ncmp.cpp")),
        "fcmp" => Some(include_str!("../assets/checkers/fcmp.cpp")),
        "lcmp" => Some(include_str!("../assets/checkers/lcmp.cpp")),
        "rcmp4" => Some(include_str!("../assets/checkers/rcmp4.cpp")),
        "rcmp6" => Some(include_str!("../assets/checkers/rcmp6.cpp")),
        "rcmp9" => Some(include_str!("../assets/checkers/rcmp9.cpp")),
        "yesno" => Some(include_str!("../assets/checkers/yesno.cpp")),
        "nyesno" => Some(include_str!("../assets/checkers/nyesno.cpp")),
        _ => None,
    }
}

/// Returns `true` if `name` is one of the 9 built-in standard checkers.
pub fn is_standard_checker(name: &str) -> bool {
    STANDARD_CHECKERS.contains(&name)
}

/// Resolves which `testlib.h` to use for compiling programs in `problem_dir`.
///
/// Priority:
/// 1. `<problem_dir>/testlib.h` (user-provided per-problem)
/// 2. `<workspace_root>/testlib.h` (user-provided per-workspace)
/// 3. Bundled `TESTLIB_H` (materialized into the cache include directory).
#[derive(Debug, Clone)]
pub enum ResolvedTestlib {
    UserFile {
        include_dir: PathBuf,
        file_path: PathBuf,
        content_hash: String,
    },
    Bundled {
        content_hash: String,
    },
}

impl ResolvedTestlib {
    pub fn content_hash(&self) -> &str {
        match self {
            ResolvedTestlib::UserFile { content_hash, .. } => content_hash,
            ResolvedTestlib::Bundled { content_hash } => content_hash,
        }
    }
}

pub fn resolve_testlib(workspace_root: &Path, problem_dir: &Path) -> ResolvedTestlib {
    for dir in [problem_dir, workspace_root] {
        let candidate = dir.join("testlib.h");
        if candidate.is_file()
            && let Ok(bytes) = std::fs::read(&candidate)
        {
            return ResolvedTestlib::UserFile {
                include_dir: dir.to_path_buf(),
                file_path: candidate,
                content_hash: xcpc_storage::content_hash(&bytes),
            };
        }
    }
    ResolvedTestlib::Bundled {
        content_hash: xcpc_storage::content_hash(TESTLIB_H.as_bytes()),
    }
}
