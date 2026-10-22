use super::GitCli;
use crate::git::{
    constants::SNAPSHOT_REF_PREFIX,
    process::{GitProcess, check_version},
};
use crate::{
    domain::{RepoPath, SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN},
    open_git_snapshots,
    service::{GitSnapshots, SnapshotErrorKind, SnapshotFile},
};
#[path = "../../../tests/support/git.rs"]
mod fixture;
use fixture::{ObjectFormat, TempRepo};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn version_gate() {
    // Arrange
    for output in [
        b"git version 2.29.0\n".as_slice(),
        b"git version 2.50.1 (Apple Git-155)\n",
    ] {
        // Act
        let result = check_version(output);
        // Assert
        assert!(result.is_ok());
    }
    // Arrange
    for output in [b"git version 2.28.7\n".as_slice(), b"unknown\n"] {
        // Act
        let error = check_version(output).unwrap_err();
        // Assert
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    }
}

#[test]
fn open_rejects_old_git_before_object_calls() {
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script(
        "old-git",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> calls\necho 'git version 2.28.7'\n",
    );
    let process = GitProcess::new_with_executable(exe);
    // Act
    let error = GitCli::open_with_process(repo.get_root(), process)
        .err()
        .unwrap();
    let calls = fs::read_to_string(repo.get_root().join("calls")).unwrap();
    // Assert
    assert!(error.get_message().contains("2.29"));
    assert_eq!(calls, "-c core.hooksPath=/dev/null --version\n");
}

#[test]
fn discovers_root_from_subdirectory() {
    // Arrange
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let expected_length = match format {
            ObjectFormat::Sha1 => SHA1_OID_HEX_LEN,
            ObjectFormat::Sha256 => SHA256_OID_HEX_LEN,
        };
        // Act
        let git = GitCli::open(repo.get_subdir()).unwrap();
        // Assert
        assert_eq!(git.root, repo.get_root());
        assert_eq!(git.oid_len, expected_length);
    }
}

#[test]
fn open_rejects_non_repository_and_bare() {
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let bare = repo.get_root().join("bare");
    repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
    let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
    fs::create_dir(&empty).unwrap();
    // Act
    let bare_result = GitCli::open(&bare);
    let empty_result = GitCli::open(&empty);
    // Assert
    assert!(bare_result.is_err());
    assert!(empty_result.is_err());
    fs::remove_dir(empty).unwrap();
}

#[test]
fn subprocess_drains_and_reaps_on_io_failure() {
    if std::env::var_os("LETOPIS_IO_WORKER").is_none() {
        // Arrange / Act / Assert: supervisor runs this exact worker and checks its exit.
        fixture::supervise_test(
            "git::cli::tests::subprocess_drains_and_reaps_on_io_failure",
            &[("LETOPIS_IO_WORKER", "1")],
        );
        return;
    }
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script("failing-git", "#!/bin/sh\necho $$ > child-pid\nhead -c 262144 /dev/zero >&2\necho STDERR_CANARY >&2\nhead -c 262144 /dev/zero\nhead -c 1048576 > /dev/null\nexit 7\n");
    let process = GitProcess::new_with_executable(exe);
    let input = b"STDIN_CANARY".repeat(100000);
    // Act
    let error = process.run(repo.get_root(), &[], Some(&input)).unwrap_err();
    let safe = format!("{error} {error:?}");
    let pid = fs::read_to_string(repo.get_root().join("child-pid")).unwrap();
    let child_exists = Command::new("kill")
        .args(["-0", pid.trim()])
        .output()
        .unwrap()
        .status
        .success();
    // Assert
    assert!(!safe.contains("STDIN_CANARY"));
    assert!(!safe.contains("STDERR_CANARY"));
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(!child_exists, "Git child must be reaped");
}

fn supervise_policy_worker() {
    fixture::supervise_test(
        "git::cli::tests::process_removes_inherited_overrides_and_hardens_writes",
        &[
            ("LETOPIS_POLICY_WORKER", "1"),
            ("GIT_DIR", "CANARY_DIR"),
            ("GIT_WORK_TREE", "CANARY_WORKTREE"),
            ("GIT_INDEX_FILE", "CANARY_INDEX"),
            ("GIT_OBJECT_DIRECTORY", "CANARY_OBJECTS"),
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "core.bare"),
            ("GIT_CONFIG_VALUE_0", "true"),
            ("GIT_CONFIG_PARAMETERS", "CANARY_CONFIG"),
        ],
    );
}

#[test]
fn process_removes_inherited_overrides_and_hardens_writes() {
    if std::env::var_os("LETOPIS_POLICY_WORKER").is_none() {
        // Arrange / Act / Assert: exercise inherited overrides in a supervised worker.
        supervise_policy_worker();
        return;
    }
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script(
        "policy-git",
        "#!/bin/sh\nenv | sort\nprintf 'ARG=%s\\n' \"$@\"\n",
    );
    let process = GitProcess::new_with_executable(exe);
    let args = ["hash-object".into(), "-w".into(), "--stdin".into()];
    // Act
    let output = process.run(repo.get_root(), &args, None).unwrap();
    let text = String::from_utf8(output).unwrap();
    // Assert
    assert!(!text.contains("CANARY"));
    assert!(!text.contains("GIT_CONFIG_COUNT="));
    assert!(!text.contains("GIT_CONFIG_KEY_0="));
    for flag in [
        "GIT_NO_REPLACE_OBJECTS=1",
        "GIT_ALLOW_PROTOCOL=\n",
        "GIT_NO_LAZY_FETCH=1",
        "GIT_TERMINAL_PROMPT=0",
        "LC_ALL=C",
        "ARG=core.hooksPath=/dev/null",
        "ARG=core.fsync=all",
        "ARG=core.fsyncMethod=fsync",
    ] {
        assert!(text.contains(flag), "missing {flag}");
    }
}

fn create_newline_repository_pair(parent: &TempRepo, format: ObjectFormat) -> (PathBuf, PathBuf) {
    let sibling = parent.get_root().join("repo");
    let target = parent.get_root().join("repo\n");
    for root in [&sibling, &target] {
        parent.run_git_successfully(
            &["init", "-q", format.to_init_arg(), root.to_str().unwrap()],
            None,
        );
    }
    (sibling, target)
}

#[test]
fn discovers_trailing_newline_repository_without_touching_sibling() {
    // Arrange
    for format in fixture::FORMATS {
        let parent = TempRepo::new(format);
        let (sibling, target) = create_newline_repository_pair(&parent, format);
        let sibling_objects = parent.run_git_successfully(
            &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
            None,
        );
        let sibling_refs =
            parent.run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
        let path = RepoPath::parse("captured.md").unwrap();
        // Act
        let git = open_git_snapshots(&target).unwrap();
        let tree = git
            .create_tree(&[SnapshotFile {
                path: &path,
                bytes: b"newline repository isolation\0",
            }])
            .unwrap();
        git.pin(&tree).unwrap();
        let objects_after = parent.run_git_successfully(
            &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
            None,
        );
        let refs_after =
            parent.run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
        let name = format!("{SNAPSHOT_REF_PREFIX}{}", tree.as_str());
        let actual = parent.run_git(
            &[
                "-C",
                target.to_str().unwrap(),
                "show-ref",
                "--verify",
                &name,
            ],
            None,
        );
        let bytes = git.read_blob(&tree, &path).unwrap();
        let discovered_root = GitCli::open(&target).unwrap().root;
        // Assert
        assert_eq!(
            objects_after, sibling_objects,
            "opening repo\\n must not write objects into sibling repo"
        );
        assert_eq!(
            refs_after, sibling_refs,
            "opening repo\\n must not pin a ref in sibling repo"
        );
        assert!(
            actual.status.success(),
            "snapshot must be retained in the requested newline-suffixed repository"
        );
        assert_eq!(bytes, b"newline repository isolation\0");
        assert_eq!(discovered_root, target);
    }
}

#[test]
fn candidates_follow_git_ignore_and_preserve_deleted_tracked_paths() {
    use crate::service::CorpusFiles;
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    for path in [
        "deleted.md",
        "tracked-ignored.md",
        "space name.markdown",
        "ignored.md",
        "plain.txt",
    ] {
        fs::write(repo.get_root().join(path), b"bytes").unwrap();
    }
    repo.run_git_successfully(&["add", "deleted.md", "tracked-ignored.md"], None);
    fs::remove_file(repo.get_root().join("deleted.md")).unwrap();
    fs::write(repo.get_root().join(".gitignore"), b"*ignored.md\n").unwrap();
    fs::create_dir(repo.get_root().join(".letopis")).unwrap();
    fs::write(repo.get_root().join(".letopis/private.md"), b"private").unwrap();
    let before = repo.capture_fixture_state();
    let objects = repo.run_git_successfully(&["count-objects", "-v"], None);
    let refs = repo.run_git_successfully(&["show-ref"], None);
    let git = GitCli::open(repo.get_root()).unwrap();
    // Act
    let candidates = git.list_candidates().unwrap();
    // Assert
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        [
            "deleted.md",
            "live.md",
            "space name.markdown",
            "tracked-ignored.md"
        ]
    );
    assert_eq!(repo.capture_fixture_state(), before);
    assert_eq!(
        repo.run_git_successfully(&["count-objects", "-v"], None),
        objects
    );
    assert_eq!(repo.run_git_successfully(&["show-ref"], None), refs);
}

#[test]
fn candidates_reject_unsupported_raw_paths() {
    use crate::service::{CorpusFiles, DocumentErrorKind};
    for raw in [
        b"bad\xff.md".to_vec(),
        b"bad#anchor.md".to_vec(),
        b"bad\nline.md".to_vec(),
    ] {
        // Arrange
        let repo = TempRepo::new(ObjectFormat::Sha1);
        // Git's index supports raw paths even when the host filesystem rejects them.
        let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend(raw);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        let git = GitCli::open(repo.get_root()).unwrap();
        // Act
        let error = git.list_candidates().unwrap_err();
        // Assert
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        assert!(!error.to_string().contains("bad"));
    }
}

#[test]
fn candidates_deduplicate_unmerged_index_stages() {
    use crate::service::CorpusFiles;
    // Arrange
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"conflict"));
    let entries = format!(
        "0 {}\tconflict.md\n100644 {oid} 1\tconflict.md\n100644 {oid} 2\tconflict.md\n100644 {oid} 3\tconflict.md\n",
        "0".repeat(40)
    );
    repo.run_git_successfully(&["update-index", "--index-info"], Some(entries.as_bytes()));
    let git = GitCli::open(repo.get_root()).unwrap();
    // Act
    let candidates = git.list_candidates().unwrap();
    // Assert
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        ["conflict.md", "live.md"]
    );
}
