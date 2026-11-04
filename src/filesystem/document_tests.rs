use super::{LocalFileSystem, get_file_stat_by_descriptor, reopen_concurrently_created_lock};
use crate::{
    domain::RepoPath,
    service::{DocumentErrorKind, EmbedLocker, FileSystem},
};
use rustix::{
    fs::{FlockOperation, flock, fstat},
    io::Errno,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Barrier, mpsc},
    thread,
    time::Duration,
};
use tempfile::tempdir;

#[test]
fn get_regular_file_preserves_bytes_and_stat() {
    // Given: a regular document with non-UTF-8 bytes and a Unicode path.
    let root = tempdir().unwrap();
    let path = RepoPath::parse("заметка with spaces.md").unwrap();
    fs::write(root.path().join(path.as_str()), b"literal\0\xffbytes").unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let before = filesystem.get_time_ns().unwrap();
    // When: the document and its file metadata are retrieved.
    let captured = filesystem.get_file(&path).unwrap().unwrap();
    let stat = filesystem.get_file_stat_by_path(&path).unwrap().unwrap();
    // Then: bytes, metadata, and capture time match the file.
    assert_eq!(captured.bytes, b"literal\0\xffbytes");
    assert_eq!(captured.stat, stat);
    assert_eq!(stat.size, 14);
    assert_eq!(
        stat.inode,
        fs::metadata(root.path().join(path.as_str())).unwrap().ino()
    );
    assert!(captured.checked_at_ns >= before);
    assert!(captured.checked_at_ns <= filesystem.get_time_ns().unwrap());
}

#[test]
fn nonregular_final_entries_are_absent_without_following_symlinks() {
    // Given: a symlink to an external file, a directory, and a FIFO.
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("note.md"), b"secret").unwrap();
    symlink(outside.path().join("note.md"), root.path().join("link.md")).unwrap();
    fs::create_dir(root.path().join("directory.md")).unwrap();
    create_fifo(&root.path().join("pipe.md"));
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    for raw in ["link.md", "directory.md", "pipe.md"] {
        let path = RepoPath::parse(raw).unwrap();
        // When / Then: observation and capture treat the entry as absent.
        assert!(filesystem.get_file_stat_by_path(&path).unwrap().is_none());
        assert!(filesystem.get_file(&path).unwrap().is_none());
    }
}

#[test]
fn nonregular_entry_changes_between_observations_fail() {
    for initial in ["missing", "symlink"] {
        // Given: a missing path or symlink that changes into a FIFO after observation.
        let root = tempdir().unwrap();
        let path = root.path().join("note.md");
        if initial == "symlink" {
            symlink("target", &path).unwrap();
        }
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        filesystem.between_stats.replace(Some(Box::new(move || {
            if initial == "symlink" {
                fs::remove_file(&path).unwrap();
            }
            create_fifo(&path);
        })));
        // When / Then: the changed entry is rejected instead of treated as stable absence.
        assert!(
            filesystem
                .get_file_stat_by_path(&RepoPath::parse("note.md").unwrap())
                .is_err()
        );
    }
}

#[test]
fn reject_final_and_parent_symlinks() {
    // Given: a symlink in a parent and a regular file replaced before opening.
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("note.md"), b"secret").unwrap();
    symlink(outside.path(), root.path().join("parent")).unwrap();
    let path = RepoPath::parse("parent/note.md").unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    // When / Then: traversal through the parent symlink fails.
    assert!(filesystem.get_file(&path).is_err());
    assert!(filesystem.get_file_stat_by_path(&path).is_err());
    let target = root.path().join("note.md");
    fs::write(&target, b"regular").unwrap();
    let external = outside.path().join("note.md");
    filesystem.before_open.replace(Some(Box::new(move || {
        fs::remove_file(&target).unwrap();
        symlink(external, target).unwrap();
    })));
    // When / Then: replacement after stat fails without reading the target.
    assert!(
        filesystem
            .get_file(&RepoPath::parse("note.md").unwrap())
            .is_err()
    );
}

#[test]
fn missing_file_returns_none() {
    // Given: a repository without the requested documents.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    for raw in ["missing.md", "missing/child.md"] {
        let path = RepoPath::parse(raw).unwrap();
        // When / Then: reading and stating a missing document return no file.
        assert!(filesystem.get_file(&path).unwrap().is_none());
        assert!(filesystem.get_file_stat_by_path(&path).unwrap().is_none());
    }
}

#[test]
fn replacement_during_read_fails() {
    for replacement in ["regular", "fifo", "parent", "deleted", "modified"] {
        // Given
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        let full = root.path().join("dir/note.md");
        fs::write(&full, b"before").unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let directory = root.path().to_path_buf();
        filesystem
            .after_read
            .replace(Some(Box::new(move || match replacement {
                "parent" => {
                    fs::rename(directory.join("dir"), directory.join("old")).unwrap();
                    symlink(directory.join("old"), directory.join("dir")).unwrap();
                }
                "modified" => fs::write(&full, b"different length").unwrap(),
                other => {
                    fs::remove_file(&full).unwrap();
                    match other {
                        "regular" => fs::write(full, b"after!").unwrap(),
                        "fifo" => create_fifo(&full),
                        _ => {}
                    }
                }
            })));
        // When / Then
        assert!(
            filesystem
                .get_file(&RepoPath::parse("dir/note.md").unwrap())
                .is_err()
        );
    }
}

#[test]
fn fifo_replacement_before_open_is_rejected_without_blocking() {
    // Given
    let root = tempdir().unwrap();
    let path = root.path().join("note.md");
    fs::write(&path, b"regular").unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    filesystem.before_open.replace(Some(Box::new(move || {
        fs::remove_file(&path).unwrap();
        create_fifo(&path);
    })));
    let (sender, receiver) = mpsc::channel();
    // When: timeout is a hang watchdog, not a scheduling mechanism.
    let worker = thread::spawn(move || {
        sender
            .send(filesystem.get_file(&RepoPath::parse("note.md").unwrap()))
            .unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    // Then
    assert!(result.is_err());
    worker.join().unwrap();
}

#[test]
fn index_owner_writes_pid_after_acquiring_lock() {
    // Given
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    // When
    let _guard = filesystem.lock_index().unwrap();
    // Then
    assert_eq!(
        fs::read_to_string(root.path().join(".letopis/index.lock")).unwrap(),
        format!("{}\n", std::process::id())
    );
}

#[test]
fn embed_probe_preserves_busy_owner_bytes_and_independent_index_lock() {
    // Given a stale PID longer than this process's PID.
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".letopis")).unwrap();
    let path = root.path().join(".letopis/embed.lock");
    fs::write(&path, b"99999999999999999999999999999999\n").unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    // When: a stale PID does not prevent ownership; busy probes do not write.
    let guard = filesystem.try_lock_embed().unwrap().unwrap();
    let bytes = fs::read(&path).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    let _index = filesystem.lock_index().unwrap();
    assert!(filesystem.try_lock_embed().unwrap().is_none());
    // Then
    assert_eq!(bytes, format!("{}\n", std::process::id()).as_bytes());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(guard);
    let _next = filesystem.try_lock_embed().unwrap().unwrap();
    assert_eq!(fs::metadata(path).unwrap().ino(), inode);
}

#[test]
fn hardlinked_lock_targets_are_rejected_without_modifying_corpus_or_external_bytes() {
    for name in ["index.lock", "embed.lock"] {
        for external in [false, true] {
            // Given each target on the same filesystem as its lock hard link.
            let root = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let target_root = if external {
                outside.path()
            } else {
                root.path()
            };
            let target = target_root.join("private-note.md");
            let bytes = b"private corpus or external contents\0\xff";
            fs::write(&target, bytes).unwrap();
            fs::create_dir(root.path().join(".letopis")).unwrap();
            let path = root.path().join(".letopis").join(name);
            fs::hard_link(&target, &path).unwrap();
            let inode = fs::metadata(&target).unwrap().ino();
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            // When
            let result = if name == "index.lock" {
                filesystem.lock_index().map(Some)
            } else {
                filesystem.try_lock_embed()
            };
            // Then the error neither mutates the inode nor exposes private data.
            assert_eq!(
                fs::read(&target).unwrap(),
                bytes,
                "{name} external={external}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::metadata(&target).unwrap().ino(), inode);
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
            let error = result.err().expect("hard-linked lock must fail");
            assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
            let diagnostic = format!("{error} {error:?}");
            assert!(!diagnostic.contains("private corpus"));
            assert!(!diagnostic.contains(target.to_str().unwrap()));
        }
    }
}

#[test]
fn aliased_index_and_embed_names_are_rejected_without_mutation() {
    // Given
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".letopis")).unwrap();
    let index = root.path().join(".letopis/index.lock");
    let embed = root.path().join(".letopis/embed.lock");
    fs::write(&index, b"shared lock inode bytes").unwrap();
    fs::hard_link(&index, &embed).unwrap();
    let inode = fs::metadata(&index).unwrap().ino();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    for name in ["index.lock", "embed.lock"] {
        // When
        let result = if name == "index.lock" {
            filesystem.lock_index().map(Some)
        } else {
            filesystem.try_lock_embed()
        };
        // Then
        assert_eq!(fs::read(&index).unwrap(), b"shared lock inode bytes");
        assert_eq!(fs::read(&embed).unwrap(), b"shared lock inode bytes");
        assert_eq!(fs::metadata(&index).unwrap().ino(), inode);
        assert_eq!(fs::metadata(&embed).unwrap().ino(), inode);
        let error = result.err().expect("aliased lock names must fail");
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
    }
}

#[test]
fn hardlink_added_during_index_contention_is_rejected_after_acquiring_ownership() {
    // Given an initially private lock descriptor and observe actual contention.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let guard = filesystem.lock_index().unwrap();
    let path = root.path().join(".letopis/index.lock");
    fs::write(&path, b"holder bytes must survive").unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    let waiting = LocalFileSystem::open(root.path()).unwrap();
    let (sender, receiver) = mpsc::channel();
    waiting
        .on_contention
        .replace(Some(Box::new(move || sender.send(()).unwrap())));
    let worker = thread::spawn(move || waiting.lock_index());
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let alias = root.path().join("note.md");
    fs::hard_link(&path, &alias).unwrap();
    // When
    drop(guard);
    let result = worker.join().unwrap();
    // Then the pre-acquisition check cannot hide a subsequently added link.
    assert_eq!(fs::read(&path).unwrap(), b"holder bytes must survive");
    assert_eq!(fs::read(&alias).unwrap(), b"holder bytes must survive");
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
    let error = result
        .err()
        .expect("new hard link must fail after contention");
    assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
    // Removing the alias permits reacquisition; failure released its kernel lock.
    fs::remove_file(alias).unwrap();
    drop(filesystem.lock_index().unwrap());
}

#[test]
fn busy_embed_probe_rejects_new_hardlink_without_touching_owner_bytes() {
    // Given
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let _guard = filesystem.try_lock_embed().unwrap().unwrap();
    let path = root.path().join(".letopis/embed.lock");
    let alias = root.path().join("note.md");
    let bytes = fs::read(&path).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    fs::hard_link(&path, &alias).unwrap();
    // When
    let result = filesystem.try_lock_embed();
    // Then an unsafe descriptor is an error even when another owner is busy.
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(&alias).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
    let error = result.err().expect("hard-linked busy lock must fail");
    assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
}

#[test]
fn ordinary_hardlinked_document_reads_remain_supported() {
    // Given
    let root = tempdir().unwrap();
    fs::write(root.path().join("note.md"), b"unchanged document bytes").unwrap();
    fs::hard_link(root.path().join("note.md"), root.path().join("alias.md")).unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let inode = fs::metadata(root.path().join("note.md")).unwrap().ino();
    for name in ["note.md", "alias.md"] {
        // When
        let captured = filesystem
            .get_file(&RepoPath::parse(name).unwrap())
            .unwrap()
            .unwrap();
        // Then lock-specific validation must not reject ordinary document hard links.
        assert_eq!(captured.bytes, b"unchanged document bytes");
        assert_eq!(captured.stat.inode, inode);
        assert_eq!(
            fs::read(root.path().join(name)).unwrap(),
            b"unchanged document bytes"
        );
    }
}

#[test]
fn both_lock_names_reject_unsafe_files_and_service_directories() {
    for name in ["index.lock", "embed.lock"] {
        for kind in [
            "symlink",
            "fifo",
            "directory",
            "service_link",
            "service_file",
            "service_fifo",
        ] {
            // Given
            let root = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let target = outside.path().join("untouched");
            fs::write(&target, b"private outside bytes").unwrap();
            let service = root.path().join(".letopis");
            match kind {
                "service_link" => symlink(outside.path(), &service).unwrap(),
                "service_file" => fs::write(&service, b"invalid").unwrap(),
                "service_fifo" => create_fifo(&service),
                _ => {
                    fs::create_dir(&service).unwrap();
                    let path = service.join(name);
                    match kind {
                        "symlink" => symlink(&target, path).unwrap(),
                        "fifo" => create_fifo(&path),
                        _ => fs::create_dir(path).unwrap(),
                    }
                }
            }
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            // When / Then
            if name == "index.lock" {
                assert!(filesystem.lock_index().is_err());
            } else {
                assert!(filesystem.try_lock_embed().is_err());
            }
            assert_eq!(fs::read(target).unwrap(), b"private outside bytes");
        }
    }
}

#[test]
fn replacement_during_index_contention_is_rejected_before_pid_write() {
    for replacement in ["regular", "fifo", "service_link"] {
        // Given
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let guard = filesystem.lock_index().unwrap();
        let waiting = LocalFileSystem::open(root.path()).unwrap();
        let (sender, receiver) = mpsc::channel();
        waiting
            .on_contention
            .replace(Some(Box::new(move || sender.send(()).unwrap())));
        let worker = thread::spawn(move || waiting.lock_index());
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let path = root.path().join(".letopis/index.lock");
        let old = root.path().join("old.lock");
        fs::rename(&path, &old).unwrap();
        match replacement {
            "regular" => fs::write(&path, b"replacement must survive").unwrap(),
            "fifo" => create_fifo(&path),
            _ => {
                fs::rename(
                    root.path().join(".letopis"),
                    root.path().join("service-old"),
                )
                .unwrap();
                symlink(
                    root.path().join("service-old"),
                    root.path().join(".letopis"),
                )
                .unwrap();
            }
        }
        // When
        drop(guard);
        // Then
        assert!(worker.join().unwrap().is_err());
        assert_eq!(
            fs::read(old).unwrap(),
            format!("{}\n", std::process::id()).as_bytes()
        );
        if replacement == "regular" {
            assert_eq!(fs::read(path).unwrap(), b"replacement must survive");
        }
    }
}

#[test]
fn std_file_and_rustix_locks_exclude_each_other_for_both_names() {
    for name in ["index.lock", "embed.lock"] {
        // Given
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let guard = if name == "index.lock" {
            filesystem.lock_index().unwrap()
        } else {
            filesystem.try_lock_embed().unwrap().unwrap()
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.path().join(".letopis").join(name))
            .unwrap();
        // When / Then: std owner excludes a rustix peer, then reverse roles.
        assert_eq!(
            flock(&file, FlockOperation::NonBlockingLockExclusive),
            Err(Errno::WOULDBLOCK)
        );
        drop(guard);
        flock(&file, FlockOperation::NonBlockingLockExclusive).unwrap();
        if name == "embed.lock" {
            let before = fs::read(root.path().join(".letopis/embed.lock")).unwrap();
            assert!(filesystem.try_lock_embed().unwrap().is_none());
            assert_eq!(
                fs::read(root.path().join(".letopis/embed.lock")).unwrap(),
                before
            );
        } else {
            let waiting = LocalFileSystem::open(root.path()).unwrap();
            let (sender, receiver) = mpsc::channel();
            waiting
                .on_contention
                .replace(Some(Box::new(move || sender.send(()).unwrap())));
            let worker = thread::spawn(move || waiting.lock_index().unwrap());
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(!worker.is_finished());
            flock(&file, FlockOperation::Unlock).unwrap();
            drop(worker.join().unwrap());
        }
    }
}

#[test]
fn created_lock_files_and_service_directory_have_restrictive_modes() {
    // Given
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    // When
    let _index = filesystem.lock_index().unwrap();
    let _embed = filesystem.try_lock_embed().unwrap().unwrap();
    // Then
    for (name, allowed) in [
        (".letopis", 0o700),
        (".letopis/index.lock", 0o600),
        (".letopis/embed.lock", 0o600),
    ] {
        let mode = fs::metadata(root.path().join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode & !allowed, 0);
    }
}

const LOCK_CHILD: &str = "filesystem::local::tests::run_lock_child";
const PROCESS_WAIT: Duration = Duration::from_secs(10);

#[test]
#[ignore = "private lock subprocess entry, invoked by parent tests"]
fn run_lock_child() {
    // Given: configure the child process from the parent fixture.
    let socket = std::env::var_os("TK11_LOCK_SOCKET").unwrap();
    let root = std::env::var_os("TK11_LOCK_ROOT").unwrap();
    let mode = std::env::var("TK11_LOCK_MODE").unwrap();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(PROCESS_WAIT)).unwrap();
    // Keep the peer alive until the parent has configured its socket timeouts.
    stream.read_exact(&mut [0]).unwrap();
    let filesystem = LocalFileSystem::open(Path::new(&root)).unwrap();
    let mut contention = stream.try_clone().unwrap();
    if mode == "index" {
        filesystem.on_contention.replace(Some(Box::new(move || {
            writeln!(contention, "contended").unwrap();
        })));
    }
    // When: attempt the requested lock operation.
    let guard = if mode == "index" {
        filesystem.lock_index().unwrap()
    } else {
        let Some(guard) = filesystem.try_lock_embed().unwrap() else {
            eprintln!("busy {mode} pid={}", std::process::id());
            writeln!(stream, "busy").unwrap();
            return;
        };
        guard
    };
    // Then: report ownership to the parent, which checks the outcome.
    eprintln!("owned {mode} pid={}", std::process::id());
    writeln!(stream, "owned").unwrap();
    stream.read_exact(&mut [0]).unwrap();
    drop(guard);
    eprintln!("released {mode} pid={}", std::process::id());
    writeln!(stream, "released").unwrap();
}

struct LockWorker {
    child: Child,
    stream: BufReader<UnixStream>,
    _socket: tempfile::TempDir,
}

impl LockWorker {
    fn spawn(root: &Path, mode: &str) -> Self {
        let socket = tempfile::Builder::new()
            .prefix("tk11-lock-")
            .tempdir_in("/tmp")
            .unwrap();
        let address = socket.path().join("control");
        let listener = UnixListener::bind(&address).unwrap();
        let evidence = Path::new(".artifacts/tk11/task-2/process");
        fs::create_dir_all(evidence).unwrap();
        let name = socket.path().file_name().unwrap().to_str().unwrap();
        let log = fs::File::create(evidence.join(format!("{name}.log"))).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", LOCK_CHILD, "--ignored", "--nocapture"])
            .env("TK11_LOCK_SOCKET", &address)
            .env("TK11_LOCK_ROOT", root)
            .env("TK11_LOCK_MODE", mode)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(log)
            .spawn()
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let accept = thread::spawn(move || sender.send(listener.accept()).ok());
        let result = receiver.recv_timeout(PROCESS_WAIT);
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = UnixStream::connect(address);
            let _ = accept.join();
            panic!("lock child did not connect: {result:?}");
        }
        let (mut stream, _) = result.unwrap().unwrap();
        accept.join().unwrap();
        stream.set_read_timeout(Some(PROCESS_WAIT)).unwrap();
        stream.set_write_timeout(Some(PROCESS_WAIT)).unwrap();
        stream.write_all(b"s").unwrap();
        Self {
            child,
            stream: BufReader::new(stream),
            _socket: socket,
        }
    }

    fn get_event(&mut self) -> String {
        let mut line = String::new();
        assert!(self.stream.read_line(&mut line).unwrap() > 0);
        line.trim().to_owned()
    }

    fn release(&mut self) {
        self.stream.get_mut().write_all(b"r").unwrap();
        assert_eq!(self.get_event(), "released");
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for LockWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn process_owner_drop_or_death_releases_both_persistent_locks() {
    for mode in ["index", "embed"] {
        for kill in [false, true] {
            // Given
            let root = tempdir().unwrap();
            let mut owner = LockWorker::spawn(root.path(), mode);
            assert_eq!(owner.get_event(), "owned");
            let path = root.path().join(format!(".letopis/{mode}.lock"));
            let inode = fs::metadata(&path).unwrap().ino();
            let bytes = format!("{}\n", owner.child.id()).into_bytes();
            assert_eq!(fs::read(&path).unwrap(), bytes);
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let mut waiter = LockWorker::spawn(root.path(), mode);
            assert_eq!(
                waiter.get_event(),
                if mode == "index" { "contended" } else { "busy" }
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            // When
            if kill {
                owner.child.kill().unwrap();
                assert!(!owner.child.wait().unwrap().success());
            } else {
                owner.release();
            }
            // Then the stale PID and persistent inode survive release/death.
            if mode == "index" {
                assert_eq!(waiter.get_event(), "owned");
                assert_eq!(
                    fs::read(&path).unwrap(),
                    format!("{}\n", waiter.child.id()).as_bytes()
                );
                waiter.release();
                drop(filesystem.lock_index().unwrap());
            } else {
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert!(waiter.child.wait().unwrap().success());
                drop(filesystem.try_lock_embed().unwrap().unwrap());
            }
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
            assert_eq!(
                fs::read(path).unwrap(),
                format!("{}\n", std::process::id()).as_bytes()
            );
        }
    }
}

#[test]
fn concurrent_first_lock_creation_preserves_one_regular_file() {
    for _ in 0..20 {
        // Given both callers before the persistent lock file exists.
        let root = tempdir().unwrap();
        let barrier = Barrier::new(2);
        let inodes = thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let filesystem = LocalFileSystem::open(root.path()).unwrap();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        // When: create/open and acquire the same persistent lock.
                        let guard = filesystem.lock_index().unwrap();
                        fstat(&guard.file).unwrap().st_ino
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        // Then: both calls succeed against exactly the same regular inode.
        assert_eq!(inodes[0], inodes[1]);
        assert_eq!(
            fs::metadata(root.path().join(".letopis/index.lock"))
                .unwrap()
                .ino(),
            inodes[0]
        );
    }
}

#[test]
fn concurrent_first_embed_lock_creation_has_one_owner_and_stable_inode() {
    for _ in 0..20 {
        // Given: both probes start before the file exists and retain ownership through both probes.
        let root = tempdir().unwrap();
        let barrier = Barrier::new(2);
        let outcomes = thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let filesystem = LocalFileSystem::open(root.path()).unwrap();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        // When: both workers try to acquire the lock and inspect its inode.
                        let guard = filesystem.try_lock_embed();
                        let observed = filesystem.get_file_stat_by_raw_path(".letopis/embed.lock");
                        // Release the peer even when opening failed, so the
                        // assertion reports the error instead of hanging.
                        barrier.wait();
                        (observed.unwrap().unwrap().inode, guard.unwrap().is_some())
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        // Then: both observe the same inode and exactly one acquires the lock.
        assert_eq!(outcomes[0].0, outcomes[1].0);
        assert_ne!(outcomes[0].1, outcomes[1].1);
        assert_eq!(
            fs::metadata(root.path().join(".letopis/embed.lock"))
                .unwrap()
                .ino(),
            outcomes[0].0
        );
    }
}

#[test]
fn missing_creation_reopens_existing_lock_without_replacing_it() {
    for name in ["index.lock", "embed.lock"] {
        // Given: a competitor created the lock file before the current creation attempt.
        let root = tempdir().unwrap();
        let path = root.path().join(name);
        fs::write(&path, b"persistent owner state").unwrap();
        let directory = fs::File::open(root.path()).unwrap();
        let before = fs::metadata(&path).unwrap().ino();
        // When: select exactly the ENOENT creation-error path.
        let reopened =
            reopen_concurrently_created_lock(&directory, name, Err(Errno::NOENT)).unwrap();
        // Then: identity and bytes remain intact, and unrelated errors do not trigger fallback.
        assert_eq!(
            get_file_stat_by_descriptor(&reopened).unwrap().inode,
            before
        );
        assert_eq!(fs::read(&path).unwrap(), b"persistent owner state");
        assert_eq!(
            reopen_concurrently_created_lock(&directory, name, Err(Errno::ACCESS)).unwrap_err(),
            Errno::ACCESS
        );
    }
}

#[test]
fn missing_creation_reopen_rejects_absent_and_unsafe_lock_names() {
    for name in ["index.lock", "embed.lock"] {
        for kind in ["absent", "symlink", "fifo", "directory"] {
            // Given: an absent or unsafe name under the same live directory descriptor.
            let root = tempdir().unwrap();
            let path = root.path().join(name);
            let directory = fs::File::open(root.path()).unwrap();
            let outside = tempdir().unwrap();
            let target = outside.path().join("untouched");
            fs::write(&target, b"outside").unwrap();
            match kind {
                "symlink" => symlink(&target, &path).unwrap(),
                "fifo" => create_fifo(&path),
                "directory" => fs::create_dir(&path).unwrap(),
                _ => {}
            }
            // When: the fallback may open a FIFO, but the regular-file check rejects it.
            let result = reopen_concurrently_created_lock(&directory, name, Err(Errno::NOENT));
            // Then: no file is created, symlink followed, or nonregular file accepted.
            if let Ok(file) = result {
                assert!(get_file_stat_by_descriptor(&file).is_err());
            }
            if kind == "absent" {
                assert!(!path.exists());
            }
            assert_eq!(fs::read(&target).unwrap(), b"outside");
        }
    }
}

#[test]
fn lock_releases_on_drop() {
    // Given
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let guard = filesystem.lock_index().unwrap();
    // Model a descriptor briefly inherited by a concurrent fork before exec.
    let inherited = guard.file.try_clone().unwrap();
    let path = root.path().join(".letopis/index.lock");
    let second_fd = fs::File::open(&path).unwrap();
    let inode = second_fd.metadata().unwrap().ino();
    fs::write(&path, b"persistent").unwrap();
    assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_err());
    // When
    drop(guard);
    // Then
    assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_ok());
    drop(second_fd);
    drop(inherited);
    assert_eq!(fs::read(&path).unwrap(), b"persistent");
    let _next = filesystem.lock_index().unwrap();
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        fs::read(path).unwrap(),
        format!("{}\n", std::process::id()).as_bytes()
    );
}

#[test]
fn contended_lock_waits_for_release() {
    // Given
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::open(root.path()).unwrap();
    let guard = filesystem.lock_index().unwrap();
    let second = LocalFileSystem::open(root.path()).unwrap();
    let (sender, receiver) = mpsc::channel();
    second
        .on_contention
        .replace(Some(Box::new(move || sender.send(()).unwrap())));
    let worker = thread::spawn(move || second.lock_index().unwrap());
    receiver.recv().unwrap();
    assert!(!worker.is_finished());
    // When
    drop(guard);
    // Then
    drop(worker.join().unwrap());
}

#[test]
fn reject_service_directory_and_lock_symlinks() {
    for directory_link in [true, false] {
        // Given
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("target"), b"untouched").unwrap();
        if directory_link {
            symlink(outside.path(), root.path().join(".letopis")).unwrap();
        } else {
            fs::create_dir(root.path().join(".letopis")).unwrap();
            symlink(
                outside.path().join("target"),
                root.path().join(".letopis/index.lock"),
            )
            .unwrap();
        }
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        // When / Then
        assert!(filesystem.lock_index().is_err());
        assert_eq!(
            fs::read(outside.path().join("target")).unwrap(),
            b"untouched"
        );
    }
}

fn create_fifo(path: &Path) {
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
}
