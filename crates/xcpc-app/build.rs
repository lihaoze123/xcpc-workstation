//! Captures build metadata (`XCPC_GIT_COMMIT` and `XCPC_BUILD_DATE`) at compile time
//! for `AppController::about_info()` and cold-start snapshot fingerprinting.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=XCPC_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=XCPC_BUILD_DATE");

    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    let explicit_commit = std::env::var("XCPC_GIT_COMMIT")
        .ok()
        .filter(|s| !s.trim().is_empty());

    // In dev/test profiles (especially under colocated jj), watching `.git/HEAD`
    // invalidates downstream crates on every commit/bookmark edit.
    // Only watch git refs for release builds when `XCPC_GIT_COMMIT` is not provided.
    let commit = if let Some(commit) = explicit_commit {
        commit
    } else if is_release {
        emit_git_rerun_hints();
        git_short_commit().unwrap_or_else(|| "unknown".to_owned())
    } else {
        "dev".to_owned()
    };

    let build_date = std::env::var("XCPC_BUILD_DATE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if is_release {
                utc_today()
            } else {
                "dev".to_owned()
            }
        });

    println!("cargo:rustc-env=XCPC_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=XCPC_BUILD_DATE={build_date}");
}

fn emit_git_rerun_hints() {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(raw) = String::from_utf8(output.stdout) else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let git_dir = PathBuf::from(trimmed);
    let head_path = git_dir.join("HEAD");
    if head_path.exists() {
        println!("cargo:rerun-if-changed={}", head_path.display());
    }
    if let Ok(head_text) = std::fs::read_to_string(&head_path)
        && let Some(rest) = head_text.trim().strip_prefix("ref:")
    {
        let ref_rel = rest.trim();
        if !ref_rel.is_empty() {
            let ref_path = git_dir.join(ref_rel);
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
            let packed_refs = git_dir.join("packed-refs");
            if packed_refs.exists() {
                println!("cargo:rerun-if-changed={}", packed_refs.display());
            }
        }
    }
}

fn git_short_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn utc_today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's civil_from_days algorithm (UTC Gregorian calendar).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
