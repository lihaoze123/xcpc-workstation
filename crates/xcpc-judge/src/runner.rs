//! Lightweight native process runner with resource limits and usage measurement.
//!
//! - Linux/Unix: `rlimit` (`RLIMIT_AS`, `RLIMIT_STACK`, `RLIMIT_FSIZE`, `RLIMIT_CPU`) +
//!   `wait4` `rusage` (CPU time ms + peak RSS KiB) + wall-clock watchdog kill.
//! - Windows: Job Object memory limit + `GetProcessTimes` + wall-clock watchdog kill.
//!
//! No OS security sandbox is applied (local trusted author code only).

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{JudgeError, Result, io_err};

/// Standard XCPC judging verdicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    AC,
    WA,
    PE,
    TLE,
    MLE,
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
    /// Memory limit in KiB (RSS).
    pub memory_limit_kib: u64,
    /// Output file size cap in bytes (default 256 MiB).
    pub fsize_limit_bytes: u64,
    /// Wall-clock watchdog kill timeout in milliseconds.
    pub wall_limit_ms: u64,
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
        })
    }

    /// Generous limits for running tools (generators, validators, checkers).
    pub fn tool_defaults() -> Self {
        Self {
            time_limit_ms: 30_000,
            memory_limit_kib: 1024 * 1024, // 1 GiB
            fsize_limit_bytes: 512 * 1024 * 1024,
            wall_limit_ms: 60_000,
        }
    }
}

/// Parses a human-readable time limit such as `"1s"`, `"2.5s"`, `"500ms"` into milliseconds.
pub fn parse_time_limit_ms(raw: &str) -> Result<u64> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    if let Some(num) = s.strip_suffix("ms") {
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
        .strip_suffix("sec")
        .or_else(|| s.strip_suffix('s'))
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

/// Runs a testlib checker `<checker> <in> <out> <ans>` and maps its exit code to a [`Verdict`].
///
/// Testlib exit codes:
/// - `0` -> `AC` (OK)
/// - `1` -> `WA` (Wrong Answer)
/// - `2` -> `PE` (Presentation Error)
/// - `3` -> `JE` (FAIL / Judge Error)
/// - `7` -> `AC` (Points / partial OK)
/// - anything else -> `JE`
pub fn run_checker(
    checker_bin: &Path,
    input_path: &Path,
    output_path: &Path,
    answer_path: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(Verdict, String)> {
    let args = vec![
        input_path.to_string_lossy().into_owned(),
        output_path.to_string_lossy().into_owned(),
        answer_path.to_string_lossy().into_owned(),
    ];
    let outcome = run_process(
        checker_bin,
        &args,
        None,
        None,
        RunLimits::tool_defaults(),
        cancel,
    )?;
    let msg = outcome.stderr.trim().to_owned();
    let verdict = match outcome.exit_code {
        Some(0) | Some(7) => Verdict::AC,
        Some(1) => Verdict::WA,
        Some(2) => Verdict::PE,
        Some(3) => Verdict::JE,
        _ => Verdict::JE,
    };
    Ok((verdict, msg))
}

/// Runs a testlib validator with `input_path` on stdin.
/// Returns `Ok(())` if exit code is 0, or `Err(diagnostic_message)` if invalid.
pub fn run_validator(
    validator_bin: &Path,
    input_path: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<std::result::Result<(), String>> {
    let outcome = run_process(
        validator_bin,
        &[],
        Some(input_path),
        None,
        RunLimits::tool_defaults(),
        cancel,
    )?;
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
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new(binary);
    cmd.args(args);

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

    // Compute rlimits for the child process.
    // Virtual address space (RLIMIT_AS) gets headroom over RSS limit so C++ runtime
    // mappings don't cause false MLE, while ru_maxrss enforces the exact memory limit.
    let mem_bytes = limits.memory_limit_kib.saturating_mul(1024);
    let as_limit_bytes = mem_bytes
        .saturating_mul(2)
        .max(mem_bytes.saturating_add(256 * 1024 * 1024));
    let fsize_bytes = limits.fsize_limit_bytes;
    let cpu_secs = (limits.time_limit_ms / 1000).saturating_add(2);

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
        while !wd_finished.load(Ordering::Acquire) {
            if let Some(ref c) = wd_cancel
                && c.load(Ordering::Relaxed)
            {
                wd_cancel_hit.store(true, Ordering::Release);
                unsafe {
                    let _ = libc::kill(-pid, libc::SIGKILL);
                    let _ = libc::kill(pid, libc::SIGKILL);
                }
                return;
            }
            if start.elapsed() >= wall_limit {
                wd_wall.store(true, Ordering::Release);
                unsafe {
                    let _ = libc::kill(-pid, libc::SIGKILL);
                    let _ = libc::kill(pid, libc::SIGKILL);
                }
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

    let user_ms = (rusage.ru_utime.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add((rusage.ru_utime.tv_usec as u64) / 1000);
    let sys_ms = (rusage.ru_stime.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add((rusage.ru_stime.tv_usec as u64) / 1000);
    let cpu_time_ms = user_ms.saturating_add(sys_ms);

    // On Linux ru_maxrss is in KiB; on macOS it is in bytes.
    #[cfg(target_os = "macos")]
    let peak_memory_kib = (rusage.ru_maxrss as u64) / 1024;
    #[cfg(not(target_os = "macos"))]
    let peak_memory_kib = rusage.ru_maxrss as u64;

    let (exit_code, signal) = if libc::WIFEXITED(status) {
        (Some(libc::WEXITSTATUS(status)), None)
    } else if libc::WIFSIGNALED(status) {
        (None, Some(libc::WTERMSIG(status)))
    } else {
        (None, None)
    };

    let verdict = if killed_by_wall.load(Ordering::Acquire)
        || signal == Some(libc::SIGXCPU)
        || cpu_time_ms > limits.time_limit_ms
    {
        Verdict::TLE
    } else if limits.memory_limit_kib > 0 && peak_memory_kib > limits.memory_limit_kib {
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

    let mut cmd = Command::new(binary);
    cmd.args(args);

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
