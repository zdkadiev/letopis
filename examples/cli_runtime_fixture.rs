//! Test-only executor fixture. It proves process mechanics, never product embedding.
use letopis::{
    open_filesystem, open_storage,
    service::{
        BackgroundRunner, CommandError, CommandErrorKind, CommandResult, EmbeddingExecutor,
        FileLocker, Refresher,
    },
    wire_background_runner, wire_document_refresher, wire_embedding_executor,
};
use serde_json::{Value, json};
use std::{
    env,
    error::Error,
    fs,
    io::{BufRead, BufReader, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

const CONTROL_ENV: &str = "LETOPIS_FIXTURE_CONTROL";
const SENTINEL_ENV: &str = "LETOPIS_FIXTURE_SENTINEL";
const TIMEOUT: Duration = Duration::from_secs(20);

struct FixtureTasks {
    control: Option<BufReader<UnixStream>>,
    root: PathBuf,
}
impl EmbeddingExecutor for FixtureTasks {
    fn is_work_available(&mut self) -> CommandResult<bool> {
        Ok(true)
    }
    fn drain(&mut self) -> CommandResult<()> {
        self.run_owned_drain().map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "fixture failed",
                std::io::Error::other(cause.to_string()),
            )
        })
    }
}
impl FixtureTasks {
    fn run_owned_drain(&mut self) -> Result<(), Box<dyn Error>> {
        let control = self.control.as_mut().ok_or("missing control")?;
        if get_command(control)? != "acquire" {
            return Err("missing acquire barrier".into());
        }
        let files = open_filesystem(&self.root)?;
        let lease = files.lock_try_acquire_by_path(Path::new(".letopis/embed.lock"))?;
        send_event(
            control.get_mut(),
            &json!({"event": "ownership", "owned": lease.is_some()}),
        )?;
        if lease.is_none() {
            return Ok(());
        }
        match get_command(control)?.as_str() {
            "release" => {}
            "crash" => {
                // SAFETY: raise targets this fixture's own process only, with a valid signal.
                unsafe {
                    libc::raise(libc::SIGKILL);
                }
                return Err("SIGKILL returned".into());
            }
            _ => return Err("missing release barrier".into()),
        }
        drop(lease);
        send_event(control.get_mut(), &json!({"event": "released"}))?;
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let root = env::current_dir()?;
    match args.as_slice() {
        [command] if command == "foreground" => run_foreground(&root),
        [command] if command == "product-foreground" => run_product_foreground(&root),
        [command, flag] if command == "embed" && flag == "--drain" => run_drain(&root),
        _ => Err("invalid fixture invocation".into()),
    }
}

fn get_command(control: &mut BufReader<UnixStream>) -> Result<String, Box<dyn Error>> {
    let mut line = String::new();
    if control.read_line(&mut line)? == 0 {
        return Err("observer disconnected".into());
    }
    Ok(line.trim().to_owned())
}
fn send_event(control: &mut UnixStream, value: &Value) -> Result<(), Box<dyn Error>> {
    serde_json::to_writer(&mut *control, value)?;
    control.write_all(b"\n")?;
    control.flush()?;
    Ok(())
}
fn get_descriptor(fd: i32) -> std::io::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::uninit();
    // SAFETY: fstat receives valid writable storage; an invalid fd returns an
    // error. The initialized result is read only after fstat succeeds.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized the complete stat value above.
    Ok(unsafe { stat.assume_init() })
}
fn inspect_descriptors(root: &Path) -> Result<(Vec<bool>, Vec<String>), Box<dyn Error>> {
    let null_file = fs::File::open("/dev/null")?;
    let null = get_descriptor(null_file.as_raw_fd())?;
    let standard = (0..3)
        .map(|fd| {
            get_descriptor(fd)
                .is_ok_and(|stat| stat.st_rdev == null.st_rdev && stat.st_mode == null.st_mode)
        })
        .collect();
    let service_files = fs::read_dir(root.join(".letopis"))?
        .map(|entry| {
            let entry = entry?;
            let file = fs::File::open(entry.path())?;
            Ok((entry.file_name(), get_descriptor(file.as_raw_fd())?))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let mut inherited = Vec::new();
    let fd_root = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    for entry in fs::read_dir(fd_root)? {
        let entry = entry?;
        let Ok(fd) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(stat) = get_descriptor(fd) else {
            continue;
        };
        for (name, file_stat) in &service_files {
            if stat.st_dev == file_stat.st_dev && stat.st_ino == file_stat.st_ino {
                inherited.push(name.to_string_lossy().into_owned());
            }
        }
    }
    Ok((standard, inherited))
}
fn run_drain(root: &Path) -> Result<(), Box<dyn Error>> {
    let socket = UnixStream::connect(env::var_os(CONTROL_ENV).ok_or("missing observer")?)?;
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.set_write_timeout(Some(TIMEOUT))?;
    let mut control = BufReader::new(socket);
    let (standard, inherited) = inspect_descriptors(root)?;
    // SAFETY: getsid(0) queries this process and has no pointer arguments.
    let sid = unsafe { libc::getsid(0) };
    send_event(
        control.get_mut(),
        &json!({"event": "started", "pid": std::process::id(), "sid": sid,
        "argv": env::args().skip(1).collect::<Vec<_>>(), "cwd": root,
        "sentinel": env::var(SENTINEL_ENV)?, "null_fds": standard, "inherited": inherited}),
    )?;
    let mut tasks = FixtureTasks {
        control: Some(control),
        root: root.to_path_buf(),
    };
    wire_background_runner(root, &env::current_exe()?, Some(&mut tasks))?.drain()?;
    Ok(())
}
fn run_foreground(root: &Path) -> Result<(), Box<dyn Error>> {
    // Hold real database connections and an index lease across production spawn.
    let _storage = open_storage(
        &root.join(".letopis/cache.db"),
        &root.join(".letopis/vectors.db"),
    )?;
    let files = open_filesystem(root)?;
    let _index = files.lock_acquire_by_path(Path::new(".letopis/index.lock"))?;
    let executable = env::var_os("LETOPIS_FIXTURE_EXECUTABLE")
        .map_or_else(env::current_exe, |value| Ok(PathBuf::from(value)))?;
    let mut tasks = FixtureTasks {
        control: None,
        root: root.to_path_buf(),
    };
    let warning = wire_background_runner(root, &executable, Some(&mut tasks))?.start_if_needed()?;
    // SAFETY: getsid(0) queries this process and has no pointer arguments.
    let sid = unsafe { libc::getsid(0) };
    println!(
        "{}",
        json!({"event": "foreground", "sid": sid, "warning": warning.map(|warning| json!({"message": warning.message, "details": warning.details}))})
    );
    // Test-only foreground barrier keeps the real database/lock descriptors
    // alive until the observer has inspected the exec child. It is unrelated
    // to the embedding barrier and does not wait for any drain work.
    let mut command = String::new();
    std::io::stdin().lock().read_line(&mut command)?;
    if command.trim() != "exit" {
        return Err("missing foreground exit barrier".into());
    }
    Ok(())
}

fn run_product_foreground(root: &Path) -> Result<(), Box<dyn Error>> {
    // This consumer refreshes through Documents; only the product can execute drain.
    let executable = PathBuf::from(
        env::var_os("LETOPIS_FIXTURE_EXECUTABLE").ok_or("missing product executable")?,
    );
    if executable == env::current_exe()? {
        return Err("product drain must not execute the fixture".into());
    }
    wire_document_refresher(root)?.refresh(&[])?;
    let mut tasks = wire_embedding_executor(root)?;
    let warning = wire_background_runner(root, &executable, Some(&mut tasks))?.start_if_needed()?;
    println!(
        "{}",
        json!({"event":"product-foreground", "warning":warning.map(|w| w.message)})
    );
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim() != "exit" {
        return Err("missing foreground exit barrier".into());
    }
    Ok(())
}
