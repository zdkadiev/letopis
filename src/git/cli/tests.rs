use super::{GitCli, parse_candidates};
use crate::git::{
    constants::SNAPSHOT_REF_PREFIX,
    process::{GitProcess, check_version},
};
use crate::process_test_support as fixture;
use crate::{
    domain::RepoPath,
    open_git_snapshots,
    service::{DocumentErrorKind, GitSnapshots, SnapshotErrorKind, SnapshotFile},
};
use fixture::{ObjectFormat, TempRepo};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn version_gate() {
    // Given
    for output in [
        b"git version 2.29.0\n".as_slice(),
        b"git version 2.50.1 (Apple Git-155)\n",
    ] {
        // When
        let result = check_version(output);
        // Then
        assert!(result.is_ok());
    }
    // Given
    for output in [b"git version 2.28.7\n".as_slice(), b"unknown\n"] {
        // When
        let error = check_version(output).unwrap_err();
        // Then
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    }
}

#[test]
fn open_rejects_old_git_before_object_calls() {
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script(
        "old-git",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> calls\necho 'git version 2.28.7'\n",
    );
    let process = GitProcess::new_with_executable(exe);
    // When
    let error = GitCli::open_with_process(repo.get_root(), process)
        .err()
        .unwrap();
    let calls = fs::read_to_string(repo.get_root().join("calls")).unwrap();
    // Then
    assert!(error.get_message().contains("2.29"));
    assert_eq!(calls, "-c core.hooksPath=/dev/null --version\n");
}

#[test]
fn discovers_root_from_subdirectory() {
    // Given
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let expected_length = match format {
            ObjectFormat::Sha1 => 40,
            ObjectFormat::Sha256 => 64,
        };
        // When
        let git = GitCli::open(repo.get_subdir()).unwrap();
        // Then
        assert_eq!(git.root, repo.get_root());
        assert_eq!(git.oid_len, expected_length);
    }
}

#[test]
fn open_rejects_non_repository_and_bare() {
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let bare = repo.get_root().join("bare");
    repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
    let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
    fs::create_dir(&empty).unwrap();
    // When
    let bare_result = GitCli::open(&bare);
    let empty_result = GitCli::open(&empty);
    // Then
    assert!(bare_result.is_err());
    assert!(empty_result.is_err());
    fs::remove_dir(empty).unwrap();
}

#[test]
fn subprocess_drains_and_reaps_on_io_failure() {
    if std::env::var_os("LETOPIS_IO_WORKER").is_none() {
        // Given / When / Then: supervisor runs this exact worker and checks its exit.
        fixture::supervise_test(
            "git::cli::tests::subprocess_drains_and_reaps_on_io_failure",
            &[("LETOPIS_IO_WORKER", "1")],
        );
        return;
    }
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script("failing-git", "#!/bin/sh\necho $$ > child-pid\nhead -c 262144 /dev/zero >&2\necho STDERR_CANARY >&2\nhead -c 262144 /dev/zero\nhead -c 1048576 > /dev/null\nexit 7\n");
    let process = GitProcess::new_with_executable(exe);
    let input = b"STDIN_CANARY".repeat(100000);
    // When
    let error = process.run(repo.get_root(), &[], Some(&input)).unwrap_err();
    let pid = fs::read_to_string(repo.get_root().join("child-pid")).unwrap();
    let child_exists = Command::new("kill")
        .args(["-0", pid.trim()])
        .output()
        .unwrap()
        .status
        .success();
    // Then
    for diagnostic in [error.to_string(), format!("{error:?}")] {
        assert!(
            diagnostic.contains('7'),
            "the safe exit cause must remain visible"
        );
        assert!(!diagnostic.contains("STDIN_CANARY"));
        assert!(!diagnostic.contains("STDERR_CANARY"));
    }
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(!child_exists, "Git child must be reaped");
}

#[test]
fn process_removes_inherited_overrides_and_hardens_writes() {
    if std::env::var_os("LETOPIS_POLICY_WORKER").is_none() {
        // Given / When / Then: exercise inherited overrides in a supervised worker.
        supervise_policy_worker();
        return;
    }
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.write_script(
        "policy-git",
        "#!/bin/sh\nenv | sort\nprintf 'ARG=%s\\n' \"$@\"\n",
    );
    let process = GitProcess::new_with_executable(exe);
    let args = ["hash-object".into(), "-w".into(), "--stdin".into()];
    // When
    let output = process.run(repo.get_root(), &args, None).unwrap();
    let text = String::from_utf8(output).unwrap();
    // Then
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

#[test]
fn discovers_trailing_newline_repository_without_touching_sibling() {
    // Given
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
        // When
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
        // Then
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
    use crate::service::CorpusLister;
    // Given
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
    // When
    let candidates = git.list_candidates().unwrap();
    // Then
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
    use crate::service::{CorpusLister, DocumentErrorKind};
    for raw in [
        b"bad\xff.md".to_vec(),
        b"bad#anchor.md".to_vec(),
        b"bad\nline.md".to_vec(),
    ] {
        // Given
        let repo = TempRepo::new(ObjectFormat::Sha1);
        // Git's index supports raw paths even when the host filesystem rejects them.
        let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend(raw);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        let git = GitCli::open(repo.get_root()).unwrap();
        // When
        let error = git.list_candidates().unwrap_err();
        // Then
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        assert!(!error.to_string().contains("bad"));
    }
}

#[test]
fn candidates_deduplicate_unmerged_index_stages() {
    use crate::service::CorpusLister;
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"conflict"));
    let entries = format!(
        "0 {}\tconflict.md\n100644 {oid} 1\tconflict.md\n100644 {oid} 2\tconflict.md\n100644 {oid} 3\tconflict.md\n",
        "0".repeat(40)
    );
    repo.run_git_successfully(&["update-index", "--index-info"], Some(entries.as_bytes()));
    let git = GitCli::open(repo.get_root()).unwrap();
    // When
    let candidates = git.list_candidates().unwrap();
    // Then
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        ["conflict.md", "live.md"]
    );
}

#[test]
fn candidates_exclude_metadata_before_validating_paths() {
    use crate::service::CorpusLister;
    for raw in [
        b".letopis/bad#anchor.md".as_slice(),
        b".letopis/bad\xff.md",
        b"nested/.letopis/bad\xff.markdown",
    ] {
        // Given: Git can list metadata names unsupported by the corpus model.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join(".letopis-notes.md"), b"candidate").unwrap();
        let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend(raw);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        let git = GitCli::open(repo.get_root()).unwrap();
        // When
        let candidates = git.list_candidates().unwrap();
        // Then
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            [".letopis-notes.md", "live.md"]
        );
    }
}

#[test]
fn candidates_exclude_raw_git_metadata_components() {
    // Given: Git itself forbids adding .git paths to its real index.
    let raw = b".git/bad#anchor.md\0nested/.git/bad\xff.markdown\0.git-notes.md\0";
    // When
    let candidates = parse_candidates(raw).unwrap();
    // Then
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        [".git-notes.md"]
    );
}

#[test]
fn rejects_non_absence_errors_when_inspecting_object_alternates() {
    // Given: ENOTDIR is deterministic even when tests run with elevated privileges.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let info = repo.get_root().join(".git/objects/info");
    fs::remove_dir(&info).unwrap();
    fs::write(&info, b"not a directory").unwrap();
    // When
    let error = GitCli::open(repo.get_root())
        .err()
        .expect("unsafe object store was accepted");
    // Then
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert_eq!(fs::read(info).unwrap(), b"not a directory");
}

#[test]
fn rejects_unterminated_candidates_and_accepts_empty_output() {
    // Given / When
    let malformed = parse_candidates(b"note.md").unwrap_err();
    let empty = parse_candidates(b"").unwrap();
    // Then
    assert_eq!(malformed.get_kind(), DocumentErrorKind::InternalError);
    assert!(empty.is_empty());
}

#[test]
fn rejects_object_header_with_a_different_advertised_oid() {
    // Given: all subprocess work stays real except one malformed protocol header.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let program = repo.write_script(
        "wrong-object-header",
        "#!/bin/sh\nfor arg do\n if [ \"$arg\" = --batch-check ]; then\n  read -r requested\n  printf '0000000000000000000000000000000000000000 tree 0\\n'\n  exit 0\n fi\ndone\nexec git \"$@\"\n",
    );
    let process = GitProcess::new_with_executable(program);
    let git = GitCli::open_with_process(repo.get_root(), process).unwrap();
    let tree = git.create_tree(&[]).unwrap();
    let path = RepoPath::parse("missing.md").unwrap();
    // When
    let error = git.read_blob(&tree, &path).unwrap_err();
    // Then: accepting the wrong OID would instead reach an ordinary missing path.
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
}

#[test]
fn preserves_unexpected_symbolic_ref_failure_before_writing_a_ref() {
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let program = repo.write_script(
        "failed-ref-inspection",
        "#!/bin/sh\nfor arg do\n if [ \"$arg\" = symbolic-ref ]; then exit 7; fi\ndone\nexec git \"$@\"\n",
    );
    let process = GitProcess::new_with_executable(program);
    let git = GitCli::open_with_process(repo.get_root(), process).unwrap();
    let tree = git.create_tree(&[]).unwrap();
    // When
    let error = git.pin(&tree).unwrap_err();
    // Then: exit 1 alone means a nonsymbolic ref; other exits remain failures.
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(error.to_string().contains('7'));
    assert!(
        repo.run_git_successfully(&["for-each-ref", SNAPSHOT_REF_PREFIX], None)
            .is_empty()
    );
}

#[test]
fn hardens_tree_and_ref_writes_with_git_fsync_settings() {
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let program = repo.write_script("write-arguments", "#!/bin/sh\nprintf '%s\\n' \"$@\"\n");
    let process = GitProcess::new_with_executable(program);
    for command in ["mktree", "update-ref"] {
        // When
        let output = process
            .run(repo.get_root(), &[command.into()], None)
            .unwrap();
        let arguments = String::from_utf8(output).unwrap();
        // Then
        assert!(arguments.lines().any(|arg| arg == "core.fsync=all"));
        assert!(arguments.lines().any(|arg| arg == "core.fsyncMethod=fsync"));
    }
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
