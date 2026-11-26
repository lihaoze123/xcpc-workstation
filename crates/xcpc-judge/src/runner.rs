//! Lightweight native process runner with resource limits and usage measurement.
//!
//! - Linux/Unix: `rlimit` (`RLIMIT_AS`, `RLIMIT_STACK`, `RLIMIT_FSIZE`, `RLIMIT_CPU`) +
//!   `wait4` `rusage` (CPU time ms + peak RSS KiB) + wall-clock watchdog kill.
//! - Windows: Job Object memory limit + `GetProcessTimes` + wall-clock watchdog kill.
//!
//! No OS security sandbox is applied (local trusted author code only).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{JudgeError, Result, io_err};
use crate::report::{InteractionTranscript, TranscriptDirection, TranscriptEntry};

/// Standard XCPC judging verdicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    AC,
    WA,
    PE,
    TLE,
    MLE,
    ILE,
    RE,
    CE,
    JE,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::AC => "AC",
            Verdict::WA => "WA",
            Verdict::PE => "PE",
            Verdict::TLE => "TLE",
            Verdict::MLE => "MLE",
            Verdict::ILE => "ILE",
            Verdict::RE => "RE",
            Verdict::CE => "CE",
            Verdict::JE => "JE",
        }
    }

    pub fn is_ok(self) -> bool {
        self == Verdict::AC
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resource limits for running a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLimits {
    /// CPU time limit in milliseconds.
    pub time_limit_ms: u64,
    /// Memory limit in KiB (RSS). `0` disables RSS MLE enforcement.
    pub memory_limit_kib: u64,
    /// Output file size cap in bytes (default 256 MiB).
    pub fsize_limit_bytes: u64,
    /// Wall-clock watchdog kill timeout in milliseconds.
    pub wall_limit_ms: u64,
    /// When `true`, skips `RLIMIT_AS` (required for ASan shadow memory reservation) and
    /// applies sanitizer runtime env options (`ASAN_OPTIONS`, `UBSAN_OPTIONS`).
    #[serde(default)]
    pub disable_as_limit: bool,
}

impl RunLimits {
    /// Builds limits from `problem.toml`'s `time` and `memory` strings (defaults: 1s, 256MB).
    pub fn from_problem_limits(time: Option<&str>, memory: Option<&str>) -> Result<Self> {
        let time_limit_ms = match time.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => parse_time_limit_ms(t)?,
            None => 1000,
        };
        let memory_limit_kib = match memory.map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) => parse_memory_limit_kib(m)?,
            None => 256 * 1024,
        };
        // Wall-clock watchdog: at least 2x time_limit + 2000ms so sleeping/deadlocked
        // programs are killed cleanly without cutting off legitimate CPU-bound runs early.
        let wall_limit_ms = time_limit_ms
            .saturating_mul(2)
            .saturating_add(2000)
            .max(3000);
        Ok(Self {
            time_limit_ms,
            memory_limit_kib,
            fsize_limit_bytes: 256 * 1024 * 1024,
            wall_limit_ms,
            disable_as_limit: false,
        })
    }

    /// Generous limits for running tools (generators, validators, checkers).
    pub fn tool_defaults() -> Self {
        Self {
            time_limit_ms: 30_000,
            memory_limit_kib: 1024 * 1024, // 1 GiB
            fsize_limit_bytes: 512 * 1024 * 1024,
            wall_limit_ms: 60_000,
            disable_as_limit: false,
        }
    }

    /// Relaxed limits for ASan/UBSan execution: `max(5 * TL, 5000ms)`, no `RLIMIT_AS`,
    /// and disabled RSS MLE cap to accommodate sanitizer shadow/quarantine overhead.
    pub fn for_sanitizer(base: Self) -> Self {
        let time_limit_ms = base.time_limit_ms.saturating_mul(5).max(5_000);
        let wall_limit_ms = time_limit_ms
            .saturating_mul(2)
            .saturating_add(5_000)
            .max(15_000);
        Self {
            time_limit_ms,
            memory_limit_kib: 0,
            fsize_limit_bytes: base.fsize_limit_bytes.max(512 * 1024 * 1024),
            wall_limit_ms,
            disable_as_limit: true,
        }
    }

    /// Generous limits for running sanitizer-instrumented tools (generators, validators, checkers).
    pub fn sanitizer_tool_defaults() -> Self {
        Self {
            time_limit_ms: 60_000,
            memory_limit_kib: 0,
            fsize_limit_bytes: 512 * 1024 * 1024,
            wall_limit_ms: 120_000,
            disable_as_limit: true,
        }
    }
}

/// Parses a human-readable time limit such as `"1s"`, `"2.5s"`, `"500ms"` into milliseconds.
pub fn parse_time_limit_ms(raw: &str) -> Result<u64> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    if let Some(num) = s.strip_suffix("ms").or_else(|| s.strip_suffix("毫秒")) {
        let v: f64 = num.parse().map_err(|_| JudgeError::InvalidLimit {
            kind: "time",
            value: raw.to_owned(),
        })?;
        if v <= 0.0 || !v.is_finite() {
            return Err(JudgeError::InvalidLimit {
                kind: "time",
                value: raw.to_owned(),
            });
        }
        return Ok(v.ceil() as u64);
    }
    let num = s
        .strip_suffix("seconds")
        .or_else(|| s.strip_suffix("second"))
        .or_else(|| s.strip_suffix("sec"))
        .or_else(|| s.strip_suffix('s'))
        .or_else(|| s.strip_suffix('秒'))
        .unwrap_or(&s);
    let v: f64 = num.parse().map_err(|_| JudgeError::InvalidLimit {
        kind: "time",
        value: raw.to_owned(),
    })?;
    if v <= 0.0 || !v.is_finite() {
        return Err(JudgeError::InvalidLimit {
            kind: "time",
            value: raw.to_owned(),
        });
    }
    Ok((v * 1000.0).ceil() as u64)
}

/// Parses a human-readable memory limit such as `"256MB"`, `"512MiB"`, `"1GB"`, `"65536KB"` into KiB.
pub fn parse_memory_limit_kib(raw: &str) -> Result<u64> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    let (num, mult_kib): (&str, f64) = if let Some(n) = s.strip_suffix("gib") {
        (n, 1024.0 * 1024.0)
    } else if let Some(n) = s.strip_suffix("gb").or_else(|| s.strip_suffix('g')) {
        (n, 1024.0 * 1024.0)
    } else if let Some(n) = s.strip_suffix("mib") {
        (n, 1024.0)
    } else if let Some(n) = s.strip_suffix("mb").or_else(|| s.strip_suffix('m')) {
        (n, 1024.0)
    } else if let Some(n) = s.strip_suffix("kib") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix("kb").or_else(|| s.strip_suffix('k')) {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('b') {
        (n, 1.0 / 1024.0)
    } else {
        // Bare number treated as MiB (standard OJ convention).
        (s.as_str(), 1024.0)
    };
    let v: f64 = num.parse().map_err(|_| JudgeError::InvalidLimit {
        kind: "memory",
        value: raw.to_owned(),
    })?;
    if v <= 0.0 || !v.is_finite() {
        return Err(JudgeError::InvalidLimit {
            kind: "memory",
            value: raw.to_owned(),
        });
    }
    Ok((v * mult_kib).ceil() as u64)
}

/// Outcome of executing a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOutcome {
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub peak_memory_kib: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stderr: String,
}

/// Runs an executable with optional stdin/stdout files and resource limits.
pub fn run_process(
    binary: &Path,
    args: &[String],
    stdin_path: Option<&Path>,
    stdout_path: Option<&Path>,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<RunOutcome> {
    if let Some(flag) = cancel
        && flag.load(Ordering::Relaxed)
    {
        return Err(JudgeError::Cancelled);
    }

    #[cfg(unix)]
    {
        run_process_unix(binary, args, stdin_path, stdout_path, limits, cancel)
    }
    #[cfg(windows)]
    {
        run_process_windows(binary, args, stdin_path, stdout_path, limits, cancel)
    }
}

/// Result of running a testlib checker or interactor, including `quitp` partial score ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckerOutcome {
    pub verdict: Verdict,
    /// Score ratio in `[0.0, 1.0]` (`1.0` for `_ok`, parsed ratio for `quitp`, `0.0` otherwise).
    pub score_ratio: f64,
    pub message: String,
}

/// Strips ANSI escape sequences (e.g. `\x1b[1;33m`) that testlib may emit to stderr.
fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for nc in chars.by_ref() {
                if ('@'..='~').contains(&nc) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses a `testlib.h` exit code and stderr into `(Verdict, score_ratio, clean_message)`.
///
/// Testlib exit codes:
/// - `0` -> `AC` (1.0)
/// - `1` -> `WA` (0.0)
/// - `2` -> `PE` (0.0)
/// - `3` -> `JE` (0.0, `_fail`)
/// - `7` -> `quitp` partial score (`points <ratio> [msg]`): ratio in `[0.0, 1.0]`; `AC` if `> 0.0`, `WA` if `0.0`
/// - `8` -> `_unexpected_eof` -> `WA` (0.0)
/// - anything else -> `JE` (0.0)
pub fn parse_testlib_verdict(exit_code: Option<i32>, stderr: &str) -> CheckerOutcome {
    let clean = strip_ansi_codes(stderr).trim().to_owned();
    match exit_code {
        Some(0) => CheckerOutcome {
            verdict: Verdict::AC,
            score_ratio: 1.0,
            message: clean,
        },
        Some(1) | Some(8) => CheckerOutcome {
            verdict: Verdict::WA,
            score_ratio: 0.0,
            message: clean,
        },
        Some(2) => CheckerOutcome {
            verdict: Verdict::PE,
            score_ratio: 0.0,
            message: clean,
        },
        Some(7) => {
            // Testlib prints `points <number> [message]`.
            let rest = clean
                .strip_prefix("points ")
                .or_else(|| clean.strip_prefix("Points "))
                .or_else(|| clean.strip_prefix("POINTS "))
                .unwrap_or(&clean)
                .trim_start();
            let first_tok = rest.split_whitespace().next().unwrap_or("");
            if let Ok(val) = first_tok.parse::<f64>()
                && val.is_finite()
            {
                let ratio = val.clamp(0.0, 1.0);
                let verdict = if ratio > 0.0 {
                    Verdict::AC
                } else {
                    Verdict::WA
                };
                CheckerOutcome {
                    verdict,
                    score_ratio: ratio,
                    message: clean,
                }
            } else {
                CheckerOutcome {
                    verdict: Verdict::JE,
                    score_ratio: 0.0,
                    message: clean,
                }
            }
        }
        _ => CheckerOutcome {
            verdict: Verdict::JE,
            score_ratio: 0.0,
            message: clean,
        },
    }
}

/// Runs a testlib checker `<checker> <in> <out> <ans>` with custom resource limits.
pub fn run_checker_with_score_and_limits(
    checker_bin: &Path,
    input_path: &Path,
    output_path: &Path,
    answer_path: &Path,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<CheckerOutcome> {
    let args = vec![
        input_path.to_string_lossy().into_owned(),
        output_path.to_string_lossy().into_owned(),
        answer_path.to_string_lossy().into_owned(),
    ];
    let outcome = run_process(checker_bin, &args, None, None, limits, cancel)?;
    Ok(parse_testlib_verdict(outcome.exit_code, &outcome.stderr))
}

/// Runs a testlib checker `<checker> <in> <out> <ans>` and returns verdict, `quitp` ratio, and message.
pub fn run_checker_with_score(
    checker_bin: &Path,
    input_path: &Path,
    output_path: &Path,
    answer_path: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<CheckerOutcome> {
    run_checker_with_score_and_limits(
        checker_bin,
        input_path,
        output_path,
        answer_path,
        RunLimits::tool_defaults(),
        cancel,
    )
}

/// Runs a testlib checker `<checker> <in> <out> <ans>` and maps its exit code to a [`Verdict`].
pub fn run_checker(
    checker_bin: &Path,
    input_path: &Path,
    output_path: &Path,
    answer_path: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(Verdict, String)> {
    let res = run_checker_with_score(checker_bin, input_path, output_path, answer_path, cancel)?;
    Ok((res.verdict, res.message))
}

/// Runs a testlib validator with custom resource limits and optional `--group <id>`.
pub fn run_validator_with_group_and_limits(
    validator_bin: &Path,
    input_path: &Path,
    group: Option<&str>,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<std::result::Result<(), String>> {
    let args: Vec<String> = match group.map(str::trim).filter(|s| !s.is_empty()) {
        Some(g) => vec!["--group".to_owned(), g.to_owned()],
        None => Vec::new(),
    };
    let outcome = run_process(validator_bin, &args, Some(input_path), None, limits, cancel)?;
    if outcome.exit_code == Some(0) && outcome.signal.is_none() {
        Ok(Ok(()))
    } else {
        let msg = if outcome.stderr.trim().is_empty() {
            format!(
                "validator exited with code {:?}, signal {:?}",
                outcome.exit_code, outcome.signal
            )
        } else {
            outcome.stderr.trim().to_owned()
        };
        Ok(Err(msg))
    }
}

/// Runs a testlib validator with `input_path` on stdin, optionally passing `--group <id>`.
/// Returns `Ok(())` if exit code is 0, or `Err(diagnostic_message)` if invalid.
pub fn run_validator_with_group(
    validator_bin: &Path,
    input_path: &Path,
    group: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<std::result::Result<(), String>> {
    run_validator_with_group_and_limits(
        validator_bin,
        input_path,
        group,
        RunLimits::tool_defaults(),
        cancel,
    )
}

/// Runs a testlib validator with `input_path` on stdin (no `--group`).
/// Returns `Ok(())` if exit code is 0, or `Err(diagnostic_message)` if invalid.
pub fn run_validator(
    validator_bin: &Path,
    input_path: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<std::result::Result<(), String>> {
    run_validator_with_group(validator_bin, input_path, None, cancel)
}

#[cfg(target_os = "linux")]
const LINUX_RUNNER_HELPER_CPP: &str = r#"
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t g_child_pid = 0;

static void on_term(int) {
    pid_t p = static_cast<pid_t>(g_child_pid);
    if (p > 0) {
        kill(-p, SIGKILL);
        kill(p, SIGKILL);
    }
}

static void set_lim(int resource, unsigned long long soft, unsigned long long hard) {
    struct rlimit lim;
    lim.rlim_cur = static_cast<rlim_t>(soft);
    lim.rlim_max = static_cast<rlim_t>(hard);
    setrlimit(resource, &lim);
}

int main(int argc, char* argv[]) {
    // Usage: helper <meta_path> <as_bytes> <fsize_bytes> <cpu_secs> <mem_bytes> <binary> [args...]
    if (argc < 7) return 2;
    const char* meta_path = argv[1];
    unsigned long long as_bytes = std::strtoull(argv[2], nullptr, 10);
    unsigned long long fsize_bytes = std::strtoull(argv[3], nullptr, 10);
    unsigned long long cpu_secs = std::strtoull(argv[4], nullptr, 10);
    unsigned long long mem_bytes = std::strtoull(argv[5], nullptr, 10);

    struct sigaction sa;
    std::memset(&sa, 0, sizeof(sa));
    sa.sa_handler = on_term;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGTERM, &sa, nullptr);

    pid_t pid = fork();
    if (pid < 0) return 3;
    if (pid == 0) {
        prctl(PR_SET_PDEATHSIG, SIGKILL);
        setpgid(0, 0);
        if (as_bytes > 0) set_lim(RLIMIT_AS, as_bytes, as_bytes);
        if (fsize_bytes > 0) set_lim(RLIMIT_FSIZE, fsize_bytes, fsize_bytes);
        if (cpu_secs > 0) set_lim(RLIMIT_CPU, cpu_secs, cpu_secs + 1);
        struct rlimit cur_stack;
        if (getrlimit(RLIMIT_STACK, &cur_stack) == 0) {
            rlim_t desired = mem_bytes > 0 ? static_cast<rlim_t>(mem_bytes) : cur_stack.rlim_max;
            rlim_t new_soft = (cur_stack.rlim_max == RLIM_INFINITY)
                ? desired
                : (desired < cur_stack.rlim_max ? desired : cur_stack.rlim_max);
            if (new_soft > cur_stack.rlim_cur) {
                struct rlimit lim = { new_soft, cur_stack.rlim_max };
                setrlimit(RLIMIT_STACK, &lim);
            }
        }
        execv(argv[6], &argv[6]);
        _exit(127);
    }

    g_child_pid = pid;

    int status = 0;
    struct rusage ru;
    std::memset(&ru, 0, sizeof(ru));
    while (wait4(pid, &status, 0, &ru) < 0) {
        if (errno == EINTR) continue;
        return 4;
    }

    int exit_code = -1;
    int sig = 0;
    if (WIFEXITED(status)) {
        exit_code = WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        sig = WTERMSIG(status);
    }

    unsigned long long user_us =
        static_cast<unsigned long long>(ru.ru_utime.tv_sec) * 1000000ULL +
        static_cast<unsigned long long>(ru.ru_utime.tv_usec);
    unsigned long long sys_us =
        static_cast<unsigned long long>(ru.ru_stime.tv_sec) * 1000000ULL +
        static_cast<unsigned long long>(ru.ru_stime.tv_usec);
    unsigned long long maxrss_kib = static_cast<unsigned long long>(ru.ru_maxrss);

    FILE* f = std::fopen(meta_path, "w");
    if (!f) return 5;
    std::fprintf(f, "%d %d %llu %llu %llu\n", exit_code, sig, user_us, sys_us, maxrss_kib);
    std::fclose(f);
    return 0;
}
"#;

#[cfg(target_os = "linux")]
fn ensure_linux_runner_helper() -> Option<std::path::PathBuf> {
    use std::sync::OnceLock;
    static HELPER: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    HELPER
        .get_or_init(|| {
            let compiler = crate::toolchain::resolve_active_compiler(None).ok()?;
            let cache = crate::cache::JudgeCache::open_default().ok()?;
            let testlib = crate::assets::ResolvedTestlib::Bundled {
                content_hash: String::new(),
            };
            let res = crate::toolchain::compile_cached(
                &cache,
                &compiler,
                crate::toolchain::SourceLanguage::Cpp {
                    std: crate::toolchain::CppStd::Cpp20,
                },
                "xcpc-runner-helper",
                "<builtin:runner-helper>",
                LINUX_RUNNER_HELPER_CPP.as_bytes(),
                None,
                &testlib,
            )
            .ok()?;
            Some(res.binary_path)
        })
        .clone()
}

#[cfg(unix)]
fn run_process_unix(
    binary: &Path,
    args: &[String],
    stdin_path: Option<&Path>,
    stdout_path: Option<&Path>,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<RunOutcome> {
    use std::io::Read;
    #[cfg(not(target_os = "linux"))]
    use std::os::unix::process::CommandExt;

    // Compute rlimits for the child process.
    // Virtual address space (RLIMIT_AS) gets headroom over RSS limit so C++ runtime
    // mappings don't cause false MLE, while ru_maxrss enforces the exact memory limit.
    let mem_bytes = limits.memory_limit_kib.saturating_mul(1024);
    let as_limit_bytes = if limits.disable_as_limit {
        0
    } else {
        mem_bytes
            .saturating_mul(2)
            .max(mem_bytes.saturating_add(256 * 1024 * 1024))
    };
    let fsize_bytes = limits.fsize_limit_bytes;
    let cpu_secs = (limits.time_limit_ms / 1000).saturating_add(2);

    // On Linux, spawning a child directly from a large parent (e.g. Desktop GUI with ~166 MB
    // RSS) leaks the parent's `mm->hiwater_rss` into the child's `wait4` `ru_maxrss` even
    // under `posix_spawn` (`CLONE_VM | CLONE_VFORK`). Running through a tiny ~2 MB helper
    // binary gives the target a clean `mm` before `execv` and accurate `ru_maxrss` + exact
    // pre-exec `setrlimit`.
    #[cfg(target_os = "linux")]
    let (mut cmd, meta_file) = if let Some(helper_bin) = ensure_linux_runner_helper() {
        let meta =
            tempfile::NamedTempFile::new().map_err(|e| io_err("create runner meta file", e))?;
        let mut c = Command::new(&helper_bin);
        c.arg(meta.path())
            .arg(as_limit_bytes.to_string())
            .arg(fsize_bytes.to_string())
            .arg(cpu_secs.to_string())
            .arg(mem_bytes.to_string())
            .arg(binary)
            .args(args);
        (c, Some(meta))
    } else {
        let mut c = Command::new(binary);
        c.args(args);
        (c, None)
    };

    #[cfg(not(target_os = "linux"))]
    let mut cmd = {
        let mut c = Command::new(binary);
        c.args(args);
        c
    };

    match stdin_path {
        Some(p) => {
            let f = File::open(p).map_err(|e| io_err(format!("open stdin {}", p.display()), e))?;
            cmd.stdin(Stdio::from(f));
        }
        None => {
            cmd.stdin(Stdio::null());
        }
    }

    match stdout_path {
        Some(p) => {
            let f =
                File::create(p).map_err(|e| io_err(format!("create stdout {}", p.display()), e))?;
            cmd.stdout(Stdio::from(f));
        }
        None => {
            cmd.stdout(Stdio::null());
        }
    }

    cmd.stderr(Stdio::piped());
    if limits.disable_as_limit {
        cmd.env(
            "ASAN_OPTIONS",
            "detect_leaks=0:halt_on_error=1:abort_on_error=0",
        )
        .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1");
    }

    #[cfg(not(target_os = "linux"))]
    // SAFETY: setrlimit and setpgid are async-signal-safe syscalls.
    unsafe {
        cmd.pre_exec(move || {
            // Put child in its own process group so watchdog kill cleans up any grandchildren.
            let _ = libc::setpgid(0, 0);

            let set_lim = |resource, soft: u64, hard: u64| {
                let lim = libc::rlimit {
                    rlim_cur: soft as libc::rlim_t,
                    rlim_max: hard as libc::rlim_t,
                };
                let _ = libc::setrlimit(resource, &lim);
            };

            if as_limit_bytes > 0 {
                set_lim(libc::RLIMIT_AS, as_limit_bytes, as_limit_bytes);
            }
            if fsize_bytes > 0 {
                set_lim(libc::RLIMIT_FSIZE, fsize_bytes, fsize_bytes);
            }
            if cpu_secs > 0 {
                set_lim(libc::RLIMIT_CPU, cpu_secs, cpu_secs.saturating_add(1));
            }
            // Raise stack soft limit up to hard limit (or memory limit) to avoid DFS stack overflow.
            let mut cur_stack = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::getrlimit(libc::RLIMIT_STACK, &mut cur_stack) == 0 {
                let desired = if mem_bytes > 0 {
                    mem_bytes as libc::rlim_t
                } else {
                    cur_stack.rlim_max
                };
                let new_soft = if cur_stack.rlim_max == libc::RLIM_INFINITY {
                    desired
                } else {
                    desired.min(cur_stack.rlim_max)
                };
                if new_soft > cur_stack.rlim_cur {
                    let lim = libc::rlimit {
                        rlim_cur: new_soft,
                        rlim_max: cur_stack.rlim_max,
                    };
                    let _ = libc::setrlimit(libc::RLIMIT_STACK, &lim);
                }
            }
            Ok(())
        });
    }

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn {}", binary.display()), e))?;
    let pid = child.id() as libc::pid_t;

    // Drain stderr on a helper thread so the child never blocks on a full stderr pipe.
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut limited = (&mut pipe).take(64 * 1024);
            let _ = limited.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    });

    // Watchdog thread to kill child on wall-clock timeout or cancellation.
    let finished = Arc::new(AtomicBool::new(false));
    let killed_by_wall = Arc::new(AtomicBool::new(false));
    let killed_by_cancel = Arc::new(AtomicBool::new(false));

    let wd_finished = Arc::clone(&finished);
    let wd_wall = Arc::clone(&killed_by_wall);
    let wd_cancel_hit = Arc::clone(&killed_by_cancel);
    let wd_cancel = cancel.cloned();
    let wall_limit = Duration::from_millis(limits.wall_limit_ms);

    let watchdog = std::thread::spawn(move || {
        let kill_child = || unsafe {
            #[cfg(not(target_os = "linux"))]
            let _ = libc::kill(-pid, libc::SIGKILL);
            let _ = libc::kill(pid, libc::SIGKILL);
        };
        while !wd_finished.load(Ordering::Acquire) {
            if let Some(ref c) = wd_cancel
                && c.load(Ordering::Relaxed)
            {
                wd_cancel_hit.store(true, Ordering::Release);
                kill_child();
                return;
            }
            if start.elapsed() >= wall_limit {
                wd_wall.store(true, Ordering::Release);
                kill_child();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    // Reap child with wait4 to obtain exact rusage (CPU time + peak RSS).
    let mut status: libc::c_int = 0;
    let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
    let wait_ret = loop {
        let r = unsafe { libc::wait4(pid, &mut status, 0, &mut rusage) };
        if r == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            finished.store(true, Ordering::Release);
            let _ = watchdog.join();
            return Err(io_err("wait4", err));
        }
        break r;
    };
    let _ = wait_ret;
    let wall_time_ms = start.elapsed().as_millis() as u64;
    finished.store(true, Ordering::Release);
    let _ = watchdog.join();

    // Forget child handle since we already reaped its PID via wait4.
    std::mem::forget(child);

    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if killed_by_cancel.load(Ordering::Acquire) {
        return Err(JudgeError::Cancelled);
    }

    // Parse isolated child metrics from helper meta file on Linux if available.
    #[cfg(target_os = "linux")]
    let helper_metrics: Option<(Option<i32>, Option<i32>, u64, u64)> =
        meta_file.as_ref().and_then(|mf| {
            let text = std::fs::read_to_string(mf.path()).ok()?;
            let mut parts = text.split_whitespace();
            let code: i32 = parts.next()?.parse().ok()?;
            let sig: i32 = parts.next()?.parse().ok()?;
            let user_us: u64 = parts.next()?.parse().ok()?;
            let sys_us: u64 = parts.next()?.parse().ok()?;
            let maxrss_kib: u64 = parts.next()?.parse().ok()?;
            let exit_code = (code >= 0).then_some(code);
            let signal = (sig > 0).then_some(sig);
            let cpu_ms = user_us.saturating_add(sys_us) / 1000;
            Some((exit_code, signal, cpu_ms, maxrss_kib))
        });
    #[cfg(not(target_os = "linux"))]
    let helper_metrics: Option<(Option<i32>, Option<i32>, u64, u64)> = None;

    let (exit_code, signal, cpu_time_ms, peak_memory_kib) = if let Some(m) = helper_metrics {
        m
    } else {
        let user_ms = (rusage.ru_utime.tv_sec as u64)
            .saturating_mul(1000)
            .saturating_add((rusage.ru_utime.tv_usec as u64) / 1000);
        let sys_ms = (rusage.ru_stime.tv_sec as u64)
            .saturating_mul(1000)
            .saturating_add((rusage.ru_stime.tv_usec as u64) / 1000);
        let cpu_ms = user_ms.saturating_add(sys_ms);

        // On Linux ru_maxrss is in KiB; on macOS it is in bytes.
        #[cfg(target_os = "macos")]
        let peak_kib = (rusage.ru_maxrss as u64) / 1024;
        #[cfg(not(target_os = "macos"))]
        let peak_kib = rusage.ru_maxrss as u64;

        let (ec, sig) = if libc::WIFEXITED(status) {
            (Some(libc::WEXITSTATUS(status)), None)
        } else if libc::WIFSIGNALED(status) {
            (None, Some(libc::WTERMSIG(status)))
        } else {
            (None, None)
        };
        (ec, sig, cpu_ms, peak_kib)
    };

    let is_bad_alloc = stderr.contains("std::bad_alloc") || stderr.contains("bad_alloc");

    let verdict = if killed_by_wall.load(Ordering::Acquire)
        || signal == Some(libc::SIGXCPU)
        || cpu_time_ms > limits.time_limit_ms
    {
        Verdict::TLE
    } else if (limits.memory_limit_kib > 0 && peak_memory_kib > limits.memory_limit_kib)
        || ((signal.is_some() || exit_code != Some(0)) && is_bad_alloc)
    {
        Verdict::MLE
    } else if signal.is_some() || exit_code != Some(0) {
        Verdict::RE
    } else {
        Verdict::AC
    };

    Ok(RunOutcome {
        verdict,
        exit_code,
        signal,
        cpu_time_ms,
        wall_time_ms,
        peak_memory_kib,
        stderr,
    })
}

#[cfg(windows)]
fn run_process_windows(
    binary: &Path,
    args: &[String],
    stdin_path: Option<&Path>,
    stdout_path: Option<&Path>,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<RunOutcome> {
    use std::io::Read;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetProcessTimes;

    let binary_arg = xcpc_storage::strip_verbatim_prefix(binary);
    let mut cmd = Command::new(&binary_arg);
    for arg in args {
        cmd.arg(xcpc_storage::strip_verbatim_prefix(Path::new(arg)));
    }

    match stdin_path {
        Some(p) => {
            let f = File::open(p).map_err(|e| io_err(format!("open stdin {}", p.display()), e))?;
            cmd.stdin(Stdio::from(f));
        }
        None => {
            cmd.stdin(Stdio::null());
        }
    }
    match stdout_path {
        Some(p) => {
            let f =
                File::create(p).map_err(|e| io_err(format!("create stdout {}", p.display()), e))?;
            cmd.stdout(Stdio::from(f));
        }
        None => {
            cmd.stdout(Stdio::null());
        }
    }
    cmd.stderr(Stdio::piped());
    if limits.disable_as_limit {
        cmd.env(
            "ASAN_OPTIONS",
            "detect_leaks=0:halt_on_error=1:abort_on_error=0",
        )
        .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1");
    }

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn {}", binary.display()), e))?;

    // Attach to a Job Object to enforce process-tree cleanup and memory limit.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if !job.is_null() {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if limits.memory_limit_kib > 0 {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = (limits.memory_limit_kib as usize).saturating_mul(1024);
        }
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            AssignProcessToJobObject(job, child.as_raw_handle() as _);
        }
    }

    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut limited = (&mut pipe).take(64 * 1024);
            let _ = limited.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    });

    let wall_limit = Duration::from_millis(limits.wall_limit_ms);
    let mut killed_by_wall = false;
    let mut killed_by_cancel = false;

    let status = loop {
        if let Some(flag) = cancel
            && flag.load(Ordering::Relaxed)
        {
            let _ = child.kill();
            killed_by_cancel = true;
            break child.wait().map_err(|e| io_err("wait", e))?;
        }
        if let Some(s) = child.try_wait().map_err(|e| io_err("try_wait", e))? {
            break s;
        }
        if start.elapsed() >= wall_limit {
            let _ = child.kill();
            killed_by_wall = true;
            break child.wait().map_err(|e| io_err("wait", e))?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let wall_time_ms = start.elapsed().as_millis() as u64;

    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit_ft: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
    let mut user: FILETIME = unsafe { std::mem::zeroed() };
    let cpu_time_ms = unsafe {
        if GetProcessTimes(
            child.as_raw_handle() as _,
            &mut creation,
            &mut exit_ft,
            &mut kernel,
            &mut user,
        ) != 0
        {
            let ft_to_ms = |ft: FILETIME| -> u64 {
                let ticks = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
                ticks / 10_000
            };
            ft_to_ms(kernel).saturating_add(ft_to_ms(user))
        } else {
            wall_time_ms
        }
    };

    if !job.is_null() {
        unsafe {
            CloseHandle(job);
        }
    }

    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if killed_by_cancel {
        return Err(JudgeError::Cancelled);
    }

    let exit_code = status.code();
    let verdict = if killed_by_wall || cpu_time_ms > limits.time_limit_ms {
        Verdict::TLE
    } else if exit_code != Some(0) {
        Verdict::RE
    } else {
        Verdict::AC
    };

    Ok(RunOutcome {
        verdict,
        exit_code,
        signal: None,
        cpu_time_ms,
        wall_time_ms,
        peak_memory_kib: 0,
        stderr,
    })
}

/// Outcome of an interactive (or multi-run interactive) execution.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractiveOutcome {
    pub verdict: Verdict,
    pub score_ratio: f64,
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub peak_memory_kib: u64,
    pub checker_message: String,
    pub stderr: String,
    pub transcript: InteractionTranscript,
}

const TRANSCRIPT_HEAD_LIMIT: usize = 30;
const TRANSCRIPT_TAIL_LIMIT: usize = 30;
const TRANSCRIPT_MAX_ENTRY_CHARS: usize = 512;

#[derive(Debug, Default)]
struct TranscriptRecorder {
    head: Vec<TranscriptEntry>,
    tail: VecDeque<TranscriptEntry>,
    omitted_entries: usize,
    total_entries: usize,
    total_bytes_i2s: u64,
    total_bytes_s2i: u64,
    truncated: bool,
}

impl TranscriptRecorder {
    fn add_bytes(&mut self, dir: TranscriptDirection, n: u64) {
        match dir {
            TranscriptDirection::InteractorToSolution => {
                self.total_bytes_i2s = self.total_bytes_i2s.saturating_add(n);
            }
            TranscriptDirection::SolutionToInteractor => {
                self.total_bytes_s2i = self.total_bytes_s2i.saturating_add(n);
            }
        }
    }

    fn push_line(&mut self, dir: TranscriptDirection, raw: &str, run: Option<u32>) {
        let trimmed_cr = raw.strip_suffix('\r').unwrap_or(raw);
        let text = if trimmed_cr.chars().count() > TRANSCRIPT_MAX_ENTRY_CHARS {
            self.truncated = true;
            let prefix: String = trimmed_cr
                .chars()
                .take(TRANSCRIPT_MAX_ENTRY_CHARS)
                .collect();
            let extra = trimmed_cr.chars().count() - TRANSCRIPT_MAX_ENTRY_CHARS;
            format!("{prefix}… (+{extra} chars)")
        } else {
            trimmed_cr.to_owned()
        };
        self.total_entries += 1;
        let entry = TranscriptEntry {
            direction: dir,
            text,
            run,
        };
        if self.head.len() < TRANSCRIPT_HEAD_LIMIT {
            self.head.push(entry);
        } else {
            if self.tail.len() >= TRANSCRIPT_TAIL_LIMIT {
                self.tail.pop_front();
                self.omitted_entries += 1;
                self.truncated = true;
            }
            self.tail.push_back(entry);
        }
    }

    fn merge_from(&mut self, other: &InteractionTranscript) {
        self.total_bytes_i2s = self.total_bytes_i2s.saturating_add(other.total_bytes_i2s);
        self.total_bytes_s2i = self.total_bytes_s2i.saturating_add(other.total_bytes_s2i);
        if other.truncated {
            self.truncated = true;
        }
        for e in &other.head {
            self.push_entry(e.clone());
        }
        self.omitted_entries += other.omitted_entries;
        self.total_entries += other.omitted_entries;
        if other.omitted_entries > 0 {
            self.truncated = true;
        }
        for e in &other.tail {
            self.push_entry(e.clone());
        }
    }

    fn push_entry(&mut self, entry: TranscriptEntry) {
        self.total_entries += 1;
        if self.head.len() < TRANSCRIPT_HEAD_LIMIT {
            self.head.push(entry);
        } else {
            if self.tail.len() >= TRANSCRIPT_TAIL_LIMIT {
                self.tail.pop_front();
                self.omitted_entries += 1;
                self.truncated = true;
            }
            self.tail.push_back(entry);
        }
    }

    fn finish(self, interactor_message: String) -> InteractionTranscript {
        InteractionTranscript {
            head: self.head,
            tail: self.tail.into_iter().collect(),
            omitted_entries: self.omitted_entries,
            total_entries: self.total_entries,
            total_bytes_i2s: self.total_bytes_i2s,
            total_bytes_s2i: self.total_bytes_s2i,
            truncated: self.truncated,
            interactor_message,
        }
    }
}

fn spawn_pipe_proxy<R, W>(
    mut reader: R,
    mut writer: W,
    direction: TranscriptDirection,
    run: Option<u32>,
    recorder: Arc<Mutex<TranscriptRecorder>>,
    start: Instant,
    last_activity_ms: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut line_buf: Vec<u8> = Vec::new();
        let mut peer_closed = false;

        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let elapsed = start.elapsed().as_millis() as u64;
            last_activity_ms.store(elapsed, Ordering::Relaxed);

            let chunk = &buf[..n];
            if !peer_closed && (writer.write_all(chunk).is_err() || writer.flush().is_err()) {
                peer_closed = true;
            }

            if let Ok(mut rec) = recorder.lock() {
                rec.add_bytes(direction, n as u64);
                for &b in chunk {
                    if b == b'\n' {
                        let s = String::from_utf8_lossy(&line_buf);
                        rec.push_line(direction, &s, run);
                        line_buf.clear();
                    } else if line_buf.len() < TRANSCRIPT_MAX_ENTRY_CHARS * 4 {
                        line_buf.push(b);
                    }
                }
            }
        }

        if !line_buf.is_empty()
            && let Ok(mut rec) = recorder.lock()
        {
            let s = String::from_utf8_lossy(&line_buf);
            rec.push_line(direction, &s, run);
        }
        drop(writer);
    })
}

/// Runs a single interactive pass connecting `interactor_bin <input_path> <tout_path>` and
/// `solution_bin` via two controlled proxy pipes, recording a bounded transcript and
/// distinguishing CPU `TLE` from deadlock/idleness `ILE`.
pub fn run_interactive(
    interactor_bin: &Path,
    solution_bin: &Path,
    input_path: &Path,
    tout_path: &Path,
    limits: RunLimits,
    run_index: Option<u32>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<InteractiveOutcome> {
    if let Some(flag) = cancel
        && flag.load(Ordering::Relaxed)
    {
        return Err(JudgeError::Cancelled);
    }

    #[cfg(unix)]
    {
        run_interactive_unix(
            interactor_bin,
            solution_bin,
            input_path,
            tout_path,
            limits,
            run_index,
            cancel,
        )
    }
    #[cfg(windows)]
    {
        run_interactive_windows(
            interactor_bin,
            solution_bin,
            input_path,
            tout_path,
            limits,
            run_index,
            cancel,
        )
    }
}

#[cfg(unix)]
fn run_interactive_unix(
    interactor_bin: &Path,
    solution_bin: &Path,
    input_path: &Path,
    tout_path: &Path,
    limits: RunLimits,
    run_index: Option<u32>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<InteractiveOutcome> {
    #[cfg(not(target_os = "linux"))]
    use std::os::unix::process::CommandExt;

    let mem_bytes = limits.memory_limit_kib.saturating_mul(1024);
    let as_limit_bytes = if limits.disable_as_limit {
        0
    } else {
        mem_bytes
            .saturating_mul(2)
            .max(mem_bytes.saturating_add(256 * 1024 * 1024))
    };
    let fsize_bytes = limits.fsize_limit_bytes;
    let cpu_secs = (limits.time_limit_ms / 1000).saturating_add(2);

    // Spawn interactor: `interactor <input_path> <tout_path>`.
    let mut inter_cmd = Command::new(interactor_bin);
    inter_cmd
        .arg(input_path)
        .arg(tout_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if limits.disable_as_limit {
        inter_cmd
            .env(
                "ASAN_OPTIONS",
                "detect_leaks=0:halt_on_error=1:abort_on_error=0",
            )
            .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1");
    }

    // Spawn solution (via Linux runner helper when available for exact rusage + rlimits).
    #[cfg(target_os = "linux")]
    let (mut sol_cmd, meta_file) = if let Some(helper_bin) = ensure_linux_runner_helper() {
        let meta =
            tempfile::NamedTempFile::new().map_err(|e| io_err("create runner meta file", e))?;
        let mut c = Command::new(&helper_bin);
        c.arg(meta.path())
            .arg(as_limit_bytes.to_string())
            .arg(fsize_bytes.to_string())
            .arg(cpu_secs.to_string())
            .arg(mem_bytes.to_string())
            .arg(solution_bin);
        (c, Some(meta))
    } else {
        (Command::new(solution_bin), None)
    };
    #[cfg(not(target_os = "linux"))]
    let mut sol_cmd = {
        let mut c = Command::new(solution_bin);
        unsafe {
            c.pre_exec(move || {
                let _ = libc::setpgid(0, 0);
                let set_lim = |resource, soft: u64, hard: u64| {
                    let lim = libc::rlimit {
                        rlim_cur: soft as libc::rlim_t,
                        rlim_max: hard as libc::rlim_t,
                    };
                    let _ = libc::setrlimit(resource, &lim);
                };
                if as_limit_bytes > 0 {
                    set_lim(libc::RLIMIT_AS, as_limit_bytes, as_limit_bytes);
                }
                if fsize_bytes > 0 {
                    set_lim(libc::RLIMIT_FSIZE, fsize_bytes, fsize_bytes);
                }
                if cpu_secs > 0 {
                    set_lim(libc::RLIMIT_CPU, cpu_secs, cpu_secs.saturating_add(1));
                }
                Ok(())
            });
        }
        c
    };

    sol_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if limits.disable_as_limit {
        sol_cmd
            .env(
                "ASAN_OPTIONS",
                "detect_leaks=0:halt_on_error=1:abort_on_error=0",
            )
            .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1");
    }

    let start = Instant::now();
    let mut inter_child = inter_cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn interactor {}", interactor_bin.display()), e))?;
    let mut sol_child = sol_cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn solution {}", solution_bin.display()), e))?;

    let inter_pid = inter_child.id() as libc::pid_t;
    let sol_pid = sol_child.id() as libc::pid_t;

    let inter_stdin = inter_child.stdin.take().unwrap();
    let inter_stdout = inter_child.stdout.take().unwrap();
    let inter_stderr = inter_child.stderr.take().unwrap();

    let sol_stdin = sol_child.stdin.take().unwrap();
    let sol_stdout = sol_child.stdout.take().unwrap();
    let sol_stderr = sol_child.stderr.take().unwrap();

    let recorder = Arc::new(Mutex::new(TranscriptRecorder::default()));
    let last_activity_ms = Arc::new(AtomicU64::new(0));

    let i2s_handle = spawn_pipe_proxy(
        inter_stdout,
        sol_stdin,
        TranscriptDirection::InteractorToSolution,
        run_index,
        Arc::clone(&recorder),
        start,
        Arc::clone(&last_activity_ms),
    );
    let s2i_handle = spawn_pipe_proxy(
        sol_stdout,
        inter_stdin,
        TranscriptDirection::SolutionToInteractor,
        run_index,
        Arc::clone(&recorder),
        start,
        Arc::clone(&last_activity_ms),
    );

    let sol_stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = sol_stderr.take(64 * 1024);
        let _ = limited.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let inter_stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = inter_stderr.take(64 * 1024);
        let _ = limited.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    // Idleness limit: always strictly above `time_limit_ms` (so legitimate CPU work between
    // queries never triggers false ILE even when `time_limit_ms > 5s`), while still detecting
    // deadlocks (e.g. forgotten flush) promptly.
    let idle_limit_ms = limits
        .time_limit_ms
        .saturating_add(1500)
        .min(limits.wall_limit_ms);
    let wall_limit_ms = limits.wall_limit_ms;

    let terminate_sol = |pid: libc::pid_t| unsafe {
        #[cfg(target_os = "linux")]
        {
            // SIGTERM tells helper to SIGKILL child, reap rusage, write meta_file, and exit.
            let _ = libc::kill(pid, libc::SIGTERM);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = libc::kill(-pid, libc::SIGKILL);
            let _ = libc::kill(pid, libc::SIGKILL);
        }
    };
    let force_kill = |pid: libc::pid_t| unsafe {
        let _ = libc::kill(pid, libc::SIGKILL);
    };

    let mut sol_exited: Option<(libc::c_int, libc::rusage)> = None;
    let mut inter_exited: Option<libc::c_int> = None;
    let mut inter_exited_at: Option<Instant> = None;
    let mut sol_exited_at: Option<Instant> = None;
    let mut killed_by_cancel = false;
    let mut killed_by_timeout = false;
    let mut killed_sol_after_inter = false;
    let mut term_sent_at: Option<Instant> = None;

    loop {
        // Non-blocking reap of solution.
        if sol_exited.is_none() {
            let mut st: libc::c_int = 0;
            let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
            let r = unsafe { libc::wait4(sol_pid, &mut st, libc::WNOHANG, &mut ru) };
            if r == sol_pid {
                sol_exited = Some((st, ru));
                sol_exited_at = Some(Instant::now());
            }
        }
        // Non-blocking reap of interactor.
        if inter_exited.is_none() {
            let mut st: libc::c_int = 0;
            let r = unsafe { libc::waitpid(inter_pid, &mut st, libc::WNOHANG) };
            if r == inter_pid {
                inter_exited = Some(st);
                inter_exited_at = Some(Instant::now());
            }
        }

        if sol_exited.is_some() && inter_exited.is_some() {
            break;
        }

        // Check cancellation.
        if !killed_by_cancel
            && let Some(flag) = cancel
            && flag.load(Ordering::Relaxed)
        {
            killed_by_cancel = true;
            if sol_exited.is_none() {
                terminate_sol(sol_pid);
            }
            if inter_exited.is_none() {
                force_kill(inter_pid);
            }
            term_sent_at = Some(Instant::now());
        }

        // Check wall-clock / idleness limit.
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let idle_ms = elapsed_ms.saturating_sub(last_activity_ms.load(Ordering::Relaxed));
        if !killed_by_timeout
            && !killed_by_cancel
            && (elapsed_ms >= wall_limit_ms || idle_ms >= idle_limit_ms)
        {
            killed_by_timeout = true;
            if sol_exited.is_none() {
                terminate_sol(sol_pid);
            }
            if inter_exited.is_none() {
                force_kill(inter_pid);
            }
            term_sent_at = Some(Instant::now());
        }

        // If interactor finished first, give solution a brief grace period to observe EOF and exit.
        if let Some(t_inter) = inter_exited_at
            && sol_exited.is_none()
            && !killed_sol_after_inter
            && t_inter.elapsed() >= Duration::from_millis(500)
        {
            killed_sol_after_inter = true;
            terminate_sol(sol_pid);
            term_sent_at = Some(Instant::now());
        }

        // If solution finished first, give interactor a brief grace period to observe EOF and exit.
        if let Some(t_sol) = sol_exited_at
            && inter_exited.is_none()
            && t_sol.elapsed() >= Duration::from_millis(1000)
        {
            force_kill(inter_pid);
        }

        // Escalate SIGTERM -> SIGKILL if helper did not exit within 200ms.
        if let Some(t_term) = term_sent_at
            && t_term.elapsed() >= Duration::from_millis(200)
        {
            if sol_exited.is_none() {
                force_kill(sol_pid);
            }
            if inter_exited.is_none() {
                force_kill(inter_pid);
            }
        }

        std::thread::sleep(Duration::from_millis(5));
    }

    let wall_time_ms = start.elapsed().as_millis() as u64;
    // Forget Child handles since we already reaped both PIDs via wait4/waitpid.
    std::mem::forget(sol_child);
    std::mem::forget(inter_child);

    let _ = i2s_handle.join();
    let _ = s2i_handle.join();
    let sol_stderr = sol_stderr_handle.join().unwrap_or_default();
    let inter_stderr = inter_stderr_handle.join().unwrap_or_default();

    if killed_by_cancel {
        return Err(JudgeError::Cancelled);
    }

    let (sol_status, sol_rusage) = sol_exited.unwrap();
    let inter_status = inter_exited.unwrap();

    #[cfg(target_os = "linux")]
    let helper_metrics: Option<(Option<i32>, Option<i32>, u64, u64)> =
        meta_file.as_ref().and_then(|mf| {
            let text = std::fs::read_to_string(mf.path()).ok()?;
            let mut parts = text.split_whitespace();
            let code: i32 = parts.next()?.parse().ok()?;
            let sig: i32 = parts.next()?.parse().ok()?;
            let user_us: u64 = parts.next()?.parse().ok()?;
            let sys_us: u64 = parts.next()?.parse().ok()?;
            let maxrss_kib: u64 = parts.next()?.parse().ok()?;
            let exit_code = (code >= 0).then_some(code);
            let signal = (sig > 0).then_some(sig);
            let cpu_ms = user_us.saturating_add(sys_us) / 1000;
            Some((exit_code, signal, cpu_ms, maxrss_kib))
        });
    #[cfg(not(target_os = "linux"))]
    let helper_metrics: Option<(Option<i32>, Option<i32>, u64, u64)> = None;

    let (sol_exit, sol_signal, cpu_time_ms, peak_memory_kib) = if let Some(m) = helper_metrics {
        m
    } else {
        let user_ms = (sol_rusage.ru_utime.tv_sec as u64)
            .saturating_mul(1000)
            .saturating_add((sol_rusage.ru_utime.tv_usec as u64) / 1000);
        let sys_ms = (sol_rusage.ru_stime.tv_sec as u64)
            .saturating_mul(1000)
            .saturating_add((sol_rusage.ru_stime.tv_usec as u64) / 1000);
        #[cfg(target_os = "macos")]
        let peak_kib = (sol_rusage.ru_maxrss as u64) / 1024;
        #[cfg(not(target_os = "macos"))]
        let peak_kib = sol_rusage.ru_maxrss as u64;
        let (ec, sig) = if libc::WIFEXITED(sol_status) {
            (Some(libc::WEXITSTATUS(sol_status)), None)
        } else if libc::WIFSIGNALED(sol_status) {
            (None, Some(libc::WTERMSIG(sol_status)))
        } else {
            (None, None)
        };
        (ec, sig, user_ms.saturating_add(sys_ms), peak_kib)
    };

    let inter_exit = if libc::WIFEXITED(inter_status) {
        Some(libc::WEXITSTATUS(inter_status))
    } else {
        None
    };

    let inter_outcome = parse_testlib_verdict(inter_exit, &inter_stderr);
    let is_bad_alloc = sol_stderr.contains("std::bad_alloc") || sol_stderr.contains("bad_alloc");

    let (verdict, score_ratio, checker_message) = if sol_signal == Some(libc::SIGXCPU)
        || cpu_time_ms > limits.time_limit_ms
    {
        (
            Verdict::TLE,
            0.0,
            format!(
                "CPU 用时 {}ms，超过时限 {}ms",
                cpu_time_ms, limits.time_limit_ms
            ),
        )
    } else if killed_by_timeout {
        (
            Verdict::ILE,
            0.0,
            format!(
                "卡住了：CPU 只用了 {}ms，等待了 {}ms。检查是否刷新了输出，或双方是否都在等对方",
                cpu_time_ms, wall_time_ms
            ),
        )
    } else if (limits.memory_limit_kib > 0 && peak_memory_kib > limits.memory_limit_kib)
        || ((sol_signal.is_some() || sol_exit != Some(0)) && is_bad_alloc)
    {
        (Verdict::MLE, 0.0, "内存超过限制".to_owned())
    } else {
        let sol_crashed = !killed_sol_after_inter
            && sol_signal != Some(libc::SIGPIPE)
            && (sol_signal.is_some() || sol_exit != Some(0));
        if sol_crashed && inter_outcome.verdict == Verdict::AC {
            (Verdict::RE, 0.0, "解法异常退出".to_owned())
        } else if sol_crashed
            && matches!(inter_outcome.verdict, Verdict::WA | Verdict::PE)
            && sol_signal.is_some()
        {
            // Solution crashed with a fatal signal (e.g. SIGSEGV/SIGABRT) causing EOF in interactor.
            (
                Verdict::RE,
                0.0,
                format!("解法崩溃（信号 {:?}）", sol_signal),
            )
        } else {
            (
                inter_outcome.verdict,
                inter_outcome.score_ratio,
                inter_outcome.message,
            )
        }
    };

    let transcript = Arc::try_unwrap(recorder)
        .ok()
        .and_then(|m| m.into_inner().ok())
        .unwrap_or_default()
        .finish(checker_message.clone());

    Ok(InteractiveOutcome {
        verdict,
        score_ratio,
        cpu_time_ms,
        wall_time_ms,
        peak_memory_kib,
        checker_message,
        stderr: sol_stderr,
        transcript,
    })
}

#[cfg(windows)]
fn run_interactive_windows(
    interactor_bin: &Path,
    solution_bin: &Path,
    input_path: &Path,
    tout_path: &Path,
    limits: RunLimits,
    run_index: Option<u32>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<InteractiveOutcome> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;

    let interactor_arg = xcpc_storage::strip_verbatim_prefix(interactor_bin);
    let input_arg = xcpc_storage::strip_verbatim_prefix(input_path);
    let tout_arg = xcpc_storage::strip_verbatim_prefix(tout_path);
    let mut inter_cmd = Command::new(&interactor_arg);
    inter_cmd
        .arg(&input_arg)
        .arg(&tout_arg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let solution_arg = xcpc_storage::strip_verbatim_prefix(solution_bin);
    let mut sol_cmd = Command::new(&solution_arg);
    sol_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if limits.disable_as_limit {
        sol_cmd
            .env(
                "ASAN_OPTIONS",
                "detect_leaks=0:halt_on_error=1:abort_on_error=0",
            )
            .env("UBSAN_OPTIONS", "print_stacktrace=1:halt_on_error=1");
    }

    let start = Instant::now();
    let mut inter_child = inter_cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn interactor {}", interactor_bin.display()), e))?;
    let mut sol_child = sol_cmd
        .spawn()
        .map_err(|e| io_err(format!("spawn solution {}", solution_bin.display()), e))?;

    let inter_stdin = inter_child.stdin.take().unwrap();
    let inter_stdout = inter_child.stdout.take().unwrap();
    let inter_stderr = inter_child.stderr.take().unwrap();

    let sol_stdin = sol_child.stdin.take().unwrap();
    let sol_stdout = sol_child.stdout.take().unwrap();
    let sol_stderr = sol_child.stderr.take().unwrap();

    let recorder = Arc::new(Mutex::new(TranscriptRecorder::default()));
    let last_activity_ms = Arc::new(AtomicU64::new(0));

    let i2s_handle = spawn_pipe_proxy(
        inter_stdout,
        sol_stdin,
        TranscriptDirection::InteractorToSolution,
        run_index,
        Arc::clone(&recorder),
        start,
        Arc::clone(&last_activity_ms),
    );
    let s2i_handle = spawn_pipe_proxy(
        sol_stdout,
        inter_stdin,
        TranscriptDirection::SolutionToInteractor,
        run_index,
        Arc::clone(&recorder),
        start,
        Arc::clone(&last_activity_ms),
    );
    let sol_stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = sol_stderr.take(64 * 1024);
        let _ = limited.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let inter_stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = inter_stderr.take(64 * 1024);
        let _ = limited.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let idle_limit_ms = limits
        .time_limit_ms
        .saturating_add(1500)
        .min(limits.wall_limit_ms);
    let wall_limit_ms = limits.wall_limit_ms;

    let mut sol_status = None;
    let mut inter_status = None;
    let mut killed_by_cancel = false;
    let mut killed_by_timeout = false;

    loop {
        if sol_status.is_none() {
            sol_status = sol_child
                .try_wait()
                .map_err(|e| io_err("try_wait sol", e))?;
        }
        if inter_status.is_none() {
            inter_status = inter_child
                .try_wait()
                .map_err(|e| io_err("try_wait interactor", e))?;
        }
        if sol_status.is_some() && inter_status.is_some() {
            break;
        }
        if let Some(flag) = cancel
            && flag.load(Ordering::Relaxed)
        {
            killed_by_cancel = true;
            let _ = sol_child.kill();
            let _ = inter_child.kill();
            break;
        }
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let idle_ms = elapsed_ms.saturating_sub(last_activity_ms.load(Ordering::Relaxed));
        if elapsed_ms >= wall_limit_ms || idle_ms >= idle_limit_ms {
            killed_by_timeout = true;
            let _ = sol_child.kill();
            let _ = inter_child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let sol_status = match sol_status {
        Some(s) => s,
        None => sol_child.wait().map_err(|e| io_err("wait sol", e))?,
    };
    let inter_status = match inter_status {
        Some(s) => s,
        None => inter_child
            .wait()
            .map_err(|e| io_err("wait interactor", e))?,
    };
    let wall_time_ms = start.elapsed().as_millis() as u64;

    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit_ft: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
    let mut user: FILETIME = unsafe { std::mem::zeroed() };
    let cpu_time_ms = unsafe {
        if GetProcessTimes(
            sol_child.as_raw_handle() as _,
            &mut creation,
            &mut exit_ft,
            &mut kernel,
            &mut user,
        ) != 0
        {
            let ft_to_ms = |ft: FILETIME| -> u64 {
                let ticks = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
                ticks / 10_000
            };
            ft_to_ms(kernel).saturating_add(ft_to_ms(user))
        } else {
            0
        }
    };

    let _ = i2s_handle.join();
    let _ = s2i_handle.join();
    let sol_stderr = sol_stderr_handle.join().unwrap_or_default();
    let inter_stderr = inter_stderr_handle.join().unwrap_or_default();

    if killed_by_cancel {
        return Err(JudgeError::Cancelled);
    }

    let inter_outcome = parse_testlib_verdict(inter_status.code(), &inter_stderr);
    let (verdict, score_ratio, checker_message) = if cpu_time_ms > limits.time_limit_ms {
        (Verdict::TLE, 0.0, "超过时限".to_owned())
    } else if killed_by_timeout {
        (
            Verdict::ILE,
            0.0,
            "卡住了：等待超时，检查是否刷新了输出".to_owned(),
        )
    } else if sol_status.code() != Some(0) && inter_outcome.verdict == Verdict::AC {
        (Verdict::RE, 0.0, "解法异常退出".to_owned())
    } else {
        (
            inter_outcome.verdict,
            inter_outcome.score_ratio,
            inter_outcome.message,
        )
    };

    let transcript = Arc::try_unwrap(recorder)
        .ok()
        .and_then(|m| m.into_inner().ok())
        .unwrap_or_default()
        .finish(checker_message.clone());

    Ok(InteractiveOutcome {
        verdict,
        score_ratio,
        cpu_time_ms,
        wall_time_ms,
        peak_memory_kib: 0,
        checker_message,
        stderr: sol_stderr,
        transcript,
    })
}

/// Runs `runs` (`>= 1`) sequential interactive passes where pass `k-1`'s `tout` becomes
/// pass `k`'s `inf`. Cumulative CPU time is checked against `limits.time_limit_ms`; memory
/// limit is checked per run; aborts remaining passes on first non-AC verdict.
#[allow(clippy::too_many_arguments)]
pub fn run_multi_run_interactive(
    interactor_bin: &Path,
    solution_bin: &Path,
    input_path: &Path,
    final_tout_path: &Path,
    runs: u32,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<InteractiveOutcome> {
    let runs = runs.max(1);
    if runs == 1 {
        return run_interactive(
            interactor_bin,
            solution_bin,
            input_path,
            final_tout_path,
            limits,
            None,
            cancel,
        );
    }

    let tmp_dir = tempfile::tempdir().map_err(|e| io_err("create multi-run temp dir", e))?;
    let mut total_cpu_ms = 0u64;
    let mut total_wall_ms = 0u64;
    let mut max_mem_kib = 0u64;
    let mut min_ratio = 1.0f64;
    let mut last_msg = String::new();
    let mut combined_stderr = String::new();
    let mut merged_transcript = TranscriptRecorder::default();

    let mut current_in = input_path.to_path_buf();
    for pass in 1..=runs {
        let pass_out = if pass == runs {
            final_tout_path.to_path_buf()
        } else {
            tmp_dir.path().join(format!("pass_{pass}.out"))
        };
        let rem_cpu = limits.time_limit_ms.saturating_sub(total_cpu_ms).max(1);
        let pass_limits = RunLimits {
            time_limit_ms: rem_cpu,
            memory_limit_kib: limits.memory_limit_kib,
            fsize_limit_bytes: limits.fsize_limit_bytes,
            wall_limit_ms: rem_cpu.saturating_mul(2).saturating_add(2000).max(3000),
            disable_as_limit: limits.disable_as_limit,
        };
        let res = run_interactive(
            interactor_bin,
            solution_bin,
            &current_in,
            &pass_out,
            pass_limits,
            Some(pass),
            cancel,
        )?;
        total_cpu_ms = total_cpu_ms.saturating_add(res.cpu_time_ms);
        total_wall_ms = total_wall_ms.saturating_add(res.wall_time_ms);
        max_mem_kib = max_mem_kib.max(res.peak_memory_kib);
        merged_transcript.merge_from(&res.transcript);
        if !res.stderr.is_empty() {
            if !combined_stderr.is_empty() {
                combined_stderr.push('\n');
            }
            combined_stderr.push_str(&format!("[run {pass}] {}", res.stderr));
        }

        if total_cpu_ms > limits.time_limit_ms {
            let msg = format!(
                "cumulative time limit exceeded on run {pass}/{runs} (cpu {total_cpu_ms}ms > {}ms)",
                limits.time_limit_ms
            );
            return Ok(InteractiveOutcome {
                verdict: Verdict::TLE,
                score_ratio: 0.0,
                cpu_time_ms: total_cpu_ms,
                wall_time_ms: total_wall_ms,
                peak_memory_kib: max_mem_kib,
                checker_message: msg.clone(),
                stderr: combined_stderr,
                transcript: merged_transcript.finish(msg),
            });
        }

        if res.verdict != Verdict::AC {
            let msg = if res.checker_message.is_empty() {
                format!("第 {pass} 轮（共 {runs} 轮）：{}", res.verdict)
            } else {
                format!("第 {pass} 轮（共 {runs} 轮）：{}", res.checker_message)
            };
            return Ok(InteractiveOutcome {
                verdict: res.verdict,
                score_ratio: res.score_ratio,
                cpu_time_ms: total_cpu_ms,
                wall_time_ms: total_wall_ms,
                peak_memory_kib: max_mem_kib,
                checker_message: msg.clone(),
                stderr: combined_stderr,
                transcript: merged_transcript.finish(msg),
            });
        }

        min_ratio = min_ratio.min(res.score_ratio);
        last_msg = res.checker_message;
        current_in = pass_out;
    }

    Ok(InteractiveOutcome {
        verdict: Verdict::AC,
        score_ratio: min_ratio,
        cpu_time_ms: total_cpu_ms,
        wall_time_ms: total_wall_ms,
        peak_memory_kib: max_mem_kib,
        checker_message: last_msg.clone(),
        stderr: combined_stderr,
        transcript: merged_transcript.finish(last_msg),
    })
}

/// Runs `runs` (`>= 1`) sequential non-interactive passes where pass `k-1`'s stdout becomes
/// pass `k`'s stdin. Cumulative CPU time is checked against `limits.time_limit_ms`; memory
/// limit is checked per run; aborts remaining passes on first non-AC verdict.
pub fn run_multi_run_process(
    solution_bin: &Path,
    input_path: &Path,
    final_stdout_path: &Path,
    runs: u32,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<RunOutcome> {
    let runs = runs.max(1);
    if runs == 1 {
        return run_process(
            solution_bin,
            &[],
            Some(input_path),
            Some(final_stdout_path),
            limits,
            cancel,
        );
    }

    let tmp_dir = tempfile::tempdir().map_err(|e| io_err("create multi-run temp dir", e))?;
    let mut total_cpu_ms = 0u64;
    let mut total_wall_ms = 0u64;
    let mut max_mem_kib = 0u64;
    let mut combined_stderr = String::new();
    let mut last_exit = Some(0);
    let mut last_signal = None;

    let mut current_in = input_path.to_path_buf();
    for pass in 1..=runs {
        let pass_out = if pass == runs {
            final_stdout_path.to_path_buf()
        } else {
            tmp_dir.path().join(format!("pass_{pass}.out"))
        };
        let rem_cpu = limits.time_limit_ms.saturating_sub(total_cpu_ms).max(1);
        let pass_limits = RunLimits {
            time_limit_ms: rem_cpu,
            memory_limit_kib: limits.memory_limit_kib,
            fsize_limit_bytes: limits.fsize_limit_bytes,
            wall_limit_ms: rem_cpu.saturating_mul(2).saturating_add(2000).max(3000),
            disable_as_limit: limits.disable_as_limit,
        };
        let res = run_process(
            solution_bin,
            &[],
            Some(&current_in),
            Some(&pass_out),
            pass_limits,
            cancel,
        )?;
        total_cpu_ms = total_cpu_ms.saturating_add(res.cpu_time_ms);
        total_wall_ms = total_wall_ms.saturating_add(res.wall_time_ms);
        max_mem_kib = max_mem_kib.max(res.peak_memory_kib);
        last_exit = res.exit_code;
        last_signal = res.signal;
        if !res.stderr.is_empty() {
            if !combined_stderr.is_empty() {
                combined_stderr.push('\n');
            }
            combined_stderr.push_str(&format!("[run {pass}] {}", res.stderr));
        }

        if total_cpu_ms > limits.time_limit_ms {
            return Ok(RunOutcome {
                verdict: Verdict::TLE,
                exit_code: last_exit,
                signal: last_signal,
                cpu_time_ms: total_cpu_ms,
                wall_time_ms: total_wall_ms,
                peak_memory_kib: max_mem_kib,
                stderr: combined_stderr,
            });
        }
        if res.verdict != Verdict::AC {
            return Ok(RunOutcome {
                verdict: res.verdict,
                exit_code: last_exit,
                signal: last_signal,
                cpu_time_ms: total_cpu_ms,
                wall_time_ms: total_wall_ms,
                peak_memory_kib: max_mem_kib,
                stderr: combined_stderr,
            });
        }
        current_in = pass_out;
    }

    Ok(RunOutcome {
        verdict: Verdict::AC,
        exit_code: last_exit,
        signal: last_signal,
        cpu_time_ms: total_cpu_ms,
        wall_time_ms: total_wall_ms,
        peak_memory_kib: max_mem_kib,
        stderr: combined_stderr,
    })
}
