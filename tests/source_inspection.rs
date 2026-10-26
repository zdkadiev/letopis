mod support;

use letopis::{
    domain::{LineRange, RepoPath, Sha256},
    open_git_snapshots,
    service::{CommandErrorKind, ElementAddress, GitSnapshots, Inspection, Inspector},
    wire_inspector,
};
use std::{fs, process::Command};
use support::reading::ReadingRepo;

const MIXED_NOTE: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nKnowledgeToken\n[source](source.txt \"letopis:type=external\")\n[source](source.txt#L1-L1 \"letopis:type=external\")\n[source](https://Host/x?q=1#part \"letopis:type=external\")\n";

#[test]
fn inspect_mixed_batch_uses_current_tree_bytes() {
    // Arrange: a stale index must not supply source bytes or knowledge coordinates.
    let repo = ReadingRepo::new();
    repo.write("source.txt", b"old");
    repo.write("note.md", MIXED_NOTE);
    inspect(&repo, &["note.md"]);
    repo.write("source.txt", b"abc");
    // Act
    let result = inspect(
        &repo,
        &[
            "note.md",
            "source.txt",
            "source.txt#L1-L1",
            "https://Host/x?q=1#part",
        ],
    );
    // Assert
    assert_eq!(result.elements.len(), 4);
    let knowledge = &result.elements["note.md"];
    assert_eq!(knowledge.path.as_str(), "note.md");
    assert_eq!(knowledge.lines, Some(LineRange::new(1, 5).unwrap()));
    assert_eq!(
        knowledge.hash,
        Some(
            Sha256::parse("e0e8e261083b8c8edbaefd4f06968f531205135e6b0d93f951fb635e3c71f5a8")
                .unwrap()
        )
    );
    assert_eq!(read_blob(&repo, &result, "note.md"), MIXED_NOTE);
    assert_eq!(result.elements["source.txt"].path.as_str(), "source.txt");
    let hash = Some(
        Sha256::parse("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap(),
    );
    assert_eq!(result.elements["source.txt"].hash, hash);
    assert_eq!(result.elements["source.txt#L1-L1"].hash, hash);
    assert_eq!(
        result.elements["source.txt#L1-L1"].lines,
        Some(LineRange::new(1, 1).unwrap())
    );
    let remote = &result.elements["https://Host/x?q=1#part"];
    assert_eq!(remote.path.as_str(), "https://Host/x?q=1#part");
    assert_eq!((remote.hash, remote.lines), (None, None));
    assert_eq!(read_blob(&repo, &result, "source.txt"), b"abc");
    repo.write("source.txt", b"later");
    assert_eq!(read_blob(&repo, &result, "source.txt"), b"abc");
}

#[test]
fn inspect_preserves_source_bytes_and_deduplicates_ranges() {
    // Arrange
    let repo = ReadingRepo::new();
    repo.write("source.txt", b"one\r\ntwo");
    write_references(
        &repo,
        &["source.txt", "source.txt#L1-L1", "source.txt#L2-L2"],
    );
    // Act
    let result = inspect(
        &repo,
        &[
            "./source.txt",
            "source.txt",
            "source.txt#L1-L1",
            "source.txt#L02-L02",
            "source.txt#L2-L2",
        ],
    );
    // Assert
    assert_eq!(result.elements.len(), 3);
    assert_eq!(read_blob(&repo, &result, "source.txt"), b"one\r\ntwo");
    assert_eq!(
        result.elements["source.txt#L1-L1"].hash,
        Some(
            Sha256::parse("5259d46a49644bf76792231ef7315b5293677c49ddd7e69d95557013e10320d4")
                .unwrap()
        )
    );
    assert_eq!(
        result.elements["source.txt#L2-L2"].hash,
        Some(
            Sha256::parse("3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3")
                .unwrap()
        )
    );
    assert_eq!(
        result.elements["source.txt"].hash,
        Some(
            Sha256::parse("29a776bb35efe730dabb1b1d3ad74dbf80cc3e9009e168241798ea73adca3dcf")
                .unwrap()
        )
    );
    let tree = Command::new("git")
        .current_dir(repo.get_root())
        .args(["ls-tree", "-r", "--name-only", result.snapshot.as_str()])
        .output()
        .unwrap();
    assert!(tree.status.success());
    assert_eq!(
        tree.stdout
            .split(|byte| *byte == b'\n')
            .filter(|path| *path == b"source.txt")
            .count(),
        1
    );
}

#[test]
fn inspect_distinguishes_missing_bytes_from_missing_membership() {
    // Arrange
    let repo = ReadingRepo::new();
    write_references(&repo, &["missing.txt", "short.txt#L1-L2"]);
    repo.write("short.txt", b"abc");
    // Act
    let result = inspect(&repo, &["note.md", "missing.txt", "short.txt#L1-L2"]);
    // Assert: a missing range retains its existing whole file in the tree.
    assert_eq!(result.elements["missing.txt"].hash, None);
    assert_eq!(result.elements["short.txt#L1-L2"].hash, None);
    assert_eq!(read_blob(&repo, &result, "short.txt"), b"abc");
    assert!(
        open_git_snapshots(repo.get_root())
            .unwrap()
            .read_blob(&result.snapshot, &RepoPath::parse("missing.txt").unwrap())
            .is_err()
    );
    // Act / Assert: removing the last current declaration removes membership.
    write_references(&repo, &[]);
    let ids = ["note.md", "missing.txt"].map(|id| ElementAddress::parse(id).unwrap());
    let error = wire_inspector(repo.get_root())
        .unwrap()
        .inspect(&ids)
        .unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
}

#[test]
fn inspect_includes_unrequested_declared_source_files() {
    // Arrange
    let repo = ReadingRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("unrelated.txt", b"private unrelated");
    repo.write("plain.md", b"unmarked source");
    write_references(&repo, &["source.txt", "plain.md"]);
    // Act
    let result = inspect(&repo, &["note.md"]);
    // Assert
    assert_eq!(result.elements.len(), 1);
    assert_eq!(read_blob(&repo, &result, "source.txt"), b"abc");
    assert_eq!(read_blob(&repo, &result, "plain.md"), b"unmarked source");
    let git = open_git_snapshots(repo.get_root()).unwrap();
    for path in ["unrelated.txt", ".letopis/cache.db"] {
        assert!(
            git.read_blob(&result.snapshot, &RepoPath::parse(path).unwrap())
                .is_err()
        );
    }
}

#[test]
fn inspect_hashes_empty_binary_and_complete_line_bytes() {
    for (bytes, address, expected) in [
        (
            b"".as_slice(),
            "source.txt",
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        ),
        (
            b"\xff\x00\nZ".as_slice(),
            "source.txt#L1-L1",
            Some("c933d2fe5a3675b959c287c271739ac2db888cc8c0d68c1c5b58ac5b80f5d735"),
        ),
        (
            b"one\r\ntwo".as_slice(),
            "source.txt#L1-L1",
            Some("5259d46a49644bf76792231ef7315b5293677c49ddd7e69d95557013e10320d4"),
        ),
        (b"".as_slice(), "source.txt#L1-L1", None),
    ] {
        // Arrange
        let repo = ReadingRepo::new();
        repo.write("source.txt", bytes);
        write_references(&repo, &[address]);
        // Act
        let result = inspect(&repo, &[address]);
        // Assert
        assert_eq!(
            result.elements[address].hash,
            expected.map(|s| Sha256::parse(s).unwrap())
        );
        assert_eq!(read_blob(&repo, &result, "source.txt"), bytes);
    }
}

#[test]
fn inspect_retains_source_through_gc_without_changing_user_git_state() {
    // Arrange: ReadingRepo starts with a committed HEAD and staged changes.
    let repo = ReadingRepo::new();
    repo.write("source.txt", b"source before later edit\n");
    write_references(&repo, &["source.txt"]);
    let head = fs::read(repo.get_root().join(".git/HEAD")).unwrap();
    let index = fs::read(repo.get_root().join(".git/index")).unwrap();
    // Act
    let result = inspect(&repo, &["note.md", "source.txt"]);
    repo.write("source.txt", b"edited after inspect\n");
    let status = Command::new("git")
        .current_dir(repo.get_root())
        .args(["gc", "--prune=now"])
        .status()
        .unwrap();
    // Assert
    assert!(status.success());
    assert_eq!(
        read_blob(&repo, &result, "source.txt"),
        b"source before later edit\n"
    );
    assert_eq!(fs::read(repo.get_root().join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::read(repo.get_root().join(".git/index")).unwrap(), index);
}

#[cfg(unix)]
#[test]
fn inspect_rejects_symlinked_declared_source_without_retaining_success() {
    // Arrange
    let repo = ReadingRepo::new();
    write_references(&repo, &["source.txt"]);
    std::os::unix::fs::symlink("live.md", repo.get_root().join("source.txt")).unwrap();
    let ids = [ElementAddress::parse("source.txt").unwrap()];
    // Act
    let error = wire_inspector(repo.get_root())
        .unwrap()
        .inspect(&ids)
        .unwrap_err();
    // Assert
    assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
    assert!(!error.to_string().contains("unstaged live"));
}

fn inspect(repo: &ReadingRepo, ids: &[&str]) -> Inspection {
    let ids = ids
        .iter()
        .map(|id| ElementAddress::parse(id).unwrap())
        .collect::<Vec<_>>();
    wire_inspector(repo.get_root())
        .unwrap()
        .inspect(&ids)
        .unwrap()
}

fn write_references(repo: &ReadingRepo, sources: &[&str]) {
    let links = sources
        .iter()
        .map(|source| format!("[source]({source} \"letopis:type=external\")\n"))
        .collect::<String>();
    repo.write(
        "note.md",
        format!("<!-- letopis: {{\"created_at\":1}} -->\nKnowledgeToken\n{links}").as_bytes(),
    );
}

fn read_blob(repo: &ReadingRepo, result: &Inspection, path: &str) -> Vec<u8> {
    open_git_snapshots(repo.get_root())
        .unwrap()
        .read_blob(&result.snapshot, &RepoPath::parse(path).unwrap())
        .unwrap()
}
