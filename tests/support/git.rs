#![allow(dead_code)]
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
#[derive(Clone, Copy, Debug)]
pub enum ObjectFormat {
    Sha1,
    Sha256,
}
pub const FORMATS: [ObjectFormat; 2] = [ObjectFormat::Sha1, ObjectFormat::Sha256];
pub struct TempRepo {
    root: PathBuf,
    subdir: PathBuf,
}
#[derive(Debug, PartialEq, Eq)]
pub struct FixtureState {
    head: Vec<u8>,
    index: Vec<u8>,
    config: Vec<u8>,
    refs: Vec<u8>,
    live: Vec<(String, Vec<u8>)>,
}
pub fn isolated(command: &mut Command) {
    for (k, _) in env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            command.env_remove(k);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
}
impl TempRepo {
    pub fn new(format: ObjectFormat) -> Self {
        let root = env::temp_dir().join(format!(
            "letopis-git-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let subdir = root.join("subdir");
        let repo = Self { root, subdir };
        repo.ok(
            &[
                "init",
                "-q",
                match format {
                    ObjectFormat::Sha1 => "--object-format=sha1",
                    ObjectFormat::Sha256 => "--object-format=sha256",
                },
            ],
            None,
        );
        repo.ok(&["config", "user.name", "Fixture"], None);
        repo.ok(&["config", "user.email", "fixture@example.invalid"], None);
        fs::create_dir(repo.subdir()).unwrap();
        fs::write(repo.root.join("live.md"), b"original live\n").unwrap();
        repo.ok(&["add", "live.md"], None);
        repo.ok(&["commit", "-qm", "fixture"], None);
        fs::write(repo.root.join("live.md"), b"staged live\n").unwrap();
        repo.ok(&["add", "live.md"], None);
        fs::write(repo.root.join("live.md"), b"unstaged live\n").unwrap();
        repo
    }
    pub fn shared_clone(&self) -> Self {
        let root = self.root.join("BORROWER_PATH_CANARY");
        self.ok(
            &[
                "clone",
                "-q",
                "--shared",
                self.root.to_str().unwrap(),
                root.to_str().unwrap(),
            ],
            None,
        );
        let subdir = root.join("subdir");
        fs::create_dir(&subdir).unwrap();
        Self { root, subdir }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn subdir(&self) -> &Path {
        &self.subdir
    }
    pub fn git(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut c = Command::new("git");
        isolated(&mut c);
        c.current_dir(&self.root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        thread::scope(|s| {
            let writer = s.spawn(move || {
                if let Some(bytes) = input {
                    stdin.write_all(bytes).unwrap();
                }
            });
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            output
        })
    }
    pub fn ok(&self, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
        let out = self.git(args, input);
        assert!(
            out.status.success(),
            "fixture Git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }
    pub fn oid(&self, args: &[&str], input: Option<&[u8]>) -> String {
        String::from_utf8(self.ok(args, input))
            .unwrap()
            .trim()
            .to_owned()
    }
    pub fn head_index_config_and_live_snapshot(&self) -> FixtureState {
        FixtureState {
            head: fs::read(self.root.join(".git/HEAD")).unwrap(),
            index: fs::read(self.root.join(".git/index")).unwrap(),
            config: fs::read(self.root.join(".git/config")).unwrap(),
            refs: self
                .ok(&["for-each-ref", "--format=%(refname) %(objectname)"], None)
                .split_inclusive(|b| *b == b'\n')
                .filter(|line| !line.starts_with(b"refs/letopis/snapshots/"))
                .flatten()
                .copied()
                .collect(),
            live: ["live.md", ".gitattributes"]
                .into_iter()
                .filter_map(|p| fs::read(self.root.join(p)).ok().map(|b| (p.to_owned(), b)))
                .collect(),
        }
    }
    pub fn change_and_delete_live_files(&self, path: &str) {
        fs::write(self.root.join("live.md"), b"changed").unwrap();
        if self.root.join(path).is_file() {
            fs::remove_file(self.root.join(path)).unwrap();
        }
    }
    pub fn direct_ref(&self, oid: &str) -> String {
        self.oid(
            &["rev-parse", &format!("refs/letopis/snapshots/{oid}")],
            None,
        )
    }
    pub fn expire_reflogs_gc_prune_now_and_pack_refs(&self) {
        self.ok(&["reflog", "expire", "--expire=now", "--all"], None);
        self.ok(&["gc", "--prune=now"], None);
        self.ok(&["prune", "--expire=now"], None);
        self.ok(&["pack-refs", "--all"], None);
    }
    pub fn fsck_strict_no_reflogs(&self) -> ExitStatus {
        self.git(&["fsck", "--strict", "--no-reflogs"], None).status
    }
    pub fn script(&self, name: &str, text: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.root.join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
impl Drop for TempRepo {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

// Guards only test-owned children, including unwinding after a sibling failure.
pub struct OwnedChild(Option<Child>);
impl OwnedChild {
    pub fn spawn(command: &mut Command) -> Self {
        Self(Some(command.spawn().unwrap()))
    }
    pub fn wait(mut self, deadline: Instant) -> Output {
        let child = self.0.as_mut().unwrap();
        loop {
            if child.try_wait().unwrap().is_some() {
                return self.0.take().unwrap().wait_with_output().unwrap();
            }
            assert!(Instant::now() < deadline, "owned fixture exceeded deadline");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
pub fn supervised_test(name: &str, envs: &[(&str, &str)]) {
    let mut c = Command::new(env::current_exe().unwrap());
    c.args(["--exact", name, "--nocapture"])
        .envs(envs.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = OwnedChild::spawn(&mut c).wait(Instant::now() + Duration::from_secs(30));
    assert!(
        out.status.success(),
        "supervised test failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
