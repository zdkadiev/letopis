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
        if trace.contains("lock_acquire_by_path")
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

/// Observe a real Git ignore-file read, not stdin or a test-only product checkpoint.
///
/// # Panics
/// Panics unless the native main-thread stack reaches the ignore reader before the watchdog.
#[cfg(target_os = "macos")]
pub fn observe_git_read(pid: u32, _path: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let trace = sample_process(pid);
        if is_git_read_observed(&trace) {
            return format!("inside=git-read\n{trace}");
        }
        assert!(
            Instant::now() < deadline,
            "no native Git read observed:\n{trace}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Observe the native read syscall and its exact independently owned FIFO descriptor.
///
/// # Panics
/// Panics unless the matching syscall/descriptor is observed before the watchdog.
#[cfg(target_os = "linux")]
pub fn observe_git_read(pid: u32, path: &Path) -> String {
    wait_for_trace(
        || {
            let syscall = fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
            let fields: Vec<_> = syscall.split_whitespace().collect();
            if fields.first()?.parse::<libc::c_long>().ok()? != libc::SYS_read {
                return None;
            }
            let fd = u64::from_str_radix(fields.get(1)?.strip_prefix("0x")?, 16).ok()?;
            let metadata = fs::metadata(format!("/proc/{pid}/fd/{fd}")).ok()?;
            let expected = fs::metadata(path).ok()?;
            if (metadata.dev(), metadata.ino()) != (expected.dev(), expected.ino()) {
                return None;
            }
            Some(format!("inside=git-read\n{syscall}"))
        },
        "Git ignore FIFO read",
    )
}

#[cfg(target_os = "macos")]
fn is_git_read_observed(trace: &str) -> bool {
    let main = get_main_thread_trace(trace).unwrap_or("");
    let corpus = main.lines().any(|line| {
        line.contains("letopis::git::repository::worktree_paths_get_by_ignore_mode_and_ignore_bytes")
            || (line.contains("letopis..git..repository..GitRepository")
                && line.contains("..document_traits..CorpusLister")
                && line.contains("::get_candidates"))
            || (line.contains("letopis..git..repository..GitRepository")
                && line.contains("..GitWorktreeListing")
                && line.contains("::worktree_paths_get_by_ignore_mode_and_ignore_bytes"))
    });
    // Optimized builds may inline the free function into its trait caller.
    corpus
        && main.contains("gix_glob::search::pattern::read_in_full")
        && has_native_frame(main, "read", "libsystem_kernel.dylib")
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
        .position(|line| line.contains("letopis::cli::input::request_parse"))
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{is_git_read_observed, is_stdin_read_observed};

    #[test]
    fn stdin_read_observer_recognizes_the_request_parser_on_the_main_thread() {
        // Given: a real parser frame enclosing stdin and its counted native read.
        let trace = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::cli::input::request_parse::h123 (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n900 Thread_2 letopis-sigint\n";
        // When: the observer classifies the stack of the current request parser.
        let observed = is_stdin_read_observed(trace);
        // Then: that main-thread input boundary authorizes cancellation.
        assert!(observed);
    }

    #[test]
    fn stdin_read_observer_rejects_incomplete_or_wrong_owner_evidence() {
        // Given: stacks missing ownership, native-read, stdin, ordering or main-thread evidence.
        let wrong_thread = "900 Thread_1 com.apple.main-thread\n+ 900 thread_wait (in libsystem_kernel.dylib)\n900 Thread_2 worker\n+ 900 letopis::cli::input::request_parse (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let wrong_owner = "900 Thread_1 com.apple.main-thread\n+ 900 unrelated::cli::input::request_parse (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let no_native = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::cli::input::request_parse (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\nread (in libsystem_kernel.dylib)\n";
        let wrong_image = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::cli::input::request_parse (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\n+ 900 read (in unrelated.dylib)\n";
        let no_stdin = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::cli::input::request_parse (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let wrong_order = "900 Thread_1 com.apple.main-thread\n+ 900 read (in libsystem_kernel.dylib)\n+ 900 letopis::cli::input::request_parse (in letopis)\n+ 900 std::io::stdio::Stdin::read (in letopis)\n";
        // When / Then: incomplete or unrelated evidence never authorizes cancellation.
        for trace in [
            wrong_thread,
            wrong_owner,
            no_native,
            wrong_image,
            no_stdin,
            wrong_order,
        ] {
            assert!(!is_stdin_read_observed(trace));
        }
    }

    #[test]
    fn git_read_observer_recognizes_debug_and_inlined_release_frames() {
        // Given: independent corpus frames before and after interface consolidation.
        for caller in [
            "letopis::git::repository::worktree_paths_get_by_ignore_mode_and_ignore_bytes::h123",
            "_$LT$letopis..git..repository..GitRepository$u20$as$u20$letopis..service..GitWorktreeListing$GT$::worktree_paths_get_by_ignore_mode_and_ignore_bytes::h123",
            "_$LT$letopis..git..repository..GitRepository$u20$as$u20$letopis..service..document_traits..CorpusLister$GT$::get_candidates::h123",
        ] {
            let trace = format!(
                "900 Thread_1 com.apple.main-thread\n+ 900 {caller} (in letopis)\n+ 900 gix_glob::search::pattern::read_in_full_ignore_missing (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n900 Thread_2 letopis-sigint\n"
            );
            // When: the observer classifies the stack without relying on one uninlined symbol.
            let observed = is_git_read_observed(&trace);
            // Then: either real corpus call path acknowledges the main-thread Git read.
            assert!(observed, "{caller}");
        }
    }

    #[test]
    fn git_read_observer_rejects_incomplete_or_wrong_thread_evidence() {
        // Given: counted native reads alone, another thread's Git stack, or no native read.
        let other_thread = "900 Thread_1 com.apple.main-thread\n+ 900 read (in libsystem_kernel.dylib)\n900 Thread_2 worker\n+ 900 letopis::git::repository::worktree_paths_get_by_ignore_mode_and_ignore_bytes (in letopis)\n+ 900 gix_glob::search::pattern::read_in_full (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let no_native = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::git::repository::worktree_paths_get_by_ignore_mode_and_ignore_bytes (in letopis)\n+ 900 gix_glob::search::pattern::read_in_full (in letopis)\n";
        let no_corpus = "900 Thread_1 com.apple.main-thread\n+ 900 gix_glob::search::pattern::read_in_full (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let no_git_reader = "900 Thread_1 com.apple.main-thread\n+ 900 letopis::git::repository::worktree_paths_get_by_ignore_mode_and_ignore_bytes (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        let other_owner = "900 Thread_1 com.apple.main-thread\n+ 900 _$LT$unrelated..Repository$u20$as$u20$letopis..service..document_traits..CorpusLister$GT$::get_candidates (in letopis)\n+ 900 gix_glob::search::pattern::read_in_full (in letopis)\n+ 900 read (in libsystem_kernel.dylib)\n";
        // When / Then: none authorizes signalling at the required product boundary.
        for trace in [
            other_thread,
            no_native,
            no_corpus,
            no_git_reader,
            other_owner,
        ] {
            assert!(!is_git_read_observed(trace));
        }
    }
}
