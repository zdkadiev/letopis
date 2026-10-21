use crate::CheckResult;
use letopis_embedding_checks::{attached, capture_config, drain, pending};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::Path,
    process::{Command, Stdio},
};
#[derive(Debug, PartialEq)]
pub enum Launch {
    NoWork,
    AlreadyRunning,
    Spawned(u32),
    Failed(String),
}
fn try_lock(root: &Path) -> CheckResult<Option<File>> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join(".letopis/embed.lock"))?;
    loop {
        // SAFETY: file owns this valid descriptor; closing/death releases the kernel lease.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        match error.kind() {
            std::io::ErrorKind::WouldBlock => return Ok(None),
            std::io::ErrorKind::Interrupted => continue,
            _ => return Err(error.into()),
        }
    }
    // PID is diagnostic only, written AFTER kernel acquisition; file is never unlinked.
    file.set_len(0)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(Some(file))
}
pub fn launch_if_needed(
    root: &Path,
    executable: &Path,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<Launch> {
    // Publication-only fixtures deliberately have no embedding configuration.
    if !root.join(".letopis/config.json").exists() {
        return Ok(Launch::NoWork);
    }
    let config = capture_config(root)?;
    let conn = attached(root)?;
    let work = !pending(&conn, &config, &BTreeSet::new())?.is_empty();
    drop(conn);
    if !work {
        return Ok(Launch::NoWork);
    }
    let Some(probe) = try_lock(root)? else {
        return Ok(Launch::AlreadyRunning);
    };
    drop(probe);
    checkpoint("probe_released")?;
    let mut child = Command::new(executable);
    child
        .arg(root)
        .arg("drain")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe, uses no allocation/locks in the post-fork child.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    match child.spawn() {
        Ok(child) => {
            let pid = child.id();
            // No wait: detached candidate owns its own try-lock. The host init reaps it.
            drop(child);
            checkpoint("spawned")?;
            Ok(Launch::Spawned(pid))
        }
        Err(error) => Ok(Launch::Failed(error.to_string())),
    }
}
pub fn command(
    root: &Path,
    reindex: bool,
    executable: &Path,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<Launch> {
    letopis_publication_checks::publish(root, reindex, checkpoint)?;
    launch_if_needed(root, executable, checkpoint)
}
pub fn run_drain(
    root: &Path,
    provider: &mut dyn FnMut(&str, &str) -> CheckResult<Vec<f32>>,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<bool> {
    let Some(lease) = try_lock(root)? else {
        checkpoint("drain_rejected")?;
        return Ok(false);
    };
    checkpoint("drain_acquired")?;
    drain(root, provider, checkpoint)?;
    drop(lease);
    checkpoint("drain_released")?;
    Ok(true)
}
