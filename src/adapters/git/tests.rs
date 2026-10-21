use super::{
    GitCliAdapter,
    process::{GitProcess, check_version},
};
use crate::service::ports::git::SnapshotErrorKind::InternalError;
#[path = "../../../tests/support/git.rs"]
mod fixture;
use fixture::{ObjectFormat, TempRepo};
#[test]
fn version_gate() {
    assert!(check_version(b"git version 2.29.0\n").is_ok());
    assert!(check_version(b"git version 2.50.1 (Apple Git-155)\n").is_ok());
    assert_eq!(
        check_version(b"git version 2.28.7\n").unwrap_err().kind(),
        InternalError
    );
    assert_eq!(
        check_version(b"unknown\n").unwrap_err().kind(),
        InternalError
    );
}
#[test]
fn open_rejects_old_git_before_object_calls() {
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.script(
        "old-git",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> calls\necho 'git version 2.28.7'\n",
    );
    let error = GitCliAdapter::open_with_process(repo.root(), GitProcess::with_executable(exe))
        .err()
        .unwrap();
    assert!(error.message().contains("2.29"));
    assert_eq!(
        std::fs::read_to_string(repo.root().join("calls")).unwrap(),
        "-c core.hooksPath=/dev/null --version\n"
    );
}
#[test]
fn discovers_root_from_subdirectory() {
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let git = GitCliAdapter::open(repo.subdir()).unwrap();
        assert_eq!(git.root, repo.root());
        assert_eq!(
            git.oid_len,
            match format {
                ObjectFormat::Sha1 => 40,
                ObjectFormat::Sha256 => 64,
            }
        );
    }
}
#[test]
fn open_rejects_non_repository_and_bare() {
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let bare = repo.root().join("bare");
    repo.ok(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
    assert!(GitCliAdapter::open(&bare).is_err());
    let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
    std::fs::create_dir(&empty).unwrap();
    assert!(GitCliAdapter::open(&empty).is_err());
    std::fs::remove_dir(empty).unwrap();
}
#[test]
fn subprocess_drains_and_reaps_on_io_failure() {
    let name = "adapters::git::tests::subprocess_drains_and_reaps_on_io_failure";
    if std::env::var_os("LETOPIS_IO_WORKER").is_none() {
        fixture::supervised_test(name, &[("LETOPIS_IO_WORKER", "1")]);
        return;
    }
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe=repo.script("failing-git","#!/bin/sh\necho $$ > child-pid\nhead -c 262144 /dev/zero >&2\necho STDERR_CANARY >&2\nhead -c 262144 /dev/zero\nhead -c 1048576 > /dev/null\nexit 7\n");
    let process = GitProcess::with_executable(exe);
    let input = b"STDIN_CANARY".repeat(100000);
    let error = process.run(repo.root(), &[], Some(&input)).unwrap_err();
    let safe = format!("{error} {error:?}");
    assert!(!safe.contains("STDIN_CANARY"));
    assert!(!safe.contains("STDERR_CANARY"));
    assert_eq!(error.kind(), InternalError);
    let pid = std::fs::read_to_string(repo.root().join("child-pid")).unwrap();
    assert!(
        !std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .unwrap()
            .status
            .success(),
        "Git child must be reaped"
    );
}

#[test]
fn process_removes_inherited_overrides_and_hardens_writes() {
    let name = "adapters::git::tests::process_removes_inherited_overrides_and_hardens_writes";
    if std::env::var_os("LETOPIS_POLICY_WORKER").is_none() {
        fixture::supervised_test(
            name,
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
        return;
    }
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let exe = repo.script(
        "policy-git",
        "#!/bin/sh\nenv | sort\nprintf 'ARG=%s\\n' \"$@\"\n",
    );
    let p = GitProcess::with_executable(exe);
    let out = p
        .run(
            repo.root(),
            &["hash-object".into(), "-w".into(), "--stdin".into()],
            None,
        )
        .unwrap();
    let text = String::from_utf8(out).unwrap();
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
