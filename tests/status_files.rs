use letopis::open_filesystem;
use letopis::service::{FileErrorKind, FileReader};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CHILD_ROOT: &str = "LETOPIS_STATUS_FILES_ROOT";
const CHILD_LOCK: &str = "LETOPIS_STATUS_FILES_LOCK";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "letopis-status-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn get_lock_path(&self, name: &str) -> PathBuf {
        self.0.join(".letopis").join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct Owner(Child);
impl Owner {
    fn spawn(root: &Path, name: &str, pid: &str) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "status_files_child", "--nocapture"])
            .env(CHILD_ROOT, root)
            .env(CHILD_LOCK, name)
            .env("LETOPIS_STATUS_FILES_PID", pid)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let ready = root.join(format!("{name}.ready"));
        let start = Instant::now();
        while !ready.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "owner did not start"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        Self(child)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}

#[test]
fn reports_busy_recorded_pid() {
    // Given: an independent process holds the index lock with its PID.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let child = Owner::spawn(&fixture.0, "index.lock", "self");
    let lock_path = fixture.get_lock_path("index.lock");
    let original_bytes = fs::read(&lock_path).unwrap();
    let original_inode = fs::metadata(&lock_path).unwrap().ino();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    let start = Instant::now();
    let locks = files
        .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
        .unwrap();

    // When / Then: the generic observer reads the held owner; the exact PID returns without changing inode or bytes.
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(locks.unwrap().get(), child.0.id());
    assert_eq!(std::fs::read(&lock_path).unwrap(), original_bytes);
    assert_eq!(fs::metadata(&lock_path).unwrap().ino(), original_inode);
}

#[test]
fn ignores_stale_unlocked_pid() {
    // Given: an unlocked file contains stale PID bytes.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let lock_path = fixture.get_lock_path("index.lock");
    fs::write(&lock_path, b"999999\n").unwrap();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    let locks = files
        .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
        .unwrap();

    // When / Then: lock ownership is observed; no active owner is reported and stale bytes remain unchanged.
    assert!(locks.is_none());
    assert_eq!(fs::read(&lock_path).unwrap(), b"999999\n");
}

#[test]
fn does_not_write_probe_pid() {
    // Given: an unlocked embed lock with known bytes and inode.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let lock_path = fixture.get_lock_path("embed.lock");
    fs::write(&lock_path, b"42\n").unwrap();
    let inode = fs::metadata(&lock_path).unwrap().ino();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    // When / Then: the observer probes kernel ownership; observation changes neither bytes nor inode.
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );

    assert_eq!(fs::read(&lock_path).unwrap(), b"42\n");
    assert_eq!(fs::metadata(&lock_path).unwrap().ino(), inode);
}

#[test]
fn rejects_busy_invalid_pid() {
    // Given: a held lock contains malformed owner metadata.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let _child = Owner::spawn(&fixture.0, "embed.lock", "invalid");
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    // When / Then: the owner observer reads the bounded record; the malformed held record is rejected.
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/embed.lock"))
            .is_err()
    );
}

#[test]
fn rejects_busy_pid_with_valid_prefix_and_trailing_bytes() {
    // Given: a held record contains a valid PID prefix followed by excess bytes.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let _child = Owner::spawn(&fixture.0, "embed.lock", "overlong");
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    // When: the owner observer reads its bounded record.
    let error = files
        .lock_get_owner_by_path(Path::new(".letopis/embed.lock"))
        .unwrap_err();

    // Then: InvalidOwner is returned instead of accepting the prefix.
    assert_eq!(error.get_kind(), FileErrorKind::InvalidOwner);
}

#[test]
fn returns_missing_locks_without_creating_files() {
    // Given: a checkout without any service directory.
    let fixture = Fixture::new();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    // When / Then: missing lock and configuration paths are observed; no owner or bytes are returned and no directory is created.
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .unwrap(),
        None
    );
    assert!(!fixture.0.join(".letopis").exists());
}

#[test]
fn rejects_symlink_and_inode_replacement() {
    // Given: a pinned directory and a lock later replaced by a symlink.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();
    // When / Then: the observer revisits the lock and replaced parent; unsafe or changed paths remain errors.
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
            .unwrap()
            .is_none()
    );
    symlink("/dev/null", fixture.get_lock_path("index.lock")).unwrap();
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
            .is_err()
    );
    // Given: the pinned service directory is replaced by a fresh directory.
    fs::remove_file(fixture.get_lock_path("index.lock")).unwrap();
    fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    // When / Then: the observer revisits the lock and replaced parent; unsafe or changed paths remain errors.
    assert!(
        files
            .lock_get_owner_by_path(Path::new(".letopis/index.lock"))
            .is_err()
    );
    assert!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .is_err()
    );
}

#[test]
fn reads_existing_config_without_mutation() {
    // Given: a regular configuration file containing arbitrary bytes.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    fs::write(fixture.0.join(".letopis/config.json"), b"raw\xff").unwrap();
    let files = open_filesystem(&fixture.0).unwrap();
    files.directory_get_by_path(Path::new(".letopis")).unwrap();

    // When / Then: the generic file reader returns the exact bytes without mutation.
    assert_eq!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .unwrap(),
        Some(b"raw\xff".to_vec())
    );
}

#[test]
fn status_files_child() {
    // Given
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let name = std::env::var(CHILD_LOCK).unwrap();
    let path = Path::new(&root).join(".letopis").join(&name);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.lock().unwrap();
    let pid = match std::env::var("LETOPIS_STATUS_FILES_PID").unwrap().as_str() {
        "self" => std::process::id().to_string(),
        "invalid" => "bad pid".to_owned(),
        "overlong" => "000000000001garbage".to_owned(),
        other => panic!("unexpected pid case: {other}"),
    };
    // When
    writeln!(file, "{pid}").unwrap();
    fs::write(Path::new(&root).join(format!("{name}.ready")), b"ready").unwrap();
    // Then: keep the lock held until the parent terminates the child
    std::thread::sleep(Duration::from_secs(30));
}
