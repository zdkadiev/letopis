use super::WAIT;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{os::unix::fs::MetadataExt, path::PathBuf};

/// Wait until the process main thread is blocked reading the test FIFO.
///
/// # Panics
/// Panics when no native read observation appears before the watchdog expires.
#[cfg(target_os = "macos")]
pub fn observe_stdin_wait(pid: u32, _stdin: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let trace = sample_process(pid);
        if is_stdin_read_observed(&trace) {
            return format!("inside=stdin\n{trace}");
        }
        assert!(
            Instant::now() < deadline,
            "process never entered stdin read:\n{trace}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Wait until the process main thread is blocked reading the test FIFO.
///
/// # Panics
/// Panics when no matching syscall and descriptor identity appears before the watchdog expires.
#[cfg(target_os = "linux")]
pub fn observe_stdin_wait(pid: u32, stdin: &Path) -> String {
    wait_for_trace(
        || {
            let syscall = fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
            let fields = syscall.split_whitespace().collect::<Vec<_>>();
            let number = fields.first()?.parse::<libc::c_long>().ok()?;
            let descriptor = fields.get(1)?.strip_prefix("0x")?;
            if number != libc::SYS_read || u64::from_str_radix(descriptor, 16).ok()? != 0 {
                return None;
            }
            let observed = fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
            if observed.as_path() != stdin {
                return None;
            }
            Some(format!(
                "inside=stdin\n{}",
                serde_json::json!({
                    "pid": pid,
                    "syscall": syscall,
                    "stdin": observed,
                })
            ))
        },
        "stdin read",
    )
}

/// Wait until the process main thread is blocked in the real index lock syscall.
///
/// # Panics
/// Panics when no matching native observation appears before the watchdog expires.
#[cfg(target_os = "macos")]
pub fn observe_index_flock(pid: u32, _lock: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let trace = sample_process(pid);
        if trace.contains("lock_index")
            && has_native_frame(&trace, "flock", "libsystem_kernel.dylib")
        {
            return format!("inside=flock\n{trace}");
        }
        assert!(
            Instant::now() < deadline,
            "process never entered blocking flock:\n{trace}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Wait until the process main thread is blocked in the real index lock syscall.
///
/// # Panics
/// Panics when no matching syscall and descriptor identity appears before the watchdog expires.
#[cfg(target_os = "linux")]
pub fn observe_index_flock(pid: u32, lock: &Path) -> String {
    wait_for_trace(
        || {
            let syscall = fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
            let fields = syscall.split_whitespace().collect::<Vec<_>>();
            let number = fields.first()?.parse::<libc::c_long>().ok()?;
            let descriptor = fields.get(1)?.strip_prefix("0x")?;
            let operation = fields.get(2)?.strip_prefix("0x")?;
            if number != libc::SYS_flock
                || i32::try_from(u64::from_str_radix(operation, 16).ok()?).ok()? != libc::LOCK_EX
            {
                return None;
            }
            let descriptor = u64::from_str_radix(descriptor, 16).ok()?;
            let path = PathBuf::from(format!("/proc/{pid}/fd/{descriptor}"));
            let observed = fs::metadata(&path).ok()?;
            let expected = fs::metadata(lock).ok()?;
            if observed.ino() != expected.ino() || observed.dev() != expected.dev() {
                return None;
            }
            Some(format!(
                "inside=flock\n{}",
                serde_json::json!({
                    "pid": pid,
                    "syscall": syscall,
                    "descriptor": path,
                    "device": observed.dev(),
                    "inode": observed.ino(),
                })
            ))
        },
        "blocking flock",
    )
}

#[cfg(target_os = "macos")]
fn sample_process(pid: u32) -> String {
    let trace = tempfile::NamedTempFile::new().expect("sample file");
    let status = Command::new("/usr/bin/sample")
        .arg(pid.to_string())
        .args(["1", "1", "-file"])
        .arg(trace.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run sample");
    assert!(status.success(), "sample failed for pid {pid}");
    fs::read_to_string(trace.path()).expect("sample trace")
}

#[cfg(target_os = "macos")]
fn has_native_frame(trace: &str, symbol: &str, image: &str) -> bool {
    trace
        .lines()
        .any(|line| is_counted_native_frame(line, symbol, image))
}

#[cfg(target_os = "macos")]
fn is_stdin_read_observed(trace: &str) -> bool {
    let Some(main) = get_main_thread_trace(trace) else {
        return false;
    };
    let Some(read) = main
        .lines()
        .position(|line| is_counted_native_frame(line, "read", "libsystem_kernel.dylib"))
    else {
        return false;
    };
    let Some(parse) = main
        .lines()
        .position(|line| line.contains("letopis::cli::input::parse_request"))
    else {
        return false;
    };
    let Some(stdin) = main.lines().position(|line| line.contains("Stdin")) else {
        return false;
    };
    read > parse && read > stdin
}

#[cfg(target_os = "macos")]
fn get_main_thread_trace(trace: &str) -> Option<&str> {
    let marker = trace.find("com.apple.main-thread")?;
    let start = trace[..marker].rfind('\n').map_or(0, |index| index + 1);
    let rest = &trace[start..];
    let mut end = rest.len();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if offset > 0 && is_thread_header(line) {
            end = offset;
            break;
        }
        offset += line.len();
    }
    Some(&rest[..end])
}

#[cfg(target_os = "macos")]
fn is_thread_header(line: &str) -> bool {
    let counted = line.split_whitespace().next();
    counted.is_some_and(|count| count.parse::<u64>().is_ok()) && line.contains("Thread_")
}

#[cfg(target_os = "macos")]
fn is_counted_native_frame(line: &str, symbol: &str, image: &str) -> bool {
    // Sample indents counted frames with tree glyphs such as `+`.
    let counted = line.trim_start_matches(|character: char| !character.is_ascii_digit());
    let mut fields = counted.split_whitespace();
    fields
        .next()
        .is_some_and(|count| count.parse::<u64>().is_ok())
        && fields.next() == Some(symbol)
        && line.contains(image)
}

#[cfg_attr(
    target_os = "macos",
    expect(dead_code, reason = "Only Linux observers use the polling helper")
)]
fn wait_for_trace(mut observe: impl FnMut() -> Option<String>, waiting: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(trace) = observe() {
            return trace;
        }
        assert!(Instant::now() < deadline, "process never entered {waiting}");
        thread::sleep(Duration::from_millis(5));
    }
}
