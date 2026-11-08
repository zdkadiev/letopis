use super::{
    LocalFileSystem, file_build_stat, file_get_lock_stat, file_reopen_lock, file_retry_io,
    file_verify_unchanged, lock_finish_acquisition, lock_get_file_by_path, lock_retry_acquisition,
};
use crate::{
    domain::RepoPath,
    filesystem::test_support::{Fixture, run_bounded},
    service::{Clock, FileErrorKind, FileLocker, FileReader, FileWriter},
};
use rustix::{
    fs::{FlockOperation, flock, fstat},
    io::Errno,
};
use std::{
    error::Error,
    fs::{self, File, TryLockError},
    io::{BufRead, BufReader, ErrorKind, Read, Write},
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
use tempfile::{TempDir, tempdir};

#[test]
fn get_regular_file_preserves_bytes_and_stat() {
    // Given: a regular document with non-UTF-8 bytes and a Unicode path.
    let root = tempdir().unwrap();
    let path = RepoPath::parse("заметка with spaces.md").unwrap();
    fs::write(root.path().join(path.as_str()), b"literal\0\xffbytes").unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let before = crate::process::SystemClock::new()
        .clock_get_time_ns()
        .unwrap();
    // When: the document and its file metadata are retrieved.
    let captured = filesystem
        .file_get_by_path(Path::new(path.as_str()))
        .unwrap()
        .unwrap();
    let stat = filesystem
        .file_get_stat_by_path(Path::new(path.as_str()))
        .unwrap()
        .unwrap();
    // Then: bytes, metadata, and capture time match the file.
    assert_eq!(captured.bytes, b"literal\0\xffbytes");
    assert_eq!(captured.stat, stat);
    assert_eq!(stat.size, 14);
    assert_eq!(
        stat.inode,
        fs::metadata(root.path().join(path.as_str())).unwrap().ino()
    );
    assert!(captured.checked_at_ns >= before);
    assert!(
        captured.checked_at_ns
            <= crate::process::SystemClock::new()
                .clock_get_time_ns()
                .unwrap()
    );
}

#[test]
fn nonregular_final_entries_are_classified_without_following_symlinks() {
    // Given: a symlink to an external file, a directory, and a FIFO.
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("note.md"), b"secret").unwrap();
    symlink(outside.path().join("note.md"), root.path().join("link.md")).unwrap();
    fs::create_dir(root.path().join("directory.md")).unwrap();
    create_fifo(&root.path().join("pipe.md"));
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    for raw in ["link.md", "directory.md", "pipe.md"] {
        let path = RepoPath::parse(raw).unwrap();
        // When / Then: observation and capture reject the stable nonregular leaf.
        assert_eq!(
            filesystem
                .file_get_stat_by_path(Path::new(path.as_str()))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::NonRegular
        );
        assert_eq!(
            filesystem
                .file_get_by_path(Path::new(path.as_str()))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::NonRegular
        );
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
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        filesystem.between_stats.replace(Some(Box::new(move || {
            if initial == "symlink" {
                fs::remove_file(&path).unwrap();
            }
            create_fifo(&path);
        })));
        // When / Then: the changed entry is rejected instead of treated as stable absence.
        assert!(
            filesystem
                .file_get_stat_by_path(std::path::Path::new("note.md"))
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
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: traversal through the parent symlink fails.
    assert!(
        filesystem
            .file_get_by_path(Path::new(path.as_str()))
            .is_err()
    );
    assert!(
        filesystem
            .file_get_stat_by_path(Path::new(path.as_str()))
            .is_err()
    );
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
            .file_get_by_path(std::path::Path::new("note.md"))
            .is_err()
    );
}

#[test]
fn missing_file_returns_none() {
    // Given: a repository without the requested documents.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    for raw in ["missing.md", "missing/child.md"] {
        let path = RepoPath::parse(raw).unwrap();
        // When / Then: reading and stating a missing document return no file.
        assert!(
            filesystem
                .file_get_by_path(Path::new(path.as_str()))
                .unwrap()
                .is_none()
        );
        assert!(
            filesystem
                .file_get_stat_by_path(Path::new(path.as_str()))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn replacement_during_read_fails() {
    for replacement in ["regular", "fifo", "parent", "deleted", "modified"] {
        // Given: a regular file and an after-read replacement hook.
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        let full = root.path().join("dir/note.md");
        fs::write(&full, b"before").unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
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
        // When / Then: capture reads across the replacement; the changed capture is rejected.
        assert!(
            filesystem
                .file_get_by_path(std::path::Path::new("dir/note.md"))
                .is_err()
        );
    }
}

#[test]
fn fifo_replacement_before_open_is_rejected_without_blocking() {
    // Given: a regular leaf replaced by a FIFO at the open barrier.
    let root = tempdir().unwrap();
    let path = root.path().join("note.md");
    fs::write(&path, b"regular").unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    filesystem.before_open.replace(Some(Box::new(move || {
        fs::remove_file(&path).unwrap();
        create_fifo(&path);
    })));
    let (sender, receiver) = mpsc::channel();
    // When: timeout is a hang watchdog, not a scheduling mechanism.
    let worker = thread::spawn(move || {
        sender
            .send(filesystem.file_get_by_path(std::path::Path::new("note.md")))
            .unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    // Then: the capture fails promptly instead of blocking.
    assert!(result.is_err());
    worker.join().unwrap();
}

#[test]
fn index_owner_writes_pid_after_acquiring_lock() {
    // Given: an unowned persistent index lock path.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    // When: the index lock is acquired.
    let _guard = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    // Then: the file contains the current owner PID.
    assert_eq!(
        fs::read_to_string(root.path().join(".letopis/index.lock")).unwrap(),
        format!("{}\n", std::process::id())
    );
}

#[test]
fn embed_probe_preserves_busy_owner_bytes_and_independent_index_lock() {
    // Given: a stale PID longer than this process's PID.
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".letopis")).unwrap();
    let path = root.path().join(".letopis/embed.lock");
    fs::write(&path, b"99999999999999999999999999999999\n").unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    // When: a stale PID does not prevent ownership; busy probes do not write.
    let guard = filesystem
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    let _index = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    assert!(
        filesystem
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    // Then: contention preserves the owner bytes and index ownership remains independent.
    assert_eq!(bytes, format!("{}\n", std::process::id()).as_bytes());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(guard);
    let _next = filesystem
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::metadata(path).unwrap().ino(), inode);
}

#[test]
fn hardlinked_lock_targets_are_rejected_without_modifying_corpus_or_external_bytes() {
    for name in ["index.lock", "embed.lock"] {
        for external in [false, true] {
            // Given: each target on the same filesystem as its lock hard link.
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
            let filesystem = LocalFileSystem::new(root.path()).unwrap();
            // When: both lock acquisition modes validate those aliases.
            let result = if name == "index.lock" {
                filesystem
                    .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                    .map(Some)
            } else {
                filesystem.lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            };
            // Then: the error neither mutates the inode nor exposes private data.
            assert_eq!(
                fs::read(&target).unwrap(),
                bytes,
                "{name} external={external}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::metadata(&target).unwrap().ino(), inode);
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
            let error = result.expect_err("hard-linked lock must fail");
            assert_eq!(error.get_kind(), FileErrorKind::Aliased);
            let diagnostic = format!("{error} {error:?}");
            assert!(!diagnostic.contains("private corpus"));
            assert!(!diagnostic.contains(target.to_str().unwrap()));
        }
    }
}

#[test]
fn aliased_index_and_embed_names_are_rejected_without_mutation() {
    // Given: both lock names refer to the same inode.
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".letopis")).unwrap();
    let index = root.path().join(".letopis/index.lock");
    let embed = root.path().join(".letopis/embed.lock");
    fs::write(&index, b"shared lock inode bytes").unwrap();
    fs::hard_link(&index, &embed).unwrap();
    let inode = fs::metadata(&index).unwrap().ino();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    for name in ["index.lock", "embed.lock"] {
        // When: each role attempts acquisition.
        let result = if name == "index.lock" {
            filesystem
                .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                .map(Some)
        } else {
            filesystem.lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        };
        // Then: the alias is rejected without changing owner bytes.
        assert_eq!(fs::read(&index).unwrap(), b"shared lock inode bytes");
        assert_eq!(fs::read(&embed).unwrap(), b"shared lock inode bytes");
        assert_eq!(fs::metadata(&index).unwrap().ino(), inode);
        assert_eq!(fs::metadata(&embed).unwrap().ino(), inode);
        let error = result.expect_err("aliased lock names must fail");
        assert_eq!(error.get_kind(), FileErrorKind::Aliased);
    }
}

#[test]
fn hardlink_added_during_index_contention_is_rejected_after_acquiring_ownership() {
    // Given: an initially private lock descriptor and observe actual contention.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let guard = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    let path = root.path().join(".letopis/index.lock");
    fs::write(&path, b"holder bytes must survive").unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    let waiting = LocalFileSystem::new(root.path()).unwrap();
    let (sender, receiver) = mpsc::channel();
    waiting
        .on_contention
        .replace(Some(Box::new(move || sender.send(()).unwrap())));
    let worker = thread::spawn(move || {
        waiting.lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
    });
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let alias = root.path().join("note.md");
    fs::hard_link(&path, &alias).unwrap();
    // When: the earlier owner releases the waiting acquisition.
    drop(guard);
    let result = worker.join().unwrap();
    // Then: the pre-acquisition check cannot hide a subsequently added link.
    assert_eq!(fs::read(&path).unwrap(), b"holder bytes must survive");
    assert_eq!(fs::read(&alias).unwrap(), b"holder bytes must survive");
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
    let error = result.expect_err("new hard link must fail after contention");
    assert_eq!(error.get_kind(), FileErrorKind::Aliased);
    // Removing the alias permits reacquisition; failure released its kernel lock.
    fs::remove_file(alias).unwrap();
    drop(
        filesystem
            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
            .unwrap(),
    );
}

#[test]
fn busy_embed_probe_rejects_new_hardlink_without_touching_owner_bytes() {
    // Given: an embed lock whose held inode gains a hardlink.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let _guard = filesystem
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let path = root.path().join(".letopis/embed.lock");
    let alias = root.path().join("note.md");
    let bytes = fs::read(&path).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    fs::hard_link(&path, &alias).unwrap();
    // When: a nonblocking caller probes the aliased lock.
    let result = filesystem.lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"));
    // Then: an unsafe descriptor is an error even when another owner is busy.
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(&alias).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
    let error = result.expect_err("hard-linked busy lock must fail");
    assert_eq!(error.get_kind(), FileErrorKind::Aliased);
}

#[test]
fn ordinary_hardlinked_document_reads_remain_supported() {
    // Given: two ordinary file names share an inode.
    let root = tempdir().unwrap();
    fs::write(root.path().join("note.md"), b"unchanged document bytes").unwrap();
    fs::hard_link(root.path().join("note.md"), root.path().join("alias.md")).unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let inode = fs::metadata(root.path().join("note.md")).unwrap().ino();
    for name in ["note.md", "alias.md"] {
        // When: each name is captured through the generic reader.
        let captured = filesystem
            .file_get_by_path(std::path::Path::new(name))
            .unwrap()
            .unwrap();
        // Then: lock-specific validation must not reject ordinary document hard links.
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
            // Given: nonregular lock leaves or unsafe parent directories for each role.
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
            let filesystem = LocalFileSystem::new(root.path()).unwrap();
            // When / Then: each lock role attempts acquisition; unsafe paths fail without modifying the outside target.
            if name == "index.lock" {
                assert!(
                    filesystem
                        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                        .is_err()
                );
            } else {
                assert!(
                    filesystem
                        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                        .is_err()
                );
            }
            assert_eq!(fs::read(target).unwrap(), b"private outside bytes");
        }
    }
}

#[test]
fn replacement_during_index_contention_is_rejected_before_pid_write() {
    for replacement in ["regular", "fifo", "service_link"] {
        // Given: a contended index lock with a replacement at the wait barrier.
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let guard = filesystem
            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
            .unwrap();
        let waiting = LocalFileSystem::new(root.path()).unwrap();
        let (sender, receiver) = mpsc::channel();
        waiting
            .on_contention
            .replace(Some(Box::new(move || sender.send(()).unwrap())));
        let worker = thread::spawn(move || {
            waiting.lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        });
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
        // When: the waiting caller resumes after the original owner drops.
        drop(guard);
        // Then: replacement is rejected before recording a new PID.
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
        // Given: independent standard-library and rustix lock owners.
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let guard = if name == "index.lock" {
            filesystem
                .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                .unwrap()
        } else {
            filesystem
                .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                .unwrap()
                .unwrap()
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
            assert!(
                filesystem
                    .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                fs::read(root.path().join(".letopis/embed.lock")).unwrap(),
                before
            );
        } else {
            let waiting = LocalFileSystem::new(root.path()).unwrap();
            let (sender, receiver) = mpsc::channel();
            waiting
                .on_contention
                .replace(Some(Box::new(move || sender.send(()).unwrap())));
            let worker = thread::spawn(move || {
                waiting
                    .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                    .unwrap()
            });
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(!worker.is_finished());
            flock(&file, FlockOperation::Unlock).unwrap();
            drop(worker.join().unwrap());
        }
    }
}

#[test]
fn created_lock_files_and_service_directory_have_restrictive_modes() {
    // Given: an absent private directory and two lock paths.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    // When: both file locks are acquired.
    let _index = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    let _embed = filesystem
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    // Then: the directory is 0700 and each persistent lock is 0600.
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
    let filesystem = LocalFileSystem::new(Path::new(&root)).unwrap();
    let mut contention = stream.try_clone().unwrap();
    if mode == "index" {
        filesystem.on_contention.replace(Some(Box::new(move || {
            writeln!(contention, "contended").unwrap();
        })));
    }
    // When: attempt the requested lock operation.
    let guard = if mode == "index" {
        filesystem
            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
            .unwrap()
    } else {
        let Some(guard) = filesystem
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
        else {
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
            // Given: an independent process owns each persistent lock.
            let root = tempdir().unwrap();
            let mut owner = LockWorker::spawn(root.path(), mode);
            assert_eq!(owner.get_event(), "owned");
            let path = root.path().join(format!(".letopis/{mode}.lock"));
            let inode = fs::metadata(&path).unwrap().ino();
            let bytes = format!("{}\n", owner.child.id()).into_bytes();
            assert_eq!(fs::read(&path).unwrap(), bytes);
            let filesystem = LocalFileSystem::new(root.path()).unwrap();
            let mut waiter = LockWorker::spawn(root.path(), mode);
            assert_eq!(
                waiter.get_event(),
                if mode == "index" { "contended" } else { "busy" }
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            // When: the owner exits normally or is killed.
            if kill {
                owner.child.kill().unwrap();
                assert!(!owner.child.wait().unwrap().success());
            } else {
                owner.release();
            }
            // Then: the stale PID and persistent inode survive release/death.
            if mode == "index" {
                assert_eq!(waiter.get_event(), "owned");
                assert_eq!(
                    fs::read(&path).unwrap(),
                    format!("{}\n", waiter.child.id()).as_bytes()
                );
                waiter.release();
                drop(
                    filesystem
                        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                        .unwrap(),
                );
            } else {
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert!(waiter.child.wait().unwrap().success());
                drop(
                    filesystem
                        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                        .unwrap()
                        .unwrap(),
                );
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
        // Given: both callers before the persistent lock file exists.
        let root = tempdir().unwrap();
        let barrier = Barrier::new(2);
        let inodes = thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let filesystem = LocalFileSystem::new(root.path()).unwrap();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        // When: create/open and acquire the same persistent lock.
                        let guard = filesystem
                            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                            .unwrap();
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
                    let filesystem = LocalFileSystem::new(root.path()).unwrap();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        // When: both workers try to acquire the lock and inspect its inode.
                        let guard = filesystem
                            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"));
                        let observed =
                            filesystem.file_get_identity_by_path(Path::new(".letopis/embed.lock"));
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
fn lock_releases_on_drop() {
    // Given: an owned lock with a duplicate descriptor still open.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let guard = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    // Model a descriptor briefly inherited by a concurrent fork before exec.
    let inherited = guard.file.try_clone().unwrap();
    let path = root.path().join(".letopis/index.lock");
    let second_fd = fs::File::open(&path).unwrap();
    let inode = second_fd.metadata().unwrap().ino();
    fs::write(&path, b"persistent").unwrap();
    assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_err());
    // When: the ownership guard is dropped.
    drop(guard);
    // Then: the kernel lock is released before the duplicate closes.
    assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_ok());
    drop(second_fd);
    drop(inherited);
    assert_eq!(fs::read(&path).unwrap(), b"persistent");
    let _next = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        fs::read(path).unwrap(),
        format!("{}\n", std::process::id()).as_bytes()
    );
}

#[test]
fn contended_lock_waits_for_release() {
    // Given: one held lock and a second waiting caller.
    let root = tempdir().unwrap();
    let filesystem = LocalFileSystem::new(root.path()).unwrap();
    let guard = filesystem
        .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
        .unwrap();
    let second = LocalFileSystem::new(root.path()).unwrap();
    let (sender, receiver) = mpsc::channel();
    second
        .on_contention
        .replace(Some(Box::new(move || sender.send(()).unwrap())));
    let worker = thread::spawn(move || {
        second
            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
            .unwrap()
    });
    receiver.recv().unwrap();
    assert!(!worker.is_finished());
    // When: the owner drops its guard.
    drop(guard);
    // Then: the waiter acquires the same persistent file.
    drop(worker.join().unwrap());
}

#[test]
fn reject_service_directory_and_lock_symlinks() {
    for directory_link in [true, false] {
        // Given: a service-directory or lock symlink to an outside target.
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
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        // When / Then: lock acquisition encounters the unsafe path; the link is rejected and outside bytes remain intact.
        assert!(
            filesystem
                .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                .is_err()
        );
        assert_eq!(
            fs::read(outside.path().join("target")).unwrap(),
            b"untouched"
        );
    }
}

fn create_fifo(path: &Path) {
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
}

mod init_tests {
    use crate::filesystem::LocalFileSystem;
    use crate::filesystem::local::{SyncTarget, file_fail_next_sync};
    use crate::filesystem::test_support::Fixture;
    use crate::service::FileWriter;
    use std::path::Path;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        sync::{Arc, Barrier},
        thread,
    };

    const FIRST: &[u8] = b"{\"embedding_model\":{\"enabled\":false,\"provider\":null,\"model\":null,\"parameters\":{},\"max_retries\":0}}\n";
    const SECOND: &[u8] = b"{\"embedding_model\":{\"enabled\":false,\"provider\":null,\"model\":null,\"parameters\":{},\"max_retries\":3}}\n";

    #[test]
    fn installs_exact_private_complete_bytes() {
        // Given: an existing private parent and an absent publication leaf.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();

        // When: complete bytes are published without replacement.
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap();

        // Then: the final bytes are exact and the mode is 0600.
        let path = fixture.0.join(".letopis/config.json");
        assert_eq!(fs::read(&path).unwrap(), FIRST);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn protects_a_preexisting_permissive_service_directory() {
        // Given: `.letopis` already exists with group/other access.
        let fixture = Fixture::new();
        let directory = fixture.0.join(".letopis");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();

        // When: initialization opens a private service directory.
        files
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();

        // Then: the pinned directory is private before any database is created.
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn existing_configuration_is_never_replaced() {
        // Given: a complete first publication.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap();

        // When: a second payload is published to the same path.
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
            .unwrap();

        // Then: the first winner survives byte for byte.
        assert_eq!(
            fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
            FIRST
        );
    }

    #[test]
    fn concurrent_publishers_leave_one_complete_configuration() {
        // Given: distinct complete payloads racing for one absent leaf.
        let fixture = Fixture::new();
        let paths = Arc::new(fixture.0.clone());
        let barrier = Arc::new(Barrier::new(6));
        let mut workers = Vec::new();
        for index in 0..6 {
            let path = Arc::clone(&paths);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let files = LocalFileSystem::new(&path).unwrap();
                files
                    .directory_create_by_path(Path::new(".letopis"))
                    .unwrap();
                let payload = if index % 2 == 0 { FIRST } else { SECOND };
                barrier.wait();
                files.file_create_if_absent_by_path(Path::new(".letopis/config.json"), payload)
            }));
        }

        // When / Then: every loser accepts the winner, and no mixed file exists.
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let written = fs::read(fixture.0.join(".letopis/config.json")).unwrap();
        assert!(written == FIRST || written == SECOND);
    }

    #[test]
    fn rejects_symlink_and_nonregular_configuration_leaves() {
        // Given: an outside target and two unsafe destination leaves.
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("config.json");
        fs::write(&target, b"PRIVATE").unwrap();
        fs::create_dir_all(fixture.0.join(".letopis")).unwrap();
        symlink(&target, fixture.0.join(".letopis/config.json")).unwrap();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();

        // When / Then: no-clobber publication validates the destination; unsafe leaves are rejected without changing outside bytes.
        assert!(
            files
                .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
                .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"PRIVATE");
        fs::remove_file(fixture.0.join(".letopis/config.json")).unwrap();
        fs::create_dir(fixture.0.join(".letopis/config.json")).unwrap();
        assert!(
            files
                .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
                .is_err()
        );
    }

    #[test]
    fn file_sync_failure_leaves_absence_and_retry_publishes() {
        // Given: a service directory but no visible configuration.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();
        files
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();
        file_fail_next_sync(SyncTarget::File);

        // When: publication tries to sync the temporary file.
        let error = files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap_err();

        // Then: no partial config or temporary entry remains, and retry completes.
        assert!(!format!("{error:?}").contains("PRIVATE"));
        assert!(!fixture.0.join(".letopis/config.json").exists());
        assert_eq!(fs::read_dir(fixture.0.join(".letopis")).unwrap().count(), 0);
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap();
        assert_eq!(
            fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
            FIRST
        );
    }

    #[test]
    fn directory_sync_failure_after_existing_leaf_retries_durability() {
        // Given: an existing configuration and a failing directory sync.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap();
        file_fail_next_sync(SyncTarget::Directory);

        // When: publication retries durability for the retained winner.
        let error = files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
            .unwrap_err();

        // Then: the existing bytes survive and a retry finishes the sync step.
        assert_eq!(
            fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
            FIRST
        );
        assert!(!format!("{error:?}").contains("PRIVATE"));
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
            .unwrap();
        assert_eq!(
            fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
            FIRST
        );
    }

    #[test]
    fn service_state_sync_retries_after_root_failure_without_creating_files() {
        // Given: a completed config publication.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .unwrap();
        let before = fs::read_dir(fixture.0.join(".letopis")).unwrap().count();
        file_fail_next_sync(SyncTarget::Directory);

        // When: the root durability step is attempted and retried.
        let error = files.directory_sync_by_path(Path::new(".")).unwrap_err();

        // Then: failure creates nothing and retry can complete durability work.
        assert!(!format!("{error:?}").contains("PRIVATE"));
        assert_eq!(
            fs::read_dir(fixture.0.join(".letopis")).unwrap().count(),
            before
        );
        files.directory_sync_by_path(Path::new(".")).unwrap();
    }

    #[test]
    fn rejects_replaced_service_directory_and_missing_state_sync() {
        // Given: a filesystem which pinned the original service directory.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(Path::new(".letopis"))
            .unwrap();
        files
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();
        assert!(files.directory_sync_by_path(Path::new(".letopis")).is_ok());
        fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
        fs::create_dir(fixture.0.join(".letopis")).unwrap();

        // When / Then: publication and durability operations reuse the original pin; replacement or absence is rejected without recreating state.
        assert!(
            files
                .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
                .is_err()
        );
        assert!(files.directory_sync_by_path(Path::new(".letopis")).is_err());
        let empty = Fixture::new();
        let fresh = LocalFileSystem::new(&empty.0).unwrap();
        assert!(fresh.directory_sync_by_path(Path::new(".letopis")).is_err());
    }
}

mod read_tests {
    use crate::filesystem::LocalFileSystem;
    use crate::filesystem::test_support::{Fixture, run_bounded};
    use crate::service::FileErrorKind;
    use crate::service::FileReader;
    use std::path::Path;
    use std::{error::Error, fs};

    #[test]
    fn symlink_observation_distinguishes_missing_and_nonregular_leaves() {
        // Given: ordinary, directory, FIFO, dangling symlink and absent entries.
        let fixture = Fixture::new();
        fs::write(fixture.0.join("file"), b"bytes").unwrap();
        fs::create_dir(fixture.0.join("directory")).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(fixture.0.join("fifo"))
                .status()
                .unwrap()
                .success()
        );
        std::os::unix::fs::symlink("missing", fixture.0.join("link")).unwrap();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        // When / Then: only the final symlink is true and no entry body is opened.
        assert!(files.symlink_is_exist(Path::new("link")).unwrap());
        for name in ["file", "directory", "fifo", "missing", "missing/child"] {
            assert!(!files.symlink_is_exist(Path::new(name)).unwrap(), "{name}");
        }
    }

    #[test]
    fn raw_symlink_probe_never_follows_a_parent() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        // Given: a raw filename beneath an outside-pointing symlink parent.
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), fixture.0.join("parent")).unwrap();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        let path = Path::new(OsStr::from_bytes(b"parent/raw\xff.md"));
        // When: a generic symlink observation resolves the raw path.
        let error = files.symlink_is_exist(path).unwrap_err();
        // Then: the parent is rejected before the invalid-byte leaf is touched.
        assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
    }

    #[test]
    fn rejects_observed_replacement_between_stat_and_open() {
        if run_bounded(
            "filesystem::local::tests::read_tests::rejects_observed_replacement_between_stat_and_open",
        ) {
            return;
        }
        for action in ["regular", "symlink", "fifo", "delete"] {
            // Given: an independent process waits at the exact open boundary.
            let fixture = Fixture::new();
            fs::write(fixture.0.join("note.md"), b"original").unwrap();
            let root = LocalFileSystem::new(&fixture.0).unwrap();
            let mut mutator = fixture.spawn_mutator("note.md", action);
            // When: capture resumes after replacement or deletion.
            root.before_open.replace(Some(Box::new(move || {
                mutator.resume();
                mutator.finish();
            })));
            let result = root.file_get_by_path(Path::new("note.md"));
            // Then: the unsafe or changed file cannot be captured.
            let error = result.unwrap_err();
            if action == "symlink" {
                assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
                assert!(
                    error.source().is_some(),
                    "NOFOLLOW must reject the open syscall"
                );
            }
        }
    }

    #[test]
    fn rejects_observed_changes_after_reading_bytes() {
        if run_bounded(
            "filesystem::local::tests::read_tests::rejects_observed_changes_after_reading_bytes",
        ) {
            return;
        }
        for action in ["regular", "delete", "write"] {
            // Given: a regular file with an after-read mutation barrier.
            let fixture = Fixture::new();
            fs::write(fixture.0.join("note.md"), b"original").unwrap();
            let root = LocalFileSystem::new(&fixture.0).unwrap();
            let mut mutator = fixture.spawn_mutator("note.md", action);
            // When: capture revalidates bytes after the mutator runs.
            root.after_read.replace(Some(Box::new(move || {
                mutator.resume();
                mutator.finish();
            })));
            let result = root.file_get_by_path(Path::new("note.md"));
            // Then: every content or identity change fails capture.
            assert!(result.is_err(), "{action}");
        }
    }

    #[test]
    fn rejects_observed_ancestor_replacement() {
        if run_bounded(
            "filesystem::local::tests::read_tests::rejects_observed_ancestor_replacement",
        ) {
            return;
        }
        for action in ["directory", "symlink", "delete"] {
            // Given: a file under a directory that another process replaces.
            let fixture = Fixture::new();
            fs::create_dir(fixture.0.join("notes")).unwrap();
            fs::write(fixture.0.join("notes/a.md"), b"original").unwrap();
            let root = LocalFileSystem::new(&fixture.0).unwrap();
            let mut mutator = fixture.spawn_mutator("notes", action);
            // When: capture revalidates ancestry after reading.
            root.after_read.replace(Some(Box::new(move || {
                mutator.resume();
                mutator.finish();
            })));
            let result = root.file_get_by_path(Path::new("notes/a.md"));
            // Then: the replaced ancestry invalidates the observation.
            assert!(result.is_err(), "{action}");
        }
    }

    #[test]
    fn rejects_device_files_without_reading_them() {
        if run_bounded(
            "filesystem::local::tests::read_tests::rejects_device_files_without_reading_them",
        ) {
            return;
        }
        // Given: known device leaves under the device directory.
        let root = LocalFileSystem::new(Path::new("/dev")).unwrap();
        for name in ["null", "zero"] {
            // Given: each device has independent guards against entering opening or reading.
            root.before_open.replace(Some(Box::new(move || {
                panic!("device {name} reached opening protocol")
            })));
            root.after_read.replace(Some(Box::new(move || {
                panic!("device {name} reached reading protocol")
            })));
            // When: generic capture observes this device's metadata.
            let error = root.file_get_by_path(Path::new(name)).unwrap_err();
            // Then: rejection happens before either I/O guard can run.
            assert_eq!(error.get_kind(), FileErrorKind::NonRegular);
        }
    }
}

mod race_tests {
    use crate::filesystem::LocalFileSystem;
    use crate::filesystem::test_support::run_bounded_unprivileged;
    use crate::service::{FileErrorKind, FileLocker, FileReader};
    use std::io::ErrorKind;
    use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path};
    use tempfile::TempDir;

    #[test]
    fn contended_lock_rejects_root_replacement_before_returning_absence() {
        // Given: an owner holds an inode under a root that will be replaced on contention.
        let fixture = TempDir::new().unwrap();
        let root = fixture.path().join("root");
        fs::create_dir(&root).unwrap();
        let files = LocalFileSystem::new(&root).unwrap();
        let guard = files.lock_acquire_by_path(Path::new("owner")).unwrap();
        let saved = fixture.path().join("saved");
        let source = root.clone();
        files.on_contention.replace(Some(Box::new(move || {
            fs::rename(&source, &saved).unwrap();
            fs::create_dir(&source).unwrap();
        })));
        // When: another acquisition observes contention after the root is replaced.
        let error = files
            .lock_try_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap();
        // Then: stale root ownership cannot escape as ordinary absence or alter the owner.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert!(!root.join("owner").exists());
        drop(guard);
    }

    #[test]
    fn unheld_lock_observation_rejects_root_replacement_before_returning_absence() {
        // Given: an unlocked persistent leaf under a root replaced at its open boundary.
        let fixture = TempDir::new().unwrap();
        let root = fixture.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("owner"), b"stale").unwrap();
        let files = LocalFileSystem::new(&root).unwrap();
        let saved = fixture.path().join("saved");
        let source = root.clone();
        files.before_open.replace(Some(Box::new(move || {
            fs::rename(&source, &saved).unwrap();
            fs::create_dir(&source).unwrap();
        })));
        // When: the observer opens the retained old root and finds an unheld lock.
        let error = files
            .lock_get_owner_by_path(Path::new("owner"))
            .unwrap_err();
        // Then: root replacement is an integrity error and observation leaves bytes unchanged.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert_eq!(
            fs::read(fixture.path().join("saved/owner")).unwrap(),
            b"stale"
        );
        assert!(!root.join("owner").exists());
    }

    #[test]
    fn directory_completion_rejects_root_replacement_for_existing_paths() {
        // Given: directory traversal will replace its root immediately before completion.
        let path = ".";
        let fixture = TempDir::new().unwrap();
        let root = fixture.path().join("root");
        fs::create_dir(&root).unwrap();
        let files = LocalFileSystem::new(&root).unwrap();
        let saved = fixture.path().join("saved");
        let source = root.clone();
        files
            .before_directory_complete
            .replace(Some(Box::new(move || {
                fs::rename(&source, &saved).unwrap();
                fs::create_dir(&source).unwrap();
            })));
        // When: traversal completes with either an owned descriptor or an absent parent.
        let error = files.directory_get_by_path(Path::new(path)).unwrap_err();
        // Then: both completion branches reject the replaced root without creating leaves.
        assert_eq!(error.get_kind(), FileErrorKind::Changed, "{path}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn missing_directory_completion_rejects_root_replacement() {
        // Given: directory traversal will replace its root immediately before completion.
        let path = "missing/leaf";
        let fixture = TempDir::new().unwrap();
        let root = fixture.path().join("root");
        fs::create_dir(&root).unwrap();
        let files = LocalFileSystem::new(&root).unwrap();
        let saved = fixture.path().join("saved");
        let source = root.clone();
        files
            .before_directory_complete
            .replace(Some(Box::new(move || {
                fs::rename(&source, &saved).unwrap();
                fs::create_dir(&source).unwrap();
            })));
        // When: traversal completes with either an owned descriptor or an absent parent.
        let error = files.directory_get_by_path(Path::new(path)).unwrap_err();
        // Then: both completion branches reject the replaced root without creating leaves.
        assert_eq!(error.get_kind(), FileErrorKind::Changed, "{path}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn distinctness_rejects_a_newly_created_hardlink_alias() {
        // Given: the first leaf is absent while the second is an ordinary file.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("second"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        files.before_distinct_verify.replace(Some(Box::new(move || {
            fs::hard_link(path.join("second"), path.join("first")).unwrap();
        })));
        // When: the initially absent leaf becomes an alias before the verification pass.
        let error = files
            .file_verify_distinct_by_paths(Path::new("first"), Path::new("second"))
            .unwrap_err();
        // Then: permissive creation does not permit the distinctness check to accept aliases.
        assert_eq!(error.get_kind(), FileErrorKind::Aliased);
        assert_eq!(fs::read(root.path().join("second")).unwrap(), b"original");
    }

    #[test]
    fn optional_identity_still_permits_creation_of_a_safe_regular_file() {
        // Given: a creating operation originally observed no leaf.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        assert!(
            files
                .file_get_identity_by_path(Path::new("item"))
                .unwrap()
                .is_none()
        );
        // When: a safe regular file is created without any promise of prior identity.
        fs::write(root.path().join("item"), b"created").unwrap();
        files
            .file_verify_identity_by_path(Path::new("item"), None)
            .unwrap();
        // Then: the ordinary identity API keeps its permissive creation contract.
        assert_eq!(fs::read(root.path().join("item")).unwrap(), b"created");
    }

    #[test]
    fn captured_leaf_deleted_before_open_is_changed() {
        // Given: an observed regular leaf will be deleted at the descriptor-open boundary.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("item"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("item");
        files
            .before_open
            .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
        // When: capture cannot open the leaf it previously observed.
        let error = files.file_get_by_path(Path::new("item")).unwrap_err();
        // Then: deletion remains an integrity failure with its preserved OS cause.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert!(error.source().is_some());
        assert!(!root.path().join("item").exists());
    }

    #[test]
    fn existing_lock_deleted_before_open_is_changed_for_acquisition() {
        // Given: an existing persistent leaf will disappear at the open boundary.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("owner"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("owner");
        files
            .before_open
            .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
        // When: this generic lock operation opens the already-observed inode.
        let error = files
            .lock_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap();
        // Then: deletion is an integrity failure with its preserved cause and no recreation.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert!(error.source().is_some());
        assert!(!root.path().join("owner").exists());
    }

    #[test]
    fn existing_lock_deleted_before_open_is_changed_for_nonblocking_acquisition() {
        // Given: an existing persistent leaf will disappear at the open boundary.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("owner"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("owner");
        files
            .before_open
            .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
        // When: this generic lock operation opens the already-observed inode.
        let error = files
            .lock_try_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap();
        // Then: deletion is an integrity failure with its preserved cause and no recreation.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert!(error.source().is_some());
        assert!(!root.path().join("owner").exists());
    }

    #[test]
    fn existing_lock_deleted_before_open_is_changed_for_observation() {
        // Given: an existing persistent leaf will disappear at the open boundary.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("owner"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("owner");
        files
            .before_open
            .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
        // When: this generic lock operation opens the already-observed inode.
        let error = files
            .lock_get_owner_by_path(Path::new("owner"))
            .unwrap_err();
        // Then: deletion is an integrity failure with its preserved cause and no recreation.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert!(error.source().is_some());
        assert!(!root.path().join("owner").exists());
    }

    #[test]
    fn genuine_permission_failure_before_open_remains_io() {
        // Given: an unprivileged fixture observes a leaf that will become unreadable.
        if run_bounded_unprivileged(
            "filesystem::local::tests::race_tests::genuine_permission_failure_before_open_remains_io",
        ) {
            return;
        }
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("item"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("item");
        files.before_open.replace(Some(Box::new(move || {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
            let denied = fs::File::open(path).unwrap_err();
            assert_eq!(denied.kind(), ErrorKind::PermissionDenied);
            assert_eq!(denied.raw_os_error(), Some(libc::EACCES));
        })));
        // When: the actual open syscall fails because owner access was removed.
        let error = files.file_get_by_path(Path::new("item")).unwrap_err();
        // Then: permission failure preserves its I/O classification and private cause.
        assert_eq!(error.get_kind(), FileErrorKind::Io);
        assert!(error.source().is_some());
    }
}

mod creation_tests {
    use crate::filesystem::{
        LocalFileSystem,
        local::{directory_set_before_nested_preflight, file_set_after_absence},
        test_support::{create_fifo, run_bounded},
    };
    use crate::service::{FileError, FileErrorKind, FileLocker, FileReader, FileWriter};
    use rustix::fs::fstat;
    use std::{
        fs::{self, File, OpenOptions},
        os::unix::fs::{MetadataExt, OpenOptionsExt, symlink},
        path::Path,
        sync::mpsc,
    };
    use tempfile::TempDir;

    #[derive(Clone, Copy, Debug)]
    enum UnsafeEntry {
        Symlink,
        Fifo,
        Hardlink,
        Directory,
        Regular,
    }

    #[derive(Clone, Copy, Debug)]
    enum Observation {
        Bytes,
        Stat,
        Identity,
        Exists,
        LockOwner,
    }

    #[test]
    fn held_peer_publication_after_first_absence_preserves_inode_and_owner() {
        for name in ["index.lock", "embed.lock"] {
            // Given: a real peer acquires the initially absent leaf before confirmation.
            let root = TempDir::new().unwrap();
            fs::create_dir(root.path().join("private")).unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            let path = Path::new("private").join(name);
            let peer_path = path.clone();
            let peer_root = root.path().to_owned();
            let (send, receive) = mpsc::channel();
            file_set_after_absence(Box::new(move || {
                let peer = LocalFileSystem::new(&peer_root).unwrap();
                let guard = peer.lock_acquire_by_path(&peer_path).unwrap();
                fs::write(peer_root.join(&peer_path), b"123456\n").unwrap();
                send.send(guard).unwrap();
            }));
            // When: the contender resolves publication against the actual kernel owner.
            let attempt = files.lock_try_acquire_by_path(&path).unwrap();
            let peer = receive
                .try_recv()
                .expect("peer publication hook did not run");
            // Then: contention preserves the peer inode/PID, and later ownership reuses it.
            assert!(attempt.is_none());
            let inode = peer.file.metadata().unwrap().ino();
            assert_eq!(fs::metadata(root.path().join(&path)).unwrap().ino(), inode);
            assert_eq!(fs::read(root.path().join(&path)).unwrap(), b"123456\n");
            drop(peer);
            let guard = files.lock_try_acquire_by_path(&path).unwrap().unwrap();
            assert_eq!(guard.file.metadata().unwrap().ino(), inode);
        }
    }

    #[test]
    fn unheld_peer_publication_after_first_absence_is_acquired_without_replacement() {
        for name in ["index.lock", "embed.lock"] {
            // Given: a peer publishes a regular unlocked inode at the first absence boundary.
            let root = TempDir::new().unwrap();
            fs::create_dir(root.path().join("private")).unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            let path = Path::new("private").join(name);
            let peer_path = root.path().join(&path);
            let (send, receive) = mpsc::channel();
            file_set_after_absence(Box::new(move || {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(peer_path)
                    .unwrap();
                send.send(file).unwrap();
            }));
            // When: a blocking acquisition continues after the peer's first publication.
            let guard = files.lock_acquire_by_path(&path).unwrap();
            let peer = receive
                .try_recv()
                .expect("peer publication hook did not run");
            // Then: no new inode replaces the published leaf and only ownership records PID.
            assert_eq!(
                guard.file.metadata().unwrap().ino(),
                peer.metadata().unwrap().ino()
            );
            assert_eq!(
                fs::read(root.path().join(&path)).unwrap(),
                format!("{}\n", std::process::id()).as_bytes()
            );
        }
    }

    #[test]
    fn first_directory_peer_publication_is_adopted_and_pinned() {
        // Given: a peer creates a safe directory at the first absence boundary.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("private");
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            fs::create_dir(&path).unwrap();
            send.send(File::open(path).unwrap()).unwrap();
        }));
        // When: creation resolves the peer's publication and pins the resulting directory.
        let directory = files
            .directory_create_by_path(Path::new("private"))
            .unwrap();
        let peer = receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: the exact published inode is retained.
        assert_eq!(
            fstat(directory).unwrap().st_ino,
            peer.metadata().unwrap().ino()
        );
        // When: a new directory later replaces the now-pinned identity.
        fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
        fs::create_dir(root.path().join("private")).unwrap();
        // Then: creation cannot silently adopt another inode after pinning.
        assert_eq!(
            files
                .directory_create_by_path(Path::new("private"))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::Changed
        );
    }

    #[test]
    fn unsafe_lock_peer_publications_fail_before_mutating_external_bytes() {
        if run_bounded(
            "filesystem::local::tests::creation_tests::unsafe_lock_peer_publications_fail_before_mutating_external_bytes",
        ) {
            return;
        }
        for entry in [
            UnsafeEntry::Symlink,
            UnsafeEntry::Fifo,
            UnsafeEntry::Hardlink,
            UnsafeEntry::Directory,
        ] {
            // Given: a peer publishes an unsafe leaf after the first actual ENOENT.
            let root = TempDir::new().unwrap();
            fs::write(root.path().join("external"), b"untouched").unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            let path = root.path().to_owned();
            let (send, receive) = mpsc::channel();
            file_set_after_absence(Box::new(move || {
                publish_entry(&path, "owner", entry);
                send.send(()).unwrap();
            }));
            // When: acquisition handles the peer's unsafe first publication.
            let error = files
                .lock_try_acquire_by_path(Path::new("owner"))
                .err()
                .unwrap();
            receive
                .try_recv()
                .expect("peer publication hook did not run");
            // Then: strict type/alias/no-follow checks fail and preserve external bytes.
            let expected = match entry {
                UnsafeEntry::Symlink => FileErrorKind::UnsafePath,
                UnsafeEntry::Fifo => FileErrorKind::NonRegular,
                UnsafeEntry::Hardlink => FileErrorKind::Aliased,
                UnsafeEntry::Directory => FileErrorKind::Io,
                UnsafeEntry::Regular => unreachable!(),
            };
            assert_eq!(error.get_kind(), expected, "{entry:?}");
            assert_eq!(
                fs::read(root.path().join("external")).unwrap(),
                b"untouched"
            );
        }
    }

    #[test]
    fn unsafe_directory_peer_publications_are_not_adopted() {
        if run_bounded(
            "filesystem::local::tests::creation_tests::unsafe_directory_peer_publications_are_not_adopted",
        ) {
            return;
        }
        for entry in [
            UnsafeEntry::Symlink,
            UnsafeEntry::Fifo,
            UnsafeEntry::Regular,
        ] {
            // Given: an unsafe entry occupies the proposed directory after first absence.
            let root = TempDir::new().unwrap();
            fs::write(root.path().join("external"), b"untouched").unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            let path = root.path().to_owned();
            let (send, receive) = mpsc::channel();
            file_set_after_absence(Box::new(move || {
                publish_entry(&path, "private", entry);
                send.send(()).unwrap();
            }));
            // When: directory creation encounters the peer publication.
            let error = files
                .directory_create_by_path(Path::new("private"))
                .unwrap_err();
            receive
                .try_recv()
                .expect("peer publication hook did not run");
            // Then: creation fails closed without following or rewriting the entry.
            assert_eq!(error.get_kind(), FileErrorKind::UnsafePath, "{entry:?}");
            assert_eq!(
                fs::read(root.path().join("external")).unwrap(),
                b"untouched"
            );
        }
    }

    #[test]
    fn ordinary_file_observations_keep_strict_absence_confirmation() {
        for operation in [
            Observation::Bytes,
            Observation::Stat,
            Observation::Identity,
            Observation::Exists,
            Observation::LockOwner,
        ] {
            // Given: a regular entry appears during a non-creating absence observation.
            let root = TempDir::new().unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            let path = root.path().join("item");
            file_set_after_absence(Box::new(move || fs::write(path, b"peer").unwrap()));
            // When: an ordinary reader, identity/presence query or lock observer runs.
            let error = get_observation_error(&files, operation);
            // Then: appearance remains Changed rather than a silently adopted observation.
            assert_eq!(error.get_kind(), FileErrorKind::Changed, "{operation:?}");
            assert_eq!(fs::read(root.path().join("item")).unwrap(), b"peer");
        }
    }

    #[test]
    fn ordinary_directory_observation_keeps_strict_absence_confirmation() {
        // Given: a directory appears while a read-only directory query confirms absence.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("private");
        file_set_after_absence(Box::new(move || fs::create_dir(path).unwrap()));
        // When: the non-creating directory observation runs.
        let error = files
            .directory_get_by_path(Path::new("private"))
            .unwrap_err();
        // Then: ordinary observation keeps the strict Changed classification.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
    }

    #[test]
    fn pinned_directory_disappearance_and_reappearance_remain_changed() {
        for pinned in ["private", "private/child"] {
            // Given: a pin or pinned descendant exists before its ancestor disappears.
            let root = TempDir::new().unwrap();
            fs::create_dir_all(root.path().join(pinned)).unwrap();
            let files = LocalFileSystem::new(root.path()).unwrap();
            files
                .directory_get_by_path(Path::new(pinned))
                .unwrap()
                .unwrap();
            fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
            let path = root.path().to_owned();
            let (send, receive) = mpsc::channel();
            file_set_after_absence(Box::new(move || {
                fs::rename(path.join("saved"), path.join("private")).unwrap();
                send.send(()).unwrap();
            }));
            // When: the same inode reappears during creation's first absence observation.
            let error = files
                .directory_create_by_path(Path::new(pinned))
                .unwrap_err();
            receive
                .try_recv()
                .expect("peer publication hook did not run");
            // Then: even same-identity reappearance cannot erase observed pin disappearance.
            assert_eq!(error.get_kind(), FileErrorKind::Changed, "{pinned}");
            assert!(root.path().join(pinned).is_dir());
        }
    }

    #[test]
    fn observed_unpinned_ancestor_disappearance_and_return_is_changed() {
        // Given: an unpinned ancestor is present when the retained parent observes it.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        let inode = fs::metadata(root.path().join("a")).unwrap().ino();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        directory_set_before_nested_preflight(Box::new(move || {
            fs::rename(path.join("a"), path.join("saved")).unwrap();
        }));
        let path = root.path().to_owned();
        file_set_after_absence(Box::new(move || {
            assert!(!path.join("a").exists());
            assert_eq!(fs::metadata(path.join("saved")).unwrap().ino(), inode);
            fs::rename(path.join("saved"), path.join("a")).unwrap();
        }));
        // When: the observed ancestor disappears and returns during child preflight.
        let result = files.directory_create_by_path(Path::new("a/b"));
        // Then: the same inode returns, but no child is created and disappearance stays Changed.
        assert_eq!(fs::metadata(root.path().join("a")).unwrap().ino(), inode);
        assert!(
            !root.path().join("a/b").exists(),
            "child created after ancestor disappearance"
        );
        assert_eq!(result.unwrap_err().get_kind(), FileErrorKind::Changed);
    }

    fn publish_entry(root: &Path, name: &str, entry: UnsafeEntry) {
        let path = root.join(name);
        match entry {
            UnsafeEntry::Symlink => symlink("external", path).unwrap(),
            UnsafeEntry::Fifo => create_fifo(&path),
            UnsafeEntry::Hardlink => fs::hard_link(root.join("external"), path).unwrap(),
            UnsafeEntry::Directory => fs::create_dir(path).unwrap(),
            UnsafeEntry::Regular => fs::write(path, b"peer").unwrap(),
        }
    }

    fn get_observation_error(files: &LocalFileSystem, operation: Observation) -> FileError {
        let path = Path::new("item");
        match operation {
            Observation::Bytes => files.file_get_by_path(path).unwrap_err(),
            Observation::Stat => files.file_get_stat_by_path(path).unwrap_err(),
            Observation::Identity => files.file_get_identity_by_path(path).unwrap_err(),
            Observation::Exists => files.file_is_exist(path).unwrap_err(),
            Observation::LockOwner => files.lock_get_owner_by_path(path).unwrap_err(),
        }
    }
}

#[test]
fn no_clobber_publication_preserves_winner_and_private_modes() {
    // Given: a writable root and arbitrary caller-selected paths.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files
        .directory_create_by_path(Path::new("private"))
        .unwrap();
    // When: two complete values are offered for the same file.
    files
        .file_create_if_absent_by_path(Path::new("private/item"), b"winner")
        .unwrap();
    files
        .file_create_if_absent_by_path(Path::new("private/item"), b"loser")
        .unwrap();
    files.directory_sync_by_path(Path::new("private")).unwrap();
    // Then: the first bytes survive, private modes hold and no temporary entries remain.
    assert_eq!(
        fs::read(root.path().join("private/item")).unwrap(),
        b"winner"
    );
    assert_eq!(
        fs::metadata(root.path().join("private"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.path().join("private/item"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_dir(root.path().join("private")).unwrap().count(),
        1
    );
}

#[test]
fn file_sync_failure_preserves_absence_and_retry_publishes_complete_bytes() {
    use super::{SyncTarget, file_fail_next_sync};
    // Given: an absent destination and an injected file-data sync failure.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    file_fail_next_sync(SyncTarget::File);
    // When: durable publication fails before linking the destination.
    let error = files
        .file_create_if_absent_by_path(Path::new("item"), b"complete")
        .unwrap_err();
    // Then: no destination or temporary file survives; a clean retry succeeds.
    assert_eq!(error.get_kind(), FileErrorKind::Io);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    files
        .file_create_if_absent_by_path(Path::new("item"), b"complete")
        .unwrap();
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"complete");
}

#[test]
fn directory_sync_failure_preserves_published_winner_for_retry() {
    use super::{SyncTarget, file_fail_next_sync};
    // Given: an absent destination and an injected post-publication directory failure.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    file_fail_next_sync(SyncTarget::Directory);
    // When: bytes publish but directory durability cannot be confirmed.
    assert_eq!(
        files
            .file_create_if_absent_by_path(Path::new("item"), b"winner")
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Io
    );
    // Then: retry syncs the first complete value without overwriting or leaking temporaries.
    files
        .file_create_if_absent_by_path(Path::new("item"), b"loser")
        .unwrap();
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"winner");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_publications_preserve_one_complete_winner() {
    // Given: two independent components publish different complete byte sequences.
    let root = TempDir::new().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = [b'A', b'B']
        .into_iter()
        .map(|byte| {
            let path = root.path().to_owned();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let files = LocalFileSystem::new(&path).unwrap();
                barrier.wait();
                files
                    .file_create_if_absent_by_path(Path::new("item"), &vec![byte; 4096])
                    .unwrap();
            })
        })
        .collect();
    // When: both publication protocols race for the same absent destination.
    for worker in workers {
        worker.join().unwrap();
    }
    // Then: one complete value survives with no temporary entries.
    let bytes = fs::read(root.path().join("item")).unwrap();
    assert!(bytes == vec![b'A'; 4096] || bytes == vec![b'B'; 4096]);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn unsafe_publication_leaves_and_missing_parents_are_preserved() {
    // Given: caller-selected directory and symlink destinations plus an absent parent.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join("external"), b"untouched").unwrap();
    symlink("external", root.path().join("link")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: publication rejects unsupported paths without replacing anything.
    for path in ["folder", "link"] {
        assert_eq!(
            files
                .file_create_if_absent_by_path(Path::new(path), b"loser")
                .unwrap_err()
                .get_kind(),
            FileErrorKind::NonRegular
        );
    }
    assert_eq!(
        files
            .file_create_if_absent_by_path(Path::new("missing/item"), b"loser")
            .unwrap_err()
            .get_kind(),
        FileErrorKind::NotFound
    );
    assert_eq!(
        fs::read(root.path().join("external")).unwrap(),
        b"untouched"
    );
    assert!(!root.path().join("missing").exists());
}

#[test]
fn configuration_reads_without_sampling_wall_clock() {
    // Given: a valid unconfigured file and a clock hook that fails on any sample.
    use crate::service::{ConfigReader, ConfigurationService};
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join(".letopis")).unwrap();
    fs::write(root.path().join(".letopis/config.json"), br#"{"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0}}"#).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files
        .before_clock
        .replace(Some(Box::new(|| panic!("unexpected wall-clock sample"))));
    let provider = crate::embedding::HttpProviders::new(None);
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };
    // When: configuration is read and parsed.
    let configuration = service.get_config().unwrap();
    // Then: bytes are available without consulting the clock.
    assert!(
        configuration
            .as_ref()
            .is_some_and(|config| !config.is_enabled() && config.get_embedding_config().is_none())
    );
}

#[test]
fn bytes_read_ignores_unrepresentable_index_timestamp() {
    // Given: a file whose verified metadata is represented with an out-of-range mtime.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"exact bytes").unwrap();
    let mut files = LocalFileSystem::new(root.path()).unwrap();
    files.mtime_seconds_override = Some(10_000_000_000);
    // When: the two read operations observe the same synthetic platform timestamp.
    let bytes = files.file_get_bytes_by_path(Path::new("item")).unwrap();
    let captured_error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: byte-only use succeeds while indexed metadata fails without saturation.
    assert_eq!(bytes.as_deref(), Some(&b"exact bytes"[..]));
    assert_eq!(captured_error.get_kind(), FileErrorKind::Io);
}

#[test]
fn bytes_read_rejects_content_changed_after_reading() {
    // Given: a regular file changed immediately after its bytes are read.
    let root = TempDir::new().unwrap();
    let path = root.path().join("item");
    fs::write(&path, b"first").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files.after_read.replace(Some(Box::new(move || {
        fs::write(path, b"changed content").unwrap();
    })));
    // When: the byte-only operation reads the changing file.
    let error = files.file_get_bytes_by_path(Path::new("item")).unwrap_err();
    // Then: changed content is rejected rather than returned as stable.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
}

#[test]
fn bytes_read_never_follows_leaf_or_parent_symlinks() {
    // Given: a symlink leaf and a symlink parent pointing at real entries.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("directory")).unwrap();
    fs::write(root.path().join("directory/item"), b"secret").unwrap();
    symlink("directory/item", root.path().join("link")).unwrap();
    symlink("directory", root.path().join("parent")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When: byte-only reads encounter each symlink.
    let leaf_error = files.file_get_bytes_by_path(Path::new("link")).unwrap_err();
    let parent_error = files
        .file_get_bytes_by_path(Path::new("parent/item"))
        .unwrap_err();
    // Then: neither destination is read and each error retains its classification.
    assert_eq!(leaf_error.get_kind(), FileErrorKind::NonRegular);
    assert_eq!(parent_error.get_kind(), FileErrorKind::UnsafePath);
}

#[test]
fn bytes_read_rejects_symlink_replacing_the_observed_leaf() {
    // Given: a regular file replaced with a symlink to that same inode before opening.
    let root = TempDir::new().unwrap();
    let original = root.path().join("item");
    let moved = root.path().join("moved");
    fs::write(&original, b"secret").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files.before_open.replace(Some(Box::new(move || {
        fs::rename(&original, &moved).unwrap();
        symlink("moved", original).unwrap();
    })));
    // When: the byte-only reader opens the replaced entry.
    let error = files.file_get_bytes_by_path(Path::new("item")).unwrap_err();
    // Then: the no-follow open rejects the replacement.
    assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
}

#[test]
fn bytes_read_rejects_ancestor_replaced_during_reading() {
    // Given: a file whose ancestor is replaced immediately after reading its bytes.
    let root = TempDir::new().unwrap();
    let directory = root.path().join("directory");
    let moved = root.path().join("moved");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("item"), b"content").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files.after_read.replace(Some(Box::new(move || {
        fs::rename(&directory, moved).unwrap();
        fs::create_dir(directory).unwrap();
    })));
    // When: the byte-only operation reads under the replaced ancestor.
    let error = files
        .file_get_bytes_by_path(Path::new("directory/item"))
        .unwrap_err();
    // Then: no bytes escape from an unverified ancestor.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
}

#[test]
fn indexed_capture_samples_clock_before_reading_bytes() {
    // Given: hooks recording the clock sample and checking it at the end of the read.
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"content").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let sampled = Arc::new(AtomicBool::new(false));
    let on_clock = Arc::clone(&sampled);
    files.before_clock.replace(Some(Box::new(move || {
        on_clock.store(true, Ordering::SeqCst);
    })));
    let after_read = Arc::clone(&sampled);
    files.after_read.replace(Some(Box::new(move || {
        assert!(after_read.load(Ordering::SeqCst));
    })));
    // When: an indexed capture reads the document.
    let captured = files.file_get_by_path(Path::new("item")).unwrap().unwrap();
    // Then: the clock was sampled before the bytes and its result is retained.
    assert_eq!(captured.bytes, b"content");
    assert!(captured.checked_at_ns > 0);
}

#[test]
fn observes_absent_and_present_files_without_creating_entries() {
    // Given: a root containing one ordinary file and no private state.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"exact bytes").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let reader: &dyn FileReader = &files;
    // When: generic operations observe missing and present paths.
    assert!(!reader.file_is_exist(Path::new("missing/leaf")).unwrap());
    assert!(
        reader
            .file_get_by_path(Path::new("missing"))
            .unwrap()
            .is_none()
    );
    let captured = reader.file_get_by_path(Path::new("item")).unwrap().unwrap();
    let missing_bytes = reader.file_get_bytes_by_path(Path::new("missing")).unwrap();
    let present_bytes = reader.file_get_bytes_by_path(Path::new("item")).unwrap();
    // Then: bytes and stat agree and observations create no entries.
    assert_eq!(missing_bytes, None);
    assert_eq!(present_bytes.as_deref(), Some(&b"exact bytes"[..]));
    assert_eq!(captured.bytes, b"exact bytes");
    assert_eq!(captured.stat.size, 11);
    assert!(reader.file_is_exist(Path::new("item")).unwrap());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn distinguishes_nonregular_leaves_from_unsafe_parent_paths() {
    // Given: stable directory and symlink leaves plus a symlink parent.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join("item"), b"private").unwrap();
    symlink("item", root.path().join("link")).unwrap();
    symlink("folder", root.path().join("parent")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: leaves can be excluded by callers, while parents fail closed.
    for path in ["folder", "link"] {
        assert_eq!(
            files.file_is_exist(Path::new(path)).unwrap_err().get_kind(),
            FileErrorKind::NonRegular
        );
    }
    for path in ["parent/item", "../item", "/item", "item/leaf"] {
        assert_eq!(
            files.file_is_exist(Path::new(path)).unwrap_err().get_kind(),
            FileErrorKind::UnsafePath
        );
    }
}

#[test]
fn identity_is_metadata_only_and_ignores_content_updates() {
    // Given: one file, a hardlink alias and a distinct file.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("one"), b"first").unwrap();
    fs::hard_link(root.path().join("one"), root.path().join("alias")).unwrap();
    fs::write(root.path().join("two"), b"second").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let identity = files
        .file_get_stat_by_path(Path::new("one"))
        .unwrap()
        .unwrap()
        .get_identity();
    // When: bytes and size change without replacing the inode.
    fs::write(root.path().join("one"), b"updated bytes with another size").unwrap();
    fs::set_permissions(root.path().join("one"), fs::Permissions::from_mode(0o0)).unwrap();
    // Then: metadata works without body access; aliases remain detectable.
    assert!(files.file_is_exist(Path::new("one")).unwrap());
    files
        .file_verify_identity_by_path(Path::new("one"), Some(identity))
        .unwrap();
    files
        .file_verify_distinct_by_paths(Path::new("one"), Path::new("two"))
        .unwrap();
    assert_eq!(
        files
            .file_verify_distinct_by_paths(Path::new("one"), Path::new("alias"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Aliased
    );
}

#[test]
fn explicit_directory_pins_reject_replacement_without_creating_missing_paths() {
    // Given: an explicitly pinned existing directory.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    assert!(
        files
            .directory_get_by_path(Path::new("missing"))
            .unwrap()
            .is_none()
    );
    let pinned = files
        .directory_get_by_path(Path::new("private"))
        .unwrap()
        .unwrap();
    // When: the original directory is moved away and another takes its name.
    fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    // Then: both directory and child operations reject the changed identity.
    assert_eq!(
        files
            .directory_get_by_path(Path::new("private"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert_eq!(
        files
            .file_is_exist(Path::new("private/item"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert!(rustix::fs::fstat(pinned).is_ok());
    assert!(!root.path().join("missing").exists());
}

#[test]
fn identity_verification_rejects_replaced_or_deleted_files() {
    // Given: a file identity observed before an external operation.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let identity = files
        .file_get_stat_by_path(Path::new("item"))
        .unwrap()
        .unwrap()
        .get_identity();
    // When: a different file takes the same name.
    fs::rename(root.path().join("item"), root.path().join("saved")).unwrap();
    fs::write(root.path().join("item"), b"replacement").unwrap();
    // Then: replacement and subsequent deletion both fail identity checks.
    assert_eq!(
        files
            .file_verify_identity_by_path(Path::new("item"), Some(identity))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    fs::remove_file(root.path().join("item")).unwrap();
    assert_eq!(
        files
            .file_verify_identity_by_path(Path::new("item"), Some(identity))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    files
        .file_verify_identity_by_path(Path::new("new"), None)
        .unwrap();
}

#[test]
fn preserves_real_io_errors_and_safe_diagnostics() {
    // Given: opening a missing root fails with an operating-system cause.
    let root = TempDir::new().unwrap();
    // When: construction attempts a nonexistent directory.
    let error = LocalFileSystem::new(&root.path().join("secret-missing"))
        .err()
        .unwrap();
    // Then: typed failure retains its cause without disclosing caller paths.
    assert_eq!(error.get_kind(), FileErrorKind::Io);
    assert!(error.source().is_some());
    assert!(!format!("{error:?} {error}").contains("secret-missing"));
}

#[test]
fn existence_and_identity_never_enter_the_body_read_protocol() {
    // Given: a regular file and read hooks that fail if a body is opened or consumed.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"untouched").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files
        .before_open
        .replace(Some(Box::new(|| panic!("body opened"))));
    files
        .after_read
        .replace(Some(Box::new(|| panic!("body read"))));
    // When: metadata-only operations check presence and identity.
    assert!(files.file_is_exist(Path::new("item")).unwrap());
    let identity = files.file_get_identity_by_path(Path::new("item")).unwrap();
    files
        .file_verify_identity_by_path(Path::new("item"), identity)
        .unwrap();
    // Then: both read hooks remain unused and bytes remain unchanged.
    assert!(files.before_open.take().is_some());
    assert!(files.after_read.take().is_some());
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"untouched");
}

#[test]
fn generic_reads_reject_leaf_and_ancestor_replacement() {
    for ancestor in [false, true] {
        // Given: an ordinary file below a directory that another actor can rename.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("folder/item"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        files.after_read.replace(Some(Box::new(move || {
            if ancestor {
                fs::rename(path.join("folder"), path.join("saved")).unwrap();
                fs::create_dir(path.join("folder")).unwrap();
            } else {
                fs::rename(path.join("folder/item"), path.join("folder/saved")).unwrap();
                fs::write(path.join("folder/item"), b"replacement").unwrap();
            }
        })));
        // When: a path is replaced after bytes are read but before verification.
        let error = files
            .file_get_by_path(Path::new("folder/item"))
            .unwrap_err();
        // Then: no stale capture escapes the operation.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
    }
}

#[test]
fn identity_ignores_content_races_but_metadata_requires_stable_content() {
    for identity_only in [true, false] {
        // Given: a file that will change bytes between metadata observations.
        let root = TempDir::new().unwrap();
        let path = root.path().join("item");
        fs::write(&path, b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        files.between_stats.replace(Some(Box::new(move || {
            fs::write(path, b"changed size").unwrap();
        })));
        // When / Then: identity remains valid while a full metadata capture fails.
        if identity_only {
            assert!(
                files
                    .file_get_identity_by_path(Path::new("item"))
                    .unwrap()
                    .is_some()
            );
        } else {
            assert_eq!(
                files
                    .file_get_stat_by_path(Path::new("item"))
                    .unwrap_err()
                    .get_kind(),
                FileErrorKind::Changed
            );
        }
    }
}

#[test]
fn nofollow_rejects_symlink_even_when_it_targets_the_original_inode() {
    // Given: an observed regular file will be renamed and replaced by its symlink.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.before_open.replace(Some(Box::new(move || {
        fs::rename(path.join("item"), path.join("saved")).unwrap();
        symlink("saved", path.join("item")).unwrap();
    })));
    // When: the descriptor open crosses the leaf replacement.
    let error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: the no-follow syscall rejects the symlink before its target is read.
    assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
    assert!(error.source().is_some());
}

#[test]
fn external_path_resolution_rejects_aliases_and_preserves_io_failures() {
    // Given: ordinary external paths, a hardlink alias and an invalid parent.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("one"), b"unchanged").unwrap();
    fs::hard_link(root.path().join("one"), root.path().join("alias")).unwrap();
    // When / Then: missing leaves resolve, aliases and unsafe parents fail precisely.
    assert!(
        LocalFileSystem::file_get_resolved_path_by_path(&root.path().join("missing"))
            .unwrap()
            .is_absolute()
    );
    assert_eq!(
        LocalFileSystem::file_get_distinct_paths_by_paths(
            &root.path().join("one"),
            &root.path().join("alias")
        )
        .unwrap_err()
        .get_kind(),
        FileErrorKind::Aliased
    );
    assert_eq!(
        LocalFileSystem::file_get_resolved_path_by_path(&root.path().join("one/child"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::UnsafePath
    );
    assert_eq!(
        LocalFileSystem::file_get_resolved_path_by_path(Path::new("bad\0name"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::UnsafePath
    );
    assert_eq!(
        LocalFileSystem::file_get_resolved_path_by_path(root.path())
            .unwrap_err()
            .get_kind(),
        FileErrorKind::NonRegular
    );
}

#[test]
fn disappearance_of_a_pinned_ancestor_never_recreates_it() {
    // Given: a nested directory has an explicitly retained identity.
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("outer/private")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files
        .directory_get_by_path(Path::new("outer/private"))
        .unwrap()
        .unwrap();
    fs::rename(root.path().join("outer"), root.path().join("saved")).unwrap();
    // When / Then: missing ancestry is replacement, including on creation paths.
    assert_eq!(
        files
            .file_is_exist(Path::new("outer/private/item"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert_eq!(
        files
            .directory_create_by_path(Path::new("outer/private"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert!(!root.path().join("outer").exists());
}

#[test]
fn external_resolution_preserves_existing_symlink_canonicalization() {
    // Given: a file and an author-supplied symlink to it.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"unchanged").unwrap();
    symlink("item", root.path().join("link")).unwrap();
    // When: an external path is explicitly resolved before confined composition.
    let resolved =
        LocalFileSystem::file_get_resolved_path_by_path(&root.path().join("link")).unwrap();
    // Then: resolution selects the canonical regular target and detects its alias.
    assert_eq!(resolved, root.path().canonicalize().unwrap().join("item"));
    assert_eq!(
        LocalFileSystem::file_get_distinct_paths_by_paths(
            &root.path().join("link"),
            &root.path().join("item")
        )
        .unwrap_err()
        .get_kind(),
        FileErrorKind::Aliased
    );
}

#[test]
fn regular_leaf_replaced_by_directory_before_open_is_a_change() {
    // Given: a regular leaf will become a directory at the descriptor-open boundary.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.before_open.replace(Some(Box::new(move || {
        fs::rename(path.join("item"), path.join("saved")).unwrap();
        fs::create_dir(path.join("item")).unwrap();
    })));
    // When: capture opens the replacement without following any symlink.
    let error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: a changed observation cannot be mapped to a stable document exclusion.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert_eq!(fs::read(root.path().join("saved")).unwrap(), b"original");
}

#[test]
fn nonregular_leaf_replacement_between_observations_is_a_change() {
    // Given: a stable-looking directory leaf will become a regular file.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("item")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.between_stats.replace(Some(Box::new(move || {
        fs::rename(path.join("item"), path.join("saved")).unwrap();
        fs::write(path.join("item"), b"replacement").unwrap();
    })));
    // When: capture checks whether the nonregular leaf is a stable exclusion.
    let error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: the leaf replacement is reported rather than silently excluded.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"replacement");
}

#[test]
fn nonregular_observation_rejects_parent_replacement() {
    // Given: a nonregular leaf's parent will be replaced between its observations.
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("folder/item")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.between_stats.replace(Some(Box::new(move || {
        fs::rename(path.join("folder"), path.join("saved")).unwrap();
        fs::create_dir_all(path.join("folder/item")).unwrap();
    })));
    // When: capture observes the still-open original directory descriptor.
    let error = files
        .file_get_by_path(Path::new("folder/item"))
        .unwrap_err();
    // Then: changed ancestry is rejected even when the leaf itself has not changed.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(root.path().join("saved/item").is_dir());
}

#[test]
fn byte_capture_classifies_only_stable_nonregular_leaves_as_exclusions() {
    // Given: a directory and a symlink remain stable throughout capture observation.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    symlink("folder", root.path().join("link")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: stable unsupported leaves retain the explicit exclusion classification.
    for path in ["folder", "link"] {
        assert_eq!(
            files
                .file_get_by_path(Path::new(path))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::NonRegular
        );
    }
}

#[test]
fn represents_signed_nanosecond_boundaries() {
    // Given: platform metadata with the exact normalized i64 nanosecond boundaries.
    let file = File::open(".").unwrap();
    let mut stat = fstat(&file).unwrap();
    for (seconds, nanos, expected) in [
        (-9_223_372_037, 145_224_192, i64::MIN),
        (9_223_372_036, 854_775_807, i64::MAX),
    ] {
        stat.st_mtime = seconds;
        stat.st_mtime_nsec = nanos;
        // When: the index metadata is constructed.
        let result = file_build_stat(&stat).unwrap();
        // Then: the exact representable endpoint is retained.
        assert_eq!(result.mtime_ns, expected);
    }
}

#[test]
fn rejects_nanoseconds_outside_signed_range() {
    // Given: platform metadata one nanosecond beyond each representable endpoint.
    let file = File::open(".").unwrap();
    let mut stat = fstat(&file).unwrap();
    for (seconds, nanos) in [(-9_223_372_037, 145_224_191), (9_223_372_036, 854_775_808)] {
        stat.st_mtime = seconds;
        stat.st_mtime_nsec = nanos;
        // When: the index metadata is constructed.
        let error = file_build_stat(&stat).unwrap_err();
        // Then: out-of-range metadata is an I/O error, never a saturated timestamp.
        assert_eq!(error.get_kind(), FileErrorKind::Io);
    }
}

#[test]
fn rejects_each_independent_stat_change() {
    // Given: real platform metadata with one independent field changed in each copy.
    let file = File::open(".").unwrap();
    let before = fstat(&file).unwrap();
    let mut changes = [before; 8];
    changes[0].st_size += 1;
    changes[1].st_mtime += 1;
    changes[2].st_mtime_nsec += 1;
    changes[3].st_ctime += 1;
    changes[4].st_ctime_nsec += 1;
    changes[5].st_mode ^= 0o100;
    changes[6].st_ino ^= 1;
    changes[7].st_dev ^= 1;
    // When / Then: identical metadata succeeds and every independent mutation fails.
    assert!(file_verify_unchanged(&before, &before).is_ok());
    for after in changes {
        assert_eq!(
            file_verify_unchanged(&before, &after)
                .unwrap_err()
                .get_kind(),
            FileErrorKind::Changed
        );
    }
}

#[test]
fn concurrent_creation_after_absent_observation_reopens_the_same_inode() {
    for name in ["embed.lock", "index.lock"] {
        // Given: a second opener exactly after the first observed absence.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        let peer = LocalFileSystem::new(&fixture.0).unwrap();
        let path = Path::new(".letopis").join(name);
        let peer_path = path.clone();
        let (send, receive) = std::sync::mpsc::channel();
        files.before_open.replace(Some(Box::new(move || {
            let (_, _, file) = lock_get_file_by_path(&peer, &peer_path).unwrap();
            send.send(file).unwrap();
        })));
        // When: CREATE|EXCL opens the competitor's persistent inode.
        let (_, _, file) = lock_get_file_by_path(&files, &path).unwrap();
        let peer_file = receive.recv().unwrap();
        // Then: ownership identity is shared; neither opener unlinks/replaces it.
        let inode = file.metadata().unwrap().ino();
        assert_eq!(peer_file.metadata().unwrap().ino(), inode);
        assert_eq!(
            fs::metadata(fixture.0.join(".letopis").join(name))
                .unwrap()
                .ino(),
            inode
        );
    }
}

#[test]
fn owner_drop_unlocks_while_duplicate_descriptor_remains_open() {
    if run_bounded(
        "filesystem::local::tests::owner_drop_unlocks_while_duplicate_descriptor_remains_open",
    ) {
        return;
    }
    // Given: a held lock with a duplicate file descriptor.
    let fixture = Fixture::new();
    let owner = LocalFileSystem::new(&fixture.0).unwrap();
    let contender = LocalFileSystem::new(&fixture.0).unwrap();
    let owner_guard = owner
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let duplicate = owner_guard.file.try_clone().unwrap();
    let inode = duplicate.metadata().unwrap().ino();
    assert!(
        contender
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    // When / Then: closure of the last fd cannot make this pass.
    drop(owner_guard);
    assert!(
        contender
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_some()
    );
    assert_eq!(duplicate.metadata().unwrap().ino(), inode);
    assert_eq!(
        fs::metadata(fixture.0.join(".letopis/embed.lock"))
            .unwrap()
            .ino(),
        inode
    );
}

#[test]
fn lock_open_rejects_replacement_and_never_recreates_observed_deletion() {
    if run_bounded(
        "filesystem::local::tests::lock_open_rejects_replacement_and_never_recreates_observed_deletion",
    ) {
        return;
    }
    for action in ["regular", "symlink", "fifo", "delete"] {
        // Given: an existing lock and a process waiting to replace or delete it.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();
        fs::write(fixture.0.join(".letopis/embed.lock"), b"old owner").unwrap();
        let mut mutator = fixture.spawn_mutator(".letopis/embed.lock", action);
        // When: the mutation runs between observation and open.
        files.before_open.replace(Some(Box::new(move || {
            mutator.resume();
            mutator.finish();
        })));
        let result = lock_get_file_by_path(&files, Path::new(".letopis/embed.lock"));
        // Then: opening fails and an observed deletion remains absent.
        assert!(result.is_err(), "{action}");
        if action == "delete" {
            assert!(!fixture.0.join(".letopis/embed.lock").exists());
        }
    }
}

#[test]
fn status_lock_open_rejects_inode_replacement() {
    // Given: an existing owner file with a replacement hook.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(std::path::Path::new(".letopis"))
        .unwrap();
    let path = fixture.0.join(".letopis/index.lock");
    fs::write(&path, b"old owner").unwrap();

    // When: observation opens after the name has changed.
    let changed_path = path.clone();
    let old_path = fixture.0.join("old-index.lock");
    files.before_open.replace(Some(Box::new(move || {
        let path = changed_path;
        fs::rename(&path, old_path).unwrap();
        fs::write(&path, b"new owner").unwrap();
    })));
    let result = files.lock_get_owner_by_path(Path::new(".letopis/index.lock"));

    // Then: the replacement is rejected and its bytes are not modified.
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"new owner");
}

#[test]
fn retries_interrupted_io_until_the_operation_succeeds() {
    // Given: an interrupted syscall followed by success.
    let mut results = [Err(ErrorKind::Interrupted.into()), Ok(())].into_iter();
    // When: the file retry helper executes the sequence.
    let result = file_retry_io(|| results.next().expect("unexpected extra retry"));
    // Then: only the interrupted result is retried.
    assert!(result.is_ok());
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_io_errors() {
    // Given: a permission failure with no retryable result.
    let mut results = [Err(ErrorKind::PermissionDenied.into())].into_iter();
    // When: the file retry helper runs.
    let error =
        file_retry_io(|| results.next().expect("nonretryable error was retried")).unwrap_err();
    // Then: the original permission failure returns without another call.
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}

#[test]
fn rejects_each_prior_lock_identity_mismatch_before_writing_pid() {
    for change_inode in [true, false] {
        // Given: hold a real lock, then change exactly one prior observation field.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        let (parent, name, file) =
            lock_get_file_by_path(&files, Path::new(".letopis/embed.lock")).unwrap();
        let mut before = file_get_lock_stat(&file).unwrap();
        if change_inode {
            let (_, _, other) =
                lock_get_file_by_path(&files, Path::new(".letopis/index.lock")).unwrap();
            before.st_ino = file_get_lock_stat(&other).unwrap().st_ino;
        } else {
            before.st_dev ^= 1;
        }
        let path = fixture.0.join(".letopis/embed.lock");
        fs::write(&path, b"previous owner").unwrap();
        file.try_lock().unwrap();
        let duplicate = file.try_clone().unwrap();
        // When: lock completion compares the prior and current file identities.
        let result = lock_finish_acquisition(&files, &parent, &name, file, before);
        // Then: failed completion releases ownership even with another fd open.
        let error = result.expect_err("identity mismatch was accepted");
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert_eq!(fs::read(&path).unwrap(), b"previous owner");
        let contender = LocalFileSystem::new(&fixture.0).unwrap();
        assert!(
            contender
                .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                .unwrap()
                .is_some()
        );
        drop(duplicate);
    }
}

#[test]
fn retries_interrupted_lock_acquisition_until_success() {
    // Given: an interrupted lock syscall followed by success.
    let mut results = [
        Err(TryLockError::Error(ErrorKind::Interrupted.into())),
        Ok(()),
    ]
    .into_iter();
    // When: the lock retry helper executes the sequence.
    let acquired =
        lock_retry_acquisition(|| results.next().expect("unexpected extra retry")).unwrap();
    // Then: acquisition succeeds after exactly one retry.
    assert!(acquired);
    assert!(results.next().is_none());
}

#[test]
fn returns_contention_without_retrying() {
    // Given: a nonblocking lock syscall reports contention.
    let mut results = [Err(TryLockError::WouldBlock)].into_iter();
    // When: the lock helper inspects that outcome.
    let acquired =
        lock_retry_acquisition(|| results.next().expect("contention was retried")).unwrap();
    // Then: contention returns false without retrying.
    assert!(!acquired);
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_lock_errors() {
    // Given: a permission failure from the lock syscall.
    let mut results = [Err(TryLockError::Error(ErrorKind::PermissionDenied.into()))].into_iter();
    // When: the lock retry helper runs.
    let error =
        lock_retry_acquisition(|| results.next().expect("nonretryable lock error was retried"))
            .unwrap_err();
    // Then: the original failure returns without another attempt.
    let TryLockError::Error(cause) = error else {
        panic!("expected the original nonretryable I/O error");
    };
    assert_eq!(cause.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}

#[test]
fn missing_creation_reopens_existing_lock_without_replacing_it() {
    for name in ["index.lock", "embed.lock"] {
        // Given: a competitor created the lock file before the current creation attempt.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(name);
        fs::write(&path, b"persistent owner state").unwrap();
        let directory = fs::File::open(root.path()).unwrap();
        let before = fs::metadata(&path).unwrap().ino();
        // When: select exactly the ENOENT creation-error path.
        let reopened =
            file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::NOENT)).unwrap();
        // Then: identity and bytes remain intact, and unrelated errors do not trigger fallback.
        assert_eq!(rustix::fs::fstat(&reopened).unwrap().st_ino, before);
        assert_eq!(fs::read(&path).unwrap(), b"persistent owner state");
        assert_eq!(
            file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::ACCESS))
                .unwrap_err(),
            Errno::ACCESS
        );
    }
}

#[test]
fn missing_creation_reopen_rejects_absent_and_unsafe_lock_names() {
    for name in ["index.lock", "embed.lock"] {
        for kind in ["absent", "symlink", "fifo", "directory"] {
            // Given: an absent or unsafe name under the same live directory descriptor.
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join(name);
            let directory = fs::File::open(root.path()).unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("untouched");
            fs::write(&target, b"outside").unwrap();
            match kind {
                "symlink" => std::os::unix::fs::symlink(&target, &path).unwrap(),
                "fifo" => assert!(
                    std::process::Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success()
                ),
                "directory" => fs::create_dir(&path).unwrap(),
                _ => {}
            }
            // When: the fallback may open a FIFO, but the regular-file check rejects it.
            let result =
                file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::NOENT));
            // Then: no file is created, symlink followed, or nonregular file accepted.
            if let Ok(file) = result {
                assert!(file_get_lock_stat(&std::fs::File::from(file)).is_err());
            }
            if kind == "absent" {
                assert!(!path.exists());
            }
            assert_eq!(fs::read(&target).unwrap(), b"outside");
        }
    }
}

mod lock_tests {
    use crate::filesystem::LocalFileSystem;
    use crate::service::{FileErrorKind, FileLocker, FileReader};
    use std::{fs, path::Path};
    use tempfile::TempDir;

    #[test]
    fn lock_observation_is_read_only_and_ownership_persists_until_drop() {
        // Given: an absent arbitrary lock below an absent parent.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let observer: &dyn FileReader = &files;
        assert!(
            observer
                .lock_get_owner_by_path(Path::new("private/owner"))
                .unwrap()
                .is_none()
        );
        assert!(!root.path().join("private").exists());
        // When: acquisition creates the lock and a second acquisition contends.
        let guard = files
            .lock_acquire_by_path(Path::new("private/owner"))
            .unwrap();
        let inode = fs::metadata(root.path().join("private/owner")).unwrap();
        assert!(
            files
                .lock_try_acquire_by_path(Path::new("private/owner"))
                .unwrap()
                .is_none()
        );
        // Then: observation returns the real PID and dropping releases the same inode.
        assert_eq!(
            observer
                .lock_get_owner_by_path(Path::new("private/owner"))
                .unwrap()
                .unwrap()
                .get(),
            std::process::id()
        );
        drop(guard);
        assert!(
            observer
                .lock_get_owner_by_path(Path::new("private/owner"))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            rustix::fs::stat(root.path().join("private/owner"))
                .unwrap()
                .st_ino,
            std::os::unix::fs::MetadataExt::ino(&inode)
        );
    }

    #[test]
    fn lock_open_rejects_replacement_before_writing_owner_bytes() {
        // Given: an existing lock and a hook replacing it after initial observation.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("owner"), b"original").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        files.before_open.replace(Some(Box::new(move || {
            fs::rename(path.join("owner"), path.join("saved")).unwrap();
            fs::write(path.join("owner"), b"replacement").unwrap();
        })));
        // When: generic lock acquisition crosses the replacement boundary.
        let error = files
            .lock_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap();
        // Then: identity mismatch fails without changing either file's bytes.
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert_eq!(fs::read(root.path().join("owner")).unwrap(), b"replacement");
        assert_eq!(fs::read(root.path().join("saved")).unwrap(), b"original");
    }

    #[test]
    fn lock_parent_identity_remains_pinned_after_owner_drop() {
        // Given: generic ownership has created and observed an arbitrary parent.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        drop(
            files
                .lock_acquire_by_path(Path::new("private/owner"))
                .unwrap(),
        );
        // When: the parent is replaced after ownership is dropped.
        fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
        fs::create_dir(root.path().join("private")).unwrap();
        // Then: neither another lock nor an observational read accepts the replacement.
        assert_eq!(
            files
                .lock_try_acquire_by_path(Path::new("private/owner"))
                .err()
                .unwrap()
                .get_kind(),
            FileErrorKind::Changed
        );
        assert_eq!(
            files
                .lock_get_owner_by_path(Path::new("private/owner"))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::Changed
        );
        assert!(!root.path().join("private/owner").exists());
    }

    #[test]
    fn held_lock_rejects_invalid_bounded_pid_records_without_writing_them() {
        // Given: an externally held lock with deliberately malformed owner records.
        let root = TempDir::new().unwrap();
        let path = root.path().join("owner");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.lock().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        for record in [
            b"".as_slice(),
            b"0",
            b"-1",
            b"1x",
            b"4294967296",
            b"123456789012",
            b"1\n\n",
            b"\xff",
        ] {
            fs::write(&path, record).unwrap();
            // When: a read-only observer parses the bounded owner metadata.
            let error = files
                .lock_get_owner_by_path(Path::new("owner"))
                .unwrap_err();
            // Then: malformed records fail with a precise kind and remain unchanged.
            assert_eq!(error.get_kind(), FileErrorKind::InvalidOwner);
            assert_eq!(fs::read(&path).unwrap(), record);
        }
    }

    #[test]
    fn generic_locks_reject_hardlinks_and_preserve_external_bytes() {
        // Given: a lock path aliases another ordinary file.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("external"), b"untouched").unwrap();
        fs::hard_link(root.path().join("external"), root.path().join("owner")).unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        // When / Then: both lock acquisition and observation reject aliased lock files.
        assert_eq!(
            files
                .lock_try_acquire_by_path(Path::new("owner"))
                .err()
                .unwrap()
                .get_kind(),
            FileErrorKind::Aliased
        );
        assert_eq!(
            files
                .lock_get_owner_by_path(Path::new("owner"))
                .unwrap_err()
                .get_kind(),
            FileErrorKind::Aliased
        );
        assert_eq!(
            fs::read(root.path().join("external")).unwrap(),
            b"untouched"
        );
    }

    #[test]
    fn concurrent_first_lock_creation_has_one_owner_and_one_persistent_inode() {
        // Given: six independent components race for an absent arbitrary lock.
        let root = TempDir::new().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let path = root.path().to_owned();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let files = LocalFileSystem::new(&path).unwrap();
                    barrier.wait();
                    files
                        .lock_try_acquire_by_path(Path::new("private/owner"))
                        .unwrap()
                })
            })
            .collect();
        // When: successful guards stay alive through every attempt.
        let guards: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        // Then: only one actual owner exists and dropping it preserves the lock inode.
        assert_eq!(guards.iter().filter(|guard| guard.is_some()).count(), 1);
        let inode = rustix::fs::stat(root.path().join("private/owner"))
            .unwrap()
            .st_ino;
        drop(guards);
        assert_eq!(
            rustix::fs::stat(root.path().join("private/owner"))
                .unwrap()
                .st_ino,
            inode
        );
    }

    #[test]
    fn generic_owner_drop_unlocks_before_its_last_duplicate_closes() {
        // Given: a held generic lock has a duplicate descriptor that outlives its guard.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let guard = files.lock_acquire_by_path(Path::new("owner")).unwrap();
        let duplicate = guard.file.try_clone().unwrap();
        // When: ownership ends while that duplicate remains open.
        drop(guard);
        let next = files.lock_try_acquire_by_path(Path::new("owner")).unwrap();
        // Then: the next independently opened descriptor can acquire the same inode.
        assert!(next.is_some());
        assert_eq!(
            rustix::fs::fstat(duplicate).unwrap().st_ino,
            rustix::fs::stat(root.path().join("owner")).unwrap().st_ino
        );
    }
}
