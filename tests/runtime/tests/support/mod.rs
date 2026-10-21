#![allow(dead_code)]
#[path = "../../../embedding/tests/support/mod.rs"]
pub mod e;
#[path = "../../../publication/tests/support/mod.rs"]
pub mod p;
pub use letopis_embedding_checks::{CheckResult, attached, cache, capture_config, pending};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::Shutdown,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
const LIMIT: Duration = Duration::from_secs(20);
struct Peer {
    stream: UnixStream,
    bytes: Vec<u8>,
}
pub struct Control {
    listener: Option<UnixListener>,
    path: PathBuf,
    peers: Vec<Peer>,
}
pub struct Event {
    pub data: Value,
    stream: UnixStream,
    resumed: bool,
}
impl Event {
    pub fn wait_closed(&self) -> CheckResult<()> {
        let mut stream = self.stream.try_clone()?;
        let deadline = Instant::now() + LIMIT;
        loop {
            let mut b = [0; 1];
            match stream.read(&mut b) {
                Ok(0) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                other => return Err(format!("unexpected peer state: {other:?}").into()),
            }
            assert!(Instant::now() < deadline, "peer did not close");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn request_kill(mut self) -> CheckResult<()> {
        // A connected peer can signal only itself. PID metadata is never authority.
        self.stream.write_all(b"sigkill\n")?;
        self.resumed = true;
        Ok(())
    }
    pub fn pid(&self) -> u32 {
        self.data["pid"].as_u64().unwrap() as u32
    }
    pub fn name(&self) -> &str {
        self.data["name"].as_str().unwrap()
    }
    pub fn resume(mut self) -> CheckResult<()> {
        self.stream.write_all(b"continue\n")?;
        self.resumed = true;
        Ok(())
    }
    pub fn dead(mut self) {
        self.resumed = true;
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        if !self.resumed {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }
}
impl Control {
    pub fn new() -> CheckResult<Self> {
        // macOS sockaddr_un is short; don't place the socket in a long temp/worktree path.
        let file = tempfile::Builder::new()
            .prefix("tk4-")
            .tempfile_in("/tmp")?;
        let path = file.path().to_owned();
        drop(file);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener: Some(listener),
            path,
            peers: Vec::new(),
        })
    }
    pub fn next(&mut self) -> CheckResult<Event> {
        let deadline = Instant::now() + LIMIT;
        loop {
            while let Ok((stream, _)) = self.listener.as_ref().unwrap().accept() {
                stream.set_nonblocking(true)?;
                self.peers.push(Peer {
                    stream,
                    bytes: Vec::new(),
                });
            }
            let mut closed = Vec::new();
            for (i, peer) in self.peers.iter_mut().enumerate() {
                let mut buf = [0u8; 2048];
                match peer.stream.read(&mut buf) {
                    Ok(0) => {
                        closed.push(i);
                    }
                    Ok(n) => peer.bytes.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
                if let Some(end) = peer.bytes.iter().position(|b| *b == b'\n') {
                    let line: Vec<_> = peer.bytes.drain(..=end).collect();
                    let data = serde_json::from_slice(&line)?;
                    return Ok(Event {
                        data,
                        stream: peer.stream.try_clone()?,
                        resumed: false,
                    });
                }
            }
            for i in closed.into_iter().rev() {
                self.peers.remove(i);
            }
            if Instant::now() >= deadline {
                return Err("checkpoint timeout (20s)".into());
            }
            let mut fds = vec![libc::pollfd {
                fd: self.listener.as_ref().unwrap().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }];
            fds.extend(self.peers.iter().map(|p| libc::pollfd {
                fd: p.stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }));
            // SAFETY: poll receives exactly the initialized vector's length; descriptors remain owned.
            unsafe {
                libc::poll(fds.as_mut_ptr(), fds.len() as _, 20);
            }
        }
    }
    pub fn at(&mut self, name: &str) -> CheckResult<Event> {
        let event = self.next()?;
        assert_eq!(event.name(), name);
        Ok(event)
    }
    pub fn start(
        &self,
        root: &Path,
        mode: &str,
        cuts: &str,
        drain_cuts: &str,
    ) -> CheckResult<Process> {
        self.start_with(root, mode, cuts, drain_cuts, None, None)
    }
    pub fn start_with(
        &self,
        root: &Path,
        mode: &str,
        cuts: &str,
        drain_cuts: &str,
        executable: Option<&Path>,
        group: Option<u32>,
    ) -> CheckResult<Process> {
        let out = tempfile::tempfile()?;
        let err = tempfile::tempfile()?;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_runtime"));
        cmd.args([root.as_os_str(), mode.as_ref()])
            .env("TK4_CONTROL", &self.path)
            .env("TK4_COMMAND_CUTS", cuts)
            .env("TK4_DRAIN_CUTS", drain_cuts)
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(err.try_clone()?)
            .process_group(group.unwrap_or(0) as i32);
        if let Some(p) = executable {
            cmd.env("TK4_EXECUTABLE", p);
        }
        Ok(Process {
            child: cmd.spawn()?,
            out,
            err,
        })
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        // Closing the listener rejects not-yet-connected exec children. Existing peers
        // see EOF/abort at their next bounded hook; no PID from a file is ever signalled.
        self.listener.take();
        for peer in &self.peers {
            let _ = peer.stream.shutdown(Shutdown::Write);
        }
        let deadline = Instant::now() + LIMIT;
        while !self.peers.is_empty() && Instant::now() < deadline {
            self.peers.retain_mut(|p| {
                let mut b = [0; 4096];
                match p.stream.read(&mut b) {
                    Ok(0) => false,
                    Err(e) if e.kind() != std::io::ErrorKind::WouldBlock => false,
                    _ => true,
                }
            });
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = fs::remove_file(&self.path);
    }
}
pub struct Process {
    child: Child,
    out: File,
    err: File,
}
impl Process {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn alive(&mut self) -> CheckResult<bool> {
        Ok(self.child.try_wait()?.is_none())
    }
    pub fn signal(&mut self, signal: i32) -> CheckResult<()> {
        assert!(self.alive()?, "owned target already exited");
        // SAFETY: direct, unreaped Child remains ours; never signal a foreign/lock-file PID.
        if unsafe { libc::kill(self.pid() as i32, signal) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn wait(&mut self) -> CheckResult<ExitStatus> {
        let deadline = Instant::now() + LIMIT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            assert!(Instant::now() < deadline, "owned child exit timeout");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn killed(&mut self, signal: i32) -> CheckResult<()> {
        self.signal(signal)?;
        assert_eq!(self.wait()?.signal(), Some(signal));
        Ok(())
    }
    pub fn finish(&mut self) -> CheckResult<String> {
        let status = self.wait()?;
        let out = self.output()?;
        assert!(status.success(), "{status}: {out}");
        Ok(out)
    }
    pub fn output(&mut self) -> CheckResult<String> {
        use std::io::{Seek, SeekFrom};
        self.out.seek(SeekFrom::Start(0))?;
        self.err.seek(SeekFrom::Start(0))?;
        let mut s = String::new();
        self.out.read_to_string(&mut s)?;
        self.err.read_to_string(&mut s)?;
        Ok(s)
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub fn held(root: &Path, name: &str) -> CheckResult<bool> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join(".letopis").join(name))?;
    // SAFETY: independent open-file description, closed immediately after this observation.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(false);
    }
    let e = std::io::Error::last_os_error();
    if e.kind() == std::io::ErrorKind::WouldBlock {
        Ok(true)
    } else {
        Err(e.into())
    }
}
pub fn wait_free(root: &Path, name: &str) -> CheckResult<()> {
    let deadline = Instant::now() + LIMIT;
    while held(root, name)? {
        assert!(Instant::now() < deadline, "lock not released");
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
pub fn kill_detached(event: Event, root: &Path) -> CheckResult<()> {
    assert_eq!(event.name(), "drain_acquired");
    event.request_kill()?;
    wait_free(root, "embed.lock")
}
pub fn inode(root: &Path, name: &str) -> CheckResult<u64> {
    Ok(fs::metadata(root.join(".letopis").join(name))?.ino())
}
pub fn assert_modes(root: &Path, name: &str) -> CheckResult<()> {
    assert_eq!(fs::metadata(root.join(".letopis"))?.mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(root.join(".letopis").join(name))?.mode() & 0o777,
        0o600
    );
    Ok(())
}
pub fn assert_vectors(root: &Path) -> CheckResult<()> {
    let c = attached(root)?;
    let stored = e::stored(&c)?;
    assert_eq!(
        stored,
        std::collections::BTreeMap::from([(e::hash("alpha"), e::blob(&[1., 0., 0.]))])
    );
    assert!(pending(&c, &capture_config(root)?, &BTreeSet::new())?.is_empty());
    Ok(())
}
pub fn expected_changed(root: &Path, revision: i64) -> CheckResult<()> {
    let c = cache(root)?;
    let a = fs::read(root.join("a.md"))?;
    let b = fs::read(root.join("b.md"))?;
    p::assert_nodes(
        &c,
        root,
        &[("a.md", &a, "bravo"), ("b.md", &b, "beta")],
        revision,
    )?;
    assert_eq!(
        p::rows(&c, "SELECT * FROM edges ORDER BY source_id,target_id,type")?,
        serde_json::json!([["a.md", "b.md", "includes"]])
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_array().unwrap().clone())
            .collect::<Vec<_>>()
    );
    Ok(())
}
pub fn change(root: &Path) -> CheckResult<()> {
    p::fixture(root, "a.md", "bravo", &[("b.md", "includes")])
}

// Independent OS observation: the fixture's waiting event is BEFORE blocking flock.
// The process must be observed in the actual syscall before cancellation is sent.
impl Process {
    #[cfg(target_os = "macos")]
    pub fn observe_flock(&mut self, _root: &Path) -> CheckResult<(bool, String)> {
        assert!(self.alive()?);
        let output = tempfile::NamedTempFile::new()?;
        let out = tempfile::tempfile()?;
        let err = tempfile::tempfile()?;
        let child = Command::new("/usr/bin/sample")
            .arg(self.pid().to_string())
            .args(["1", "1", "-file"])
            .arg(output.path())
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(err.try_clone()?)
            .spawn()?;
        let mut sampler = Self { child, out, err };
        sampler.finish()?;
        let trace = fs::read_to_string(output.path())?;
        // Match a counted native call-stack frame, not a symbol merely listed in
        // binary images or the fixture's pre-syscall checkpoint/function names.
        let inside = trace.lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.next().is_some_and(|s| s.parse::<u64>().is_ok())
                && fields.next() == Some("flock")
                && line.contains("(in libsystem_kernel.dylib)")
        });
        Ok((inside, trace))
    }
    #[cfg(target_os = "linux")]
    pub fn observe_flock(&mut self, root: &Path) -> CheckResult<(bool, String)> {
        assert!(self.alive()?);
        let proc = PathBuf::from(format!("/proc/{}", self.pid()));
        let syscall = fs::read_to_string(proc.join("syscall"))?;
        let wchan = fs::read_to_string(proc.join("wchan"))?;
        let fields: Vec<_> = syscall.split_whitespace().collect();
        let number = fields.first().and_then(|s| s.parse::<libc::c_long>().ok());
        let argument = |index: usize| {
            fields
                .get(index)
                .and_then(|s| s.strip_prefix("0x"))
                .and_then(|s| u64::from_str_radix(s, 16).ok())
        };
        let mut same_file = false;
        let mut fd_path = None;
        if number == Some(libc::SYS_flock)
            && argument(2) == Some(libc::LOCK_EX as u64)
            && let Some(fd) = argument(1)
        {
            let path = proc.join("fd").join(fd.to_string());
            let actual = fs::metadata(&path)?;
            let expected = fs::metadata(root.join(".letopis/index.lock"))?;
            same_file = actual.ino() == expected.ino() && actual.dev() == expected.dev();
            fd_path = Some(fs::read_link(path)?);
        }
        Ok((same_file, serde_json::json!({"pid":self.pid(),"syscall":syscall,"wchan":wchan,"index_fd":fd_path,"same_lock_inode":same_file}).to_string()))
    }
    pub fn wait_in_flock(&mut self, root: &Path) -> CheckResult<String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (inside, trace) = self.observe_flock(root)?;
            if inside {
                return Ok(trace);
            }
            assert!(
                Instant::now() < deadline,
                "waiter never entered blocking flock: {trace}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
pub fn retain_flock_observation(label: &str, trace: &str) -> CheckResult<()> {
    if let Some(dir) = std::env::var_os("TK4_FLOCK_EVIDENCE") {
        fs::create_dir_all(&dir)?;
        let mut file = tempfile::Builder::new()
            .prefix(label)
            .suffix(".txt")
            .tempfile_in(dir)?;
        file.write_all(trace.as_bytes())?;
        let (_, path) = file.keep()?;
        println!("flock-observation:{label}:{}", path.display());
    }
    Ok(())
}
