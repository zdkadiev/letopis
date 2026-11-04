use letopis::domain::RepoPath;
use letopis::markdown::{MarkdownParser, file_parse};
use letopis::open_filesystem;
use letopis::service::{FileLocker, FileReader, FileWriter, MarkdownCodec, ParsedFile};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CHILD_ROOT: &str = "LETOPIS_EMBED_FILES_TEST_ROOT";
const CHILD_CASE: &str = "LETOPIS_EMBED_FILES_TEST_CASE";
const DEADLINE: Duration = Duration::from_secs(10);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "letopis-embedding-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct OwnedChild(Child, fs::File);
impl OwnedChild {
    fn spawn(root: &Path, case: &str) -> Self {
        let output = tempfile::tempfile().unwrap();
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "embedding_files_child", "--nocapture"])
                .env(CHILD_ROOT, root)
                .env(CHILD_CASE, case)
                .stdout(Stdio::from(output.try_clone().unwrap()))
                .stderr(Stdio::from(output.try_clone().unwrap()))
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
            output,
        )
    }
    fn get_capture(&mut self) -> String {
        self.1.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        (&mut self.1)
            .take(64 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    fn wait_success(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "child failed: {}", self.get_capture());
                return;
            }
            assert!(start.elapsed() < DEADLINE, "child exceeded deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}
#[test]
fn reads_exact_bytes_and_stable_absence_without_decoding_config() {
    // Given: missing paths followed by files containing arbitrary bytes.
    let fixture = Fixture::new();
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    let path = RepoPath::parse("notes/a.md").unwrap();
    // When / Then: reads of both missing paths return absence.
    assert_eq!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .unwrap(),
        None
    );
    assert_eq!(
        files
            .file_get_bytes_by_path(Path::new(path.as_str()))
            .unwrap(),
        None
    );
    // Given: both paths now contain arbitrary bytes.
    fs::create_dir(fixture.0.join("notes")).unwrap();
    fs::write(fixture.0.join("notes/a.md"), b"\xff\r\ntext").unwrap();
    fs::write(fixture.0.join(".letopis/config.json"), b"not JSON\xff").unwrap();
    // When / Then: the generic reader returns the selected bytes without decoding them.
    assert_eq!(
        files
            .file_get_bytes_by_path(Path::new(path.as_str()))
            .unwrap()
            .unwrap(),
        b"\xff\r\ntext"
    );
    assert_eq!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .unwrap()
            .unwrap(),
        b"not JSON\xff"
    );
}

#[test]
fn rejects_final_and_ancestor_symlinks_and_nonregular_files() {
    // Given: nonregular leaves and a parent symlink to outside data.
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::write(outside.0.join("secret.md"), b"PRIVATE_PAYLOAD").unwrap();
    symlink(outside.0.join("secret.md"), fixture.0.join("link.md")).unwrap();
    symlink(&outside.0, fixture.0.join("escape")).unwrap();
    fs::create_dir(fixture.0.join("directory.md")).unwrap();
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    // When / Then: generic reads inspect the unsafe paths; typed failures expose no outside payload.
    for path in ["link.md", "escape/secret.md", "directory.md"] {
        let error = files.file_get_bytes_by_path(Path::new(path)).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("PRIVATE_PAYLOAD"));
    }
    // Given: the configuration leaf also becomes an outside symlink.
    symlink(
        outside.0.join("secret.md"),
        fixture.0.join(".letopis/config.json"),
    )
    .unwrap();
    // When / Then: the byte read rejects that symlink.
    assert!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .is_err()
    );
}

#[test]
fn rejects_replaced_service_directory_and_symlink_at_open() {
    // Given: a pinned directory replaced by an outside symlink.
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
    symlink(&outside.0, fixture.0.join(".letopis")).unwrap();
    // When / Then: reads and lock acquisition revisit the pinned path; both the old pin and a fresh directory request reject the unsafe path.
    assert!(
        files
            .file_get_bytes_by_path(std::path::Path::new(".letopis/config.json"))
            .is_err()
    );
    assert!(
        files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .is_err()
    );
    assert!(
        open_filesystem(&fixture.0)
            .unwrap()
            .directory_create_by_path(Path::new(".letopis"))
            .is_err()
    );
}

#[test]
fn persistent_lock_excludes_real_process_and_survives_owner_death() {
    // Given: an independent process holds a private persistent lock.
    let fixture = Fixture::new();
    let mut owner = OwnedChild::spawn(&fixture.0, "owner");
    wait_for(&fixture.0.join("ready"));
    let lock_path = fixture.0.join(".letopis/embed.lock");
    let before = fs::metadata(&lock_path).unwrap();
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    // When / Then: a real other process owns this inode.
    assert!(
        files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    let busy = fs::metadata(&lock_path).unwrap();
    assert_eq!(
        (before.ctime(), before.ctime_nsec()),
        (busy.ctime(), busy.ctime_nsec())
    );
    assert_eq!(
        fs::read_to_string(&lock_path).unwrap(),
        format!("{}\n", owner.0.id())
    );
    assert_eq!(before.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(fixture.0.join(".letopis")).unwrap().mode() & 0o777,
        0o700
    );
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    let guard = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let after = fs::metadata(&lock_path).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    drop(guard);
    assert!(lock_path.exists());
}

#[test]
fn fifo_reads_and_lock_attempts_finish_promptly() {
    // Given: FIFO leaves in ordinary and lock paths.
    let fixture = Fixture::new();
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(fixture.0.join("fifo.md"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("mkfifo")
            .arg(fixture.0.join(".letopis/embed.lock"))
            .status()
            .unwrap()
            .success()
    );
    // When / Then: the parent kills and reaps a stuck child on timeout.
    OwnedChild::spawn(&fixture.0, "fifo").wait_success();
}

#[test]
fn markdown_capability_preserves_existing_parser_projection() {
    // Given: marked-looking legacy bytes and unmarked Markdown exercise both parser entry points.
    let path = RepoPath::parse("knowledge/note.md").unwrap();
    for bytes in [
        b"<!-- letopis {\"kind\":\"fact\",\"created_at\":1} -->\nHello.\n".as_slice(),
        b"# Heading {#fact}\n<!-- letopis {\"kind\":\"fact\",\"created_at\":1} -->\nHello.\n"
            .as_slice(),
        b"# Unmarked\nhello\n".as_slice(),
    ] {
        // When: parse the same path and bytes through both entry points.
        let old = file_parse(&path, bytes).unwrap();
        let new = MarkdownParser.file_parse(&path, bytes).unwrap();
        // Then: the complete projections remain equal.
        assert_parsed_equal(old, new);
    }
}

#[test]
fn embedding_files_child() {
    // Given: a child invocation with a specified file or lock scenario.
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = Path::new(&root);
    // When: the child runs the requested generic file operation.
    let files = open_filesystem(root).unwrap();
    match std::env::var(CHILD_CASE).unwrap().as_str() {
        "owner" => {
            let _guard = files
                .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                .unwrap()
                .unwrap();
            fs::write(root.join("ready"), b"ready").unwrap();
            wait_for(&root.join("release"));
        }
        "fifo" => {
            // When / Then: FIFO reads and lock attempts fail without blocking.
            assert!(files.file_get_bytes_by_path(Path::new("fifo.md")).is_err());
            assert!(
                files
                    .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                    .is_err()
            );
        }
        _ => panic!("unknown child case"),
    }
}

#[test]
fn protects_existing_service_paths_and_rejects_lock_symlinks() {
    // Given: existing service paths and a lock symlink.
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();
    fs::set_permissions(
        fixture.0.join(".letopis"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::write(fixture.0.join(".letopis/embed.lock"), b"stale pid").unwrap();
    fs::set_permissions(
        fixture.0.join(".letopis/embed.lock"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    // When: generic directory creation and lock acquisition run.
    let files = open_filesystem(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    let owner = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    // Then: private modes are applied and unsafe lock links are rejected.
    assert_eq!(
        fs::metadata(fixture.0.join(".letopis")).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(fixture.0.join(".letopis/embed.lock"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    drop(owner);
    fs::remove_file(fixture.0.join(".letopis/embed.lock")).unwrap();
    symlink("/dev/null", fixture.0.join(".letopis/embed.lock")).unwrap();
    assert!(
        files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .is_err()
    );
}

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < DEADLINE, "barrier exceeded deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_parsed_equal(old: Option<ParsedFile>, new: Option<ParsedFile>) {
    match (old, new) {
        (None, None) => {}
        (Some(old), Some(new)) => {
            assert_eq!(old.edges, new.edges);
            assert_eq!(old.elements.len(), new.elements.len());
            for (old, new) in old.elements.iter().zip(new.elements.iter()) {
                assert_eq!(old.element, new.element);
                assert_eq!(old.search_text, new.search_text);
                assert_eq!(old.search_hash, new.search_hash);
            }
        }
        _ => panic!("parser marking differs"),
    }
}
