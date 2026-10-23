use letopis::domain::RepoPath;
use letopis::markdown::{MarkdownParser, parse};
use letopis::open_embedding_files;
use letopis::service::{EmbeddingFiles, MarkdownCodec, ParsedFile};
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
    fn read_capture(&mut self) -> String {
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
                assert!(status.success(), "child failed: {}", self.read_capture());
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
    // Arrange
    let fixture = Fixture::new();
    let files = open_embedding_files(&fixture.0).unwrap();
    let path = RepoPath::parse("notes/a.md").unwrap();
    assert_eq!(files.read_config().unwrap(), None);
    assert_eq!(files.read_document(&path).unwrap(), None);
    fs::create_dir(fixture.0.join("notes")).unwrap();
    fs::write(fixture.0.join("notes/a.md"), b"\xff\r\ntext").unwrap();
    fs::write(fixture.0.join(".letopis/config.json"), b"not JSON\xff").unwrap();
    // Act / Assert
    assert_eq!(
        files.read_document(&path).unwrap().unwrap(),
        b"\xff\r\ntext"
    );
    assert_eq!(files.read_config().unwrap().unwrap(), b"not JSON\xff");
    assert!(files.get_time_secs().unwrap() > 0);
    files.wait(Duration::ZERO).unwrap();
}

#[test]
fn rejects_final_and_ancestor_symlinks_and_nonregular_files() {
    // Arrange
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::write(outside.0.join("secret.md"), b"PRIVATE_PAYLOAD").unwrap();
    symlink(outside.0.join("secret.md"), fixture.0.join("link.md")).unwrap();
    symlink(&outside.0, fixture.0.join("escape")).unwrap();
    fs::create_dir(fixture.0.join("directory.md")).unwrap();
    let files = open_embedding_files(&fixture.0).unwrap();
    // Act / Assert
    for path in ["link.md", "escape/secret.md", "directory.md"] {
        let error = files
            .read_document(&RepoPath::parse(path).unwrap())
            .unwrap_err();
        assert!(!format!("{error} {error:?}").contains("PRIVATE_PAYLOAD"));
    }
    symlink(
        outside.0.join("secret.md"),
        fixture.0.join(".letopis/config.json"),
    )
    .unwrap();
    assert!(files.read_config().is_err());
}

#[test]
fn rejects_replaced_service_directory_and_symlink_at_open() {
    // Arrange
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let files = open_embedding_files(&fixture.0).unwrap();
    fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
    symlink(&outside.0, fixture.0.join(".letopis")).unwrap();
    // Act / Assert
    assert!(files.read_config().is_err());
    assert!(files.try_lock_embed().is_err());
    assert!(open_embedding_files(&fixture.0).is_err());
}

#[test]
fn persistent_lock_excludes_real_process_and_survives_owner_death() {
    // Arrange
    let fixture = Fixture::new();
    let mut owner = OwnedChild::spawn(&fixture.0, "owner");
    wait_for(&fixture.0.join("ready"));
    let lock_path = fixture.0.join(".letopis/embed.lock");
    let before = fs::metadata(&lock_path).unwrap();
    let files = open_embedding_files(&fixture.0).unwrap();
    // Act / Assert: a real other process owns this inode.
    assert!(files.try_lock_embed().unwrap().is_none());
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
    let guard = files.try_lock_embed().unwrap().unwrap();
    let after = fs::metadata(&lock_path).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    drop(guard);
    assert!(lock_path.exists());
}

#[test]
fn fifo_reads_and_lock_attempts_finish_promptly() {
    // Arrange
    let fixture = Fixture::new();
    let _files = open_embedding_files(&fixture.0).unwrap();
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
    // Act / Assert: the parent kills and reaps a stuck child on timeout.
    OwnedChild::spawn(&fixture.0, "fifo").wait_success();
}

#[test]
fn markdown_capability_preserves_existing_parser_projection() {
    let path = RepoPath::parse("knowledge/note.md").unwrap();
    for bytes in [
        b"<!-- letopis {\"kind\":\"fact\",\"created_at\":1} -->\nHello.\n".as_slice(),
        b"# Heading {#fact}\n<!-- letopis {\"kind\":\"fact\",\"created_at\":1} -->\nHello.\n"
            .as_slice(),
        b"# Unmarked\nhello\n".as_slice(),
    ] {
        // Act
        let old = parse(&path, bytes).unwrap();
        let new = MarkdownParser.parse_file(&path, bytes).unwrap();
        // Assert
        assert_parsed_equal(old, new);
    }
}

#[test]
fn embedding_files_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = Path::new(&root);
    let files = open_embedding_files(root).unwrap();
    match std::env::var(CHILD_CASE).unwrap().as_str() {
        "owner" => {
            let _guard = files.try_lock_embed().unwrap().unwrap();
            fs::write(root.join("ready"), b"ready").unwrap();
            wait_for(&root.join("release"));
        }
        "fifo" => {
            assert!(
                files
                    .read_document(&RepoPath::parse("fifo.md").unwrap())
                    .is_err()
            );
            assert!(files.try_lock_embed().is_err());
        }
        _ => panic!("unknown child case"),
    }
}

#[test]
fn protects_existing_service_paths_and_rejects_lock_symlinks() {
    // Arrange
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
    // Act
    let files = open_embedding_files(&fixture.0).unwrap();
    let owner = files.try_lock_embed().unwrap().unwrap();
    // Assert
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
    assert!(files.try_lock_embed().is_err());
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
