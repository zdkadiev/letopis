#[path = "support/git.rs"]
mod git;
use git::{FORMATS, TempRepo};
use letopis::{
    domain::{RepoPath, TreeOid},
    open_git_snapshots,
    service::ports::git::{GitSnapshotPort, SnapshotErrorKind::InternalError, SnapshotFile},
};
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn path(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}
fn oid(s: &str) -> TreeOid {
    TreeOid::parse(s).unwrap()
}
fn external(repo: &TempRepo, child: &str, name: &str, kind: &str) -> TreeOid {
    oid(&repo.oid(
        &["mktree", "-z", "--missing"],
        Some(
            format!(
                "{} {kind} {child}\t{name}\0",
                if kind == "tree" { "040000" } else { "100644" }
            )
            .as_bytes(),
        ),
    ))
}
fn reference(tree: &TreeOid) -> String {
    format!("refs/letopis/snapshots/{}", tree.as_str())
}
fn assert_no_ref(repo: &TempRepo, tree: &TreeOid) {
    assert!(
        !repo
            .git(&["show-ref", "--verify", &reference(tree)], None)
            .status
            .success()
    );
}
#[test]
fn retained_snapshots_survive_gc() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.subdir()).unwrap();
        let p = path("nested/x");
        let s1 = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"inspect unique\0",
            }])
            .unwrap();
        let blob = repo.oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"external basis\r\n"),
        );
        let nested = external(&repo, &blob, "x", "blob");
        let basis = external(&repo, nested.as_str(), "nested", "tree");
        let before = repo.head_index_config_and_live_snapshot();
        for tree in [&s1, &basis] {
            assert_eq!(git.pin(tree).unwrap().oid(), tree);
            assert_eq!(repo.direct_ref(tree.as_str()), tree.as_str());
            assert_eq!(git.pin(tree).unwrap().oid(), tree);
        }
        assert_eq!(repo.head_index_config_and_live_snapshot(), before);
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        let reopened = open_git_snapshots(repo.root()).unwrap();
        assert_eq!(reopened.read_blob(&s1, &p).unwrap(), b"inspect unique\0");
        assert_eq!(
            reopened.read_blob(&basis, &p).unwrap(),
            b"external basis\r\n"
        );
        assert_eq!(reopened.pin(&basis).unwrap().oid(), &basis);
        assert!(repo.fsck_strict_no_reflogs().success());
    }
}
#[test]
fn unretained_unique_tree_is_collectable() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let p = path("unique");
        let s = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"UNRETAINED unique object\0",
            }])
            .unwrap();
        assert_eq!(
            git.read_blob(&s, &p).unwrap(),
            b"UNRETAINED unique object\0"
        );
        assert_no_ref(&repo, &s);
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        assert!(
            !repo
                .git(&["cat-file", "-e", s.as_str()], None)
                .status
                .success()
        );
    }
}
#[test]
fn incomplete_tree_and_ref_failure_do_not_pin() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let p = path("x");
        let length = repo.oid(&["rev-parse", "HEAD"], None).len();
        let missing = "1".repeat(length);
        let incomplete = external(&repo, &missing, "x", "blob");
        assert_eq!(git.pin(&incomplete).unwrap_err().kind(), InternalError);
        assert_no_ref(&repo, &incomplete);
        let blob = repo.oid(&["hash-object", "-w", "--stdin"], Some(b"expected"));
        let other = repo.oid(&["hash-object", "-w", "--stdin"], Some(b"CORRUPT_CANARY"));
        let corrupt = external(&repo, &blob, "x", "blob");
        let dest = repo
            .root()
            .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]));
        fs::remove_file(&dest).unwrap();
        fs::copy(
            repo.root()
                .join(format!(".git/objects/{}/{}", &other[..2], &other[2..])),
            dest,
        )
        .unwrap();
        assert_eq!(git.pin(&corrupt).unwrap_err().kind(), InternalError);
        assert_no_ref(&repo, &corrupt);
        let tree = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"forced ref failure",
            }])
            .unwrap();
        let lock = repo.root().join(format!(".git/{}.lock", reference(&tree)));
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, b"fixture owned lock").unwrap();
        let error = git.pin(&tree).unwrap_err();
        assert_eq!(error.kind(), InternalError);
        assert_no_ref(&repo, &tree);
        assert!(!format!("{error:?} {error}").contains("fixture owned lock"));
    }
}
#[test]
fn symbolic_and_conflicting_refs_are_rejected() {
    for format in FORMATS {
        for conflict in ["symbolic", "dangling-symbolic", "other", "corrupt"] {
            let repo = TempRepo::new(format);
            let git = open_git_snapshots(repo.root()).unwrap();
            let tree = git.create_tree(&[]).unwrap();
            let before = repo.head_index_config_and_live_snapshot();
            let name = reference(&tree);
            match conflict {
                "dangling-symbolic" => {
                    repo.ok(&["symbolic-ref", &name, "refs/heads/absent"], None);
                }
                "symbolic" => {
                    let branch = repo.oid(&["symbolic-ref", "HEAD"], None);
                    repo.ok(&["symbolic-ref", &name, &branch], None);
                }
                "other" => {
                    let head = repo.oid(&["rev-parse", "HEAD"], None);
                    repo.ok(&["update-ref", &name, &head], None);
                }
                _ => {
                    let dest = repo.root().join(format!(".git/{name}"));
                    fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    fs::write(dest, b"CORRUPT_REF_CANARY\n").unwrap();
                }
            }
            let err = git.pin(&tree).unwrap_err();
            assert_eq!(err.kind(), InternalError);
            assert!(!format!("{err} {err:?}").contains("CANARY"));
            assert_eq!(repo.head_index_config_and_live_snapshot(), before);
        }
    }
}
#[test]
fn concurrent_pin_is_idempotent() {
    if let Some(root) = std::env::var_os("LETOPIS_PIN_ROOT") {
        let root = std::path::PathBuf::from(root);
        let tree = oid(&std::env::var("LETOPIS_PIN_OID").unwrap());
        let id = std::env::var("LETOPIS_PIN_WORKER").unwrap();
        fs::write(root.join(format!("ready-{id}")), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.join("start").exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            open_git_snapshots(&root).unwrap().pin(&tree).unwrap().oid(),
            &tree
        );
        return;
    }
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let tree = open_git_snapshots(repo.root())
            .unwrap()
            .create_tree(&[])
            .unwrap();
        let mut children = Vec::new();
        for id in ["a", "b"] {
            let mut c = Command::new(std::env::current_exe().unwrap());
            git::isolated(&mut c);
            c.args(["--exact", "concurrent_pin_is_idempotent", "--nocapture"])
                .env("LETOPIS_PIN_ROOT", repo.root())
                .env("LETOPIS_PIN_OID", tree.as_str())
                .env("LETOPIS_PIN_WORKER", id)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            children.push(git::OwnedChild::spawn(&mut c));
        }
        let deadline = Instant::now() + Duration::from_secs(25);
        while !(repo.root().join("ready-a").exists() && repo.root().join("ready-b").exists()) {
            if Instant::now() > deadline {
                panic!("workers not ready");
            }
            thread::sleep(Duration::from_millis(5));
        }
        fs::write(repo.root().join("start"), b"go").unwrap();
        let outputs: Vec<_> = children
            .into_iter()
            .map(|child| child.wait(deadline))
            .collect();
        for out in outputs {
            assert!(
                out.status.success(),
                "{} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert_eq!(repo.direct_ref(tree.as_str()), tree.as_str());
    }
}
#[test]
fn promisor_miss_never_fetches() {
    if std::env::var_os("LETOPIS_PROMISOR_WORKER").is_none() {
        git::supervised_test(
            "promisor_miss_never_fetches",
            &[("LETOPIS_PROMISOR_WORKER", "1")],
        );
        return;
    }
    for format in FORMATS {
        let remote = TempRepo::new(format);
        let repo = TempRepo::new(format);
        let blob = remote.oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"promised remote bytes"),
        );
        let tree = external(&repo, &blob, "missing", "blob");
        let marker = repo.root().join("FETCH_RAN");
        let script = repo.script(
            "transport",
            "#!/bin/sh\ntouch \"$(dirname \"$0\")/FETCH_RAN\"\nexec git-upload-pack \"$1\"\n",
        );
        let url = format!("ext::{} {}", script.display(), remote.root().display());
        repo.ok(
            &["-c", "protocol.ext.allow=always", "ls-remote", &url],
            None,
        );
        assert!(marker.exists(), "transport trap must be reachable");
        fs::remove_file(&marker).unwrap();
        for (key, value) in [
            ("remote.origin.url", url.as_str()),
            ("remote.origin.promisor", "true"),
            ("remote.origin.partialclonefilter", "blob:none"),
            ("extensions.partialClone", "origin"),
            ("protocol.ext.allow", "always"),
            ("protocol.file.allow", "always"),
        ] {
            repo.ok(&["config", key, value], None);
        }
        let git = open_git_snapshots(repo.root()).unwrap();
        assert_eq!(
            git.read_blob(&tree, &path("missing")).unwrap_err().kind(),
            InternalError
        );
        assert_eq!(git.pin(&tree).unwrap_err().kind(), InternalError);
        assert_no_ref(&repo, &tree);
        assert!(!marker.exists(), "no lazy transport is permitted");
        assert!(
            !repo
                .root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))
                .exists()
        );
    }
}

#[test]
fn conflicting_descendant_types_do_not_pin() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let blob = repo.oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"shared oid type confusion"),
        );
        let binary: Vec<_> = blob
            .as_bytes()
            .chunks_exact(2)
            .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap())
            .collect();
        let mut raw = b"40000 a\0".to_vec();
        raw.extend_from_slice(&binary);
        raw.extend_from_slice(b"100644 z\0");
        raw.extend_from_slice(&binary);
        let tree = oid(&repo.oid(
            &["hash-object", "-w", "--literally", "-t", "tree", "--stdin"],
            Some(&raw),
        ));
        assert_eq!(git.pin(&tree).unwrap_err().kind(), InternalError);
        assert_no_ref(&repo, &tree);
    }
}
#[test]
fn gitlinks_are_retained_without_dereferencing() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let missing = "1".repeat(repo.oid(&["rev-parse", "HEAD"], None).len());
        let tree = oid(&repo.oid(
            &["mktree", "--missing", "-z"],
            Some(format!("160000 commit {missing}\tmodule\0").as_bytes()),
        ));
        let git = open_git_snapshots(repo.root()).unwrap();
        assert_eq!(git.pin(&tree).unwrap().oid(), &tree);
        assert_eq!(
            git.read_blob(&tree, &path("module")).unwrap_err().kind(),
            letopis::service::ports::git::SnapshotErrorKind::InvalidRequest
        );
        assert!(repo.fsck_strict_no_reflogs().success());
    }
}

fn assert_alternates_error(error: &letopis::service::ports::git::SnapshotError, donor: &TempRepo) {
    assert_eq!(error.kind(), InternalError);
    assert!(error.message().contains("alternates"));
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains(donor.root().to_str().unwrap()));
    assert!(!diagnostic.contains("CANARY"));
}

#[test]
fn shared_clone_is_refused_before_donor_gc_can_break_retention() {
    let mut refused = Vec::new();
    for format in FORMATS {
        let donor = TempRepo::new(format);
        let bytes = b"ALTERNATE_PAYLOAD_CANARY unique unreferenced blob\0";
        let blob = donor.oid(&["hash-object", "-w", "--stdin"], Some(bytes));
        let borrower = donor.shared_clone();
        let donor_before = donor.head_index_config_and_live_snapshot();
        let borrower_before = borrower.head_index_config_and_live_snapshot();
        let alternates = borrower.root().join(".git/objects/info/alternates");
        let configuration = fs::read(&alternates).unwrap();
        assert!(
            borrower
                .git(&["cat-file", "-e", &blob], None)
                .status
                .success()
        );
        let local_blob =
            borrower
                .root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]));
        assert!(
            !local_blob.exists(),
            "the ordinary shared clone borrows this blob"
        );
        let opened = open_git_snapshots(borrower.root());
        let p = path("captured");
        let accepted = match opened {
            Ok(git) => {
                // On the unfixed adapter this recreates the public-API failure:
                // hash-object -w reuses the donor blob and pin appears successful.
                let tree = git
                    .create_tree(&[SnapshotFile { path: &p, bytes }])
                    .unwrap();
                git.pin(&tree).unwrap();
                assert_eq!(git.read_blob(&tree, &p).unwrap(), bytes);
                assert!(
                    !local_blob.exists(),
                    "hash-object -w must not be mistaken for materialization"
                );
                Some((git, tree))
            }
            Err(error) => {
                assert_alternates_error(&error, &donor);
                None
            }
        };
        donor.expire_reflogs_gc_prune_now_and_pack_refs();
        assert!(
            !donor.git(&["cat-file", "-e", &blob], None).status.success(),
            "donor GC really removed the unreferenced blob"
        );
        assert!(
            !borrower
                .git(&["cat-file", "-e", &blob], None)
                .status
                .success()
        );
        if let Some((git, tree)) = &accepted {
            assert_eq!(git.read_blob(tree, &p).unwrap_err().kind(), InternalError);
            assert_eq!(borrower.direct_ref(tree.as_str()), tree.as_str());
            eprintln!(
                "{format:?}: accepted shared snapshot lost its donor blob after GC while its service ref remained exact"
            );
        } else {
            assert!(
                borrower
                    .ok(&["for-each-ref", "refs/letopis/snapshots/"], None)
                    .is_empty()
            );
        }
        assert_eq!(donor.head_index_config_and_live_snapshot(), donor_before);
        assert_eq!(
            borrower.head_index_config_and_live_snapshot(),
            borrower_before
        );
        assert_eq!(fs::read(&alternates).unwrap(), configuration);
        refused.push(accepted.is_none());
    }
    assert!(
        refused.iter().all(|r| *r),
        "shared clones must be refused before any snapshot is accepted: {refused:?}"
    );
}

#[test]
fn alternates_added_after_open_refuse_operations_without_writes() {
    for format in FORMATS {
        let donor = TempRepo::new(format);
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let p = path("captured");
        let tree = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"local bytes",
            }])
            .unwrap();
        let alternates = repo.root().join(".git/objects/info/alternates");
        let configuration = format!("{}\n", donor.root().join(".git/objects").display());
        fs::write(&alternates, &configuration).unwrap();
        let before = repo.head_index_config_and_live_snapshot();
        let objects = repo.ok(&["count-objects", "-v"], None);
        let create = git.create_tree(&[SnapshotFile {
            path: &p,
            bytes: b"new ALTERNATE_PAYLOAD_CANARY",
        }]);
        let pin = git.pin(&tree);
        let read = git.read_blob(&tree, &p);
        for error in [create.err(), pin.err(), read.err()] {
            assert_alternates_error(
                &error.expect("alternates introduced after open must be refused"),
                &donor,
            );
        }
        assert_no_ref(&repo, &tree);
        assert_eq!(repo.ok(&["count-objects", "-v"], None), objects);
        assert_eq!(repo.head_index_config_and_live_snapshot(), before);
        assert_eq!(fs::read(&alternates).unwrap(), configuration.as_bytes());
    }
}

#[test]
fn linked_worktree_checks_common_object_store_and_refuses_inactive_alternates() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let linked = repo.root().join("linked");
        repo.ok(
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                linked.to_str().unwrap(),
            ],
            None,
        );
        let git = open_git_snapshots(&linked).unwrap();
        let p = path("linked");
        let tree = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"linked bytes",
            }])
            .unwrap();
        git.pin(&tree).unwrap();
        assert_eq!(git.read_blob(&tree, &p).unwrap(), b"linked bytes");
        let alternates = repo.root().join(".git/objects/info/alternates");
        let before = repo.head_index_config_and_live_snapshot();
        let refs = repo.ok(&["for-each-ref"], None);
        let objects = repo.ok(&["count-objects", "-v"], None);
        for configuration in [b"".as_slice(), b"/ABSENT_DONOR_PATH_CANARY/objects\n"] {
            fs::write(&alternates, configuration).unwrap();
            match open_git_snapshots(&linked) {
                Ok(_) => {
                    panic!("linked worktree must reject even empty/inactive common alternates")
                }
                Err(error) => assert_alternates_error(&error, &repo),
            }
            assert_alternates_error(&git.pin(&tree).unwrap_err(), &repo);
            assert_eq!(fs::read(&alternates).unwrap(), configuration);
            assert_eq!(repo.head_index_config_and_live_snapshot(), before);
            assert_eq!(repo.ok(&["for-each-ref"], None), refs);
        }
        fs::remove_file(alternates).unwrap();
        assert_eq!(repo.ok(&["count-objects", "-v"], None), objects);
        assert_eq!(
            open_git_snapshots(&linked)
                .unwrap()
                .read_blob(&tree, &p)
                .unwrap(),
            b"linked bytes"
        );
    }
}
