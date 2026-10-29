use super::{BatchReader, VerifiedObject};
use crate::{git::process::GitProcess, service::SnapshotErrorKind};
use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
use tempfile::TempDir;

mod watchdog;

#[test]
fn early_broken_pipe_is_safe_and_reaps_child() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    // Given: an owned child closes stdin before the request is sent.
    if watchdog::supervise("git::batch::tests::early_broken_pipe_is_safe_and_reaps_child") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    let exe = dir.path().join("git");
    fs::write(&exe, format!("#!/usr/bin/env python3\nimport os,time\nos.close(0)\nopen({ready:?},'w').close()\ntime.sleep(60)\n")).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let process = GitProcess::new_with_executable(exe);
    let mut reader = BatchReader::open(&process, dir.path(), 40).unwrap();
    let pid = reader.child.id();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    // When: a write reaches the closed pipe.
    let error = reader
        .read_object(
            &"1".repeat(40),
            "blob",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .unwrap_err();
    drop(reader);
    // Then: the safe private cause retains BrokenPipe and the owned child is reaped.
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(error.source().unwrap().to_string().contains("BrokenPipe"));
    let mut status = 0;
    // SAFETY: waitpid receives the owned child's PID and a writable status pointer.
    assert_eq!(
        unsafe { libc::waitpid(i32::try_from(pid).unwrap(), &mut status, libc::WNOHANG) },
        -1
    );
}

#[test]
fn reads_binary_objects_without_losing_framing() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::reads_binary_objects_without_losing_framing") {
        return;
    }
    // Given: Git is the independent hash and framing oracle in both formats.
    for format in ["sha1", "sha256"] {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q", &format!("--object-format={format}")])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success()
        );
        let bytes = b"binary\n\0blob\xff";
        let oid = write_object(&dir, bytes);
        let process = GitProcess::new();
        let mut reader = BatchReader::open(&process, dir.path(), oid.len()).unwrap();
        // When: the same body is consumed twice in one child session.
        let first = reader
            .read_object(
                &oid,
                "blob",
                SnapshotErrorKind::NotFound,
                SnapshotErrorKind::InvalidRequest,
            )
            .unwrap();
        let second = reader
            .read_object(
                &oid,
                "blob",
                SnapshotErrorKind::NotFound,
                SnapshotErrorKind::InvalidRequest,
            )
            .unwrap();
        // Then: binary delimiters did not desynchronize the second response.
        assert!(matches!(first, VerifiedObject::Blob));
        assert!(matches!(second, VerifiedObject::Blob));
        reader.finish().unwrap();
    }
}

#[test]
fn returns_verified_tree_bytes() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::returns_verified_tree_bytes") {
        return;
    }
    // Given: independent empty Git tree vector.
    let oid = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let (_dir, process) = build_child(&format!("{oid} tree 0\n\n"), "");
    let mut reader = BatchReader::open(&process, std::path::Path::new("/"), 40).unwrap();
    // When: the raw tree is verified.
    let result = reader
        .read_object(
            oid,
            "tree",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .unwrap();
    // Then: callers receive exact tree bytes, not blob contents.
    let VerifiedObject::Tree(bytes) = result else {
        panic!("expected tree")
    };
    assert_eq!(bytes, b"");
    reader.finish().unwrap();
}

#[test]
fn rejects_invalid_batch_responses() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::rejects_invalid_batch_responses") {
        return;
    }
    // Given: the empty blob's independently known Git SHA-1 identity.
    let oid = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let cases = [
        (format!("{oid} missing\n"), SnapshotErrorKind::NotFound),
        (
            format!("{} blob 0\n\n", "1".repeat(40)),
            SnapshotErrorKind::InternalError,
        ),
        (
            format!("{oid} tree 0\n\n"),
            SnapshotErrorKind::InvalidRequest,
        ),
        (format!("{oid} blob -1\n"), SnapshotErrorKind::InternalError),
        (
            format!("{oid} blob 18446744073709551616\n"),
            SnapshotErrorKind::InternalError,
        ),
        (
            format!("{oid} blob 1\nx\n"),
            SnapshotErrorKind::InternalError,
        ),
        (format!("{oid} blob 0\nx"), SnapshotErrorKind::InternalError),
        (format!("{oid} blob 2\nx"), SnapshotErrorKind::InternalError),
        ("x".repeat(400), SnapshotErrorKind::InternalError),
    ];
    for (response, kind) in cases {
        let (_dir, process) = build_child(&response, "");
        let mut reader = BatchReader::open(&process, std::path::Path::new("/"), 40).unwrap();
        // When: an invalid or incomplete wire response is read.
        let error = reader
            .read_object(
                oid,
                "blob",
                SnapshotErrorKind::NotFound,
                SnapshotErrorKind::InvalidRequest,
            )
            .unwrap_err();
        // Then: typed policy survives and raw bytes stay private.
        assert_eq!(error.get_kind(), kind, "{response:?}");
        assert!(!format!("{error:?} {error}").contains("CANARY"));
    }
}

#[test]
fn stderr_pressure_does_not_block_batch() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::stderr_pressure_does_not_block_batch") {
        return;
    }
    // Given: stderr exceeds a pipe's capacity before the valid response.
    let oid = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let (_dir, process) = build_child(
        &format!("{oid} blob 0\n\n"),
        "sys.stderr.write('CANARY'*100000);sys.stderr.flush()",
    );
    let mut reader = BatchReader::open(&process, std::path::Path::new("/"), 40).unwrap();
    // When: the object is consumed and the child joined.
    reader
        .read_object(
            oid,
            "blob",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .unwrap();
    // Then: concurrent draining permits successful completion.
    reader.finish().unwrap();
}

#[test]
fn batch_child_is_reaped_on_failure_and_drop() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::batch_child_is_reaped_on_failure_and_drop") {
        return;
    }
    // Given: a malformed response followed by a child waiting for more input.
    let (dir, process) = build_child("bad\n", "");
    let mut reader = BatchReader::open(&process, std::path::Path::new("/"), 40).unwrap();
    let pid = reader.child.id();
    // When: the operation fails and releases its owned reader.
    assert!(
        reader
            .read_object(
                &"1".repeat(40),
                "blob",
                SnapshotErrorKind::NotFound,
                SnapshotErrorKind::InvalidRequest
            )
            .is_err()
    );
    drop(reader);
    // Then: waitpid reports no remaining owned child, including no zombie.
    let mut status = 0;
    // SAFETY: waitpid receives an owned child PID and a valid writable status pointer.
    assert_eq!(
        unsafe { libc::waitpid(i32::try_from(pid).unwrap(), &mut status, libc::WNOHANG) },
        -1
    );
    drop(dir);
}

#[test]
fn failed_child_exit_is_not_success() {
    // Given: a supervisor bounds protocol and child-cleanup regressions.
    if watchdog::supervise("git::batch::tests::failed_child_exit_is_not_success") {
        return;
    }
    // Given: valid framing followed by a failed Git process.
    let oid = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let (_dir, process) = build_child(&format!("{oid} blob 0\n\n"), "");
    let mut reader = BatchReader::open(&process, std::path::Path::new("/"), 40).unwrap();
    reader
        .read_object(
            oid,
            "blob",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .unwrap();
    // When: the fixture exits nonzero after stdin closes.
    fs::write(_dir.path().join("fail"), b"yes").unwrap();
    // Then: finishing propagates a safe failure.
    let error = reader.finish().unwrap_err();
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(error.source().is_some());
}

#[test]
fn raw_tree_modes_match_git_normalization() {
    // Given: raw noncanonical permissions, binary names, and both OID widths.
    for format in ["sha1", "sha256"] {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", &format!("--object-format={format}")])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let oid = write_object(&dir, b"data");
        let raw_oid: Vec<_> = (0..oid.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&oid[i..i + 2], 16).unwrap())
            .collect();
        let mut raw = Vec::new();
        for mode in [
            "100664", "100600", "100755", "120000", "40000", "160000", "000000",
        ] {
            raw.extend_from_slice(format!("{mode} name").as_bytes());
            raw.extend_from_slice(&[255, b'\n', 0]);
            raw.extend_from_slice(&raw_oid);
        }
        // When: raw entries are decoded with Git's canonical type rules.
        let entries = super::tree::read_children(&raw, oid.len()).unwrap();
        let tree = write_typed_object(&dir, &raw, "tree");
        let oracle = Command::new("git")
            .args(["ls-tree", "-z", &tree])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(oracle.status.success());
        assert_eq!(entries, read_git_children(&oracle.stdout));
        // Then: regular/symlink are blobs, directory is tree, gitlinks skipped.
        assert_eq!(
            entries,
            vec![
                (oid.clone(), "blob".into()),
                (oid.clone(), "blob".into()),
                (oid.clone(), "blob".into()),
                (oid.clone(), "blob".into()),
                (oid, "tree".into())
            ]
        );
        for broken in [
            b"100644 \0".as_slice(),
            b"oops x\0",
            b"100644 x",
            b"100644 x\0x",
        ] {
            assert!(super::tree::read_children(broken, raw_oid.len() * 2).is_err());
        }
    }
}

fn write_object(dir: &TempDir, bytes: &[u8]) -> String {
    write_typed_object(dir, bytes, "blob")
}

fn read_git_children(raw: &[u8]) -> Vec<(String, String)> {
    raw.split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let head = record.split(|b| *b == b'\t').next().unwrap();
            let fields: Vec<_> = std::str::from_utf8(head).unwrap().split(' ').collect();
            (fields[1] != "commit").then(|| (fields[2].to_owned(), fields[1].to_owned()))
        })
        .collect()
}

fn write_typed_object(dir: &TempDir, bytes: &[u8], kind: &str) -> String {
    use std::{io::Write, process::Stdio};
    let mut child = Command::new("git")
        .args(["hash-object", "--literally", "-t", kind, "-w", "--stdin"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout)
        .unwrap()
        .trim()
        .into()
}

fn build_child(response: &str, before: &str) -> (TempDir, GitProcess) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("git");
    let response = serde_json::to_string(response).unwrap();
    fs::write(&path, format!("#!/usr/bin/env python3\nimport sys,os\nsys.stdin.buffer.readline()\n{before}\nsys.stdout.buffer.write({response}.encode());sys.stdout.buffer.flush()\nsys.stdout.close()\nos.close(1)\nsys.stdin.buffer.read()\nsys.exit(1 if os.path.exists({:?}) else 0)\n",dir.path().join("fail"))).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, GitProcess::new_with_executable(path))
}
