#[path = "support/git.rs"]
mod git;
use git::{
    BLOB_KIND, COMMIT_KIND, FORMATS, GITLINK_MODE, ObjectFormat, REGULAR_FILE_MODE,
    SNAPSHOT_REF_PREFIX, TREE_KIND, TREE_MODE, TempRepo,
};
use letopis::{
    domain::{RepoPath, TreeOid},
    open_git_snapshots,
    service::{GitSnapshots, SnapshotError, SnapshotErrorKind, SnapshotFile},
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[test]
fn lightweight_retention_validates_roots_and_survives_gc() {
    // Given: a healthy captured tree in each supported object format.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let path = parse_path("file");
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &path,
                bytes: b"light retained bytes",
            }])
            .unwrap();
        let blob = repo.get_oid(&["rev-parse", &format!("{}:file", tree.as_str())], None);
        // When: only lightweight retention protects the closure from ordinary GC.
        git.ref_update_by_oid(&tree).unwrap();
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        // Then: bytes remain readable, while wrong-type/missing roots are rejected.
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(&tree, &path).unwrap(),
            b"light retained bytes"
        );
        assert_eq!(
            git.ref_update_by_oid(&TreeOid::parse(&blob).unwrap())
                .unwrap_err()
                .get_kind(),
            SnapshotErrorKind::InvalidRequest
        );
        assert_eq!(
            git.ref_update_by_oid(&TreeOid::parse(&"1".repeat(blob.len())).unwrap())
                .unwrap_err()
                .get_kind(),
            SnapshotErrorKind::NotFound
        );
    }
}

#[test]
fn lightweight_retain_preserves_refs_without_claiming_complete_closure() {
    // Given: an intact root referencing a locally missing unrelated blob.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let width = repo.get_oid(&["rev-parse", "HEAD"], None).len();
        let tree = create_external_tree(&repo, &"1".repeat(width), "unrelated", BLOB_KIND);
        // When: routine retention checks the root/ref but does not inspect children.
        git.ref_update_by_oid(&tree).unwrap();
        // Then: reachability is retained; a strong pin still rejects missing bytes.
        assert_eq!(repo.get_direct_ref(tree.as_str()), tree.as_str());
        assert_eq!(
            git.tree_sync_by_oid(&tree).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}

#[test]
fn retained_snapshots_survive_gc() {
    // Given: an application-captured tree and an independently constructed tree.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_subdir()).unwrap();
        let p = parse_path("nested/x");
        let s1 = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"inspect unique\0",
            }])
            .unwrap();
        let blob = repo.get_oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"external basis\r\n"),
        );
        let nested = create_external_tree(&repo, &blob, "x", BLOB_KIND);
        let basis = create_external_tree(&repo, nested.as_str(), "nested", TREE_KIND);
        let before = repo.capture_fixture_state();
        for tree in [&s1, &basis] {
            // When: pin each tree twice.
            git.tree_sync_by_oid(tree).unwrap();
            let actual_ref = repo.get_direct_ref(tree.as_str());
            git.tree_sync_by_oid(tree).unwrap();
            // Then: each pin names the exact tree and fixture state is unchanged.
            assert_eq!(actual_ref, tree.as_str());
        }
        assert_eq!(repo.capture_fixture_state(), before);
        // When: collect garbage, repack refs, and reopen both retained trees.
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        let reopened = open_git_snapshots(repo.get_root()).unwrap();
        let inspect_bytes = reopened.blob_get_by_tree_oid_and_path(&s1, &p).unwrap();
        let basis_bytes = reopened.blob_get_by_tree_oid_and_path(&basis, &p).unwrap();
        reopened.tree_sync_by_oid(&basis).unwrap();
        let integrity = repo.run_fsck_strict_no_reflogs();
        // Then: captured bytes, retained OID, and strict fsck remain valid.
        assert_eq!(inspect_bytes, b"inspect unique\0");
        assert_eq!(basis_bytes, b"external basis\r\n");
        assert!(integrity.success());
    }
}
#[test]
fn unretained_unique_tree_is_collectable() {
    // Given: a unique tree exists without a retaining ref.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let p = parse_path("unique");
        let s = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"UNRETAINED unique object\0",
            }])
            .unwrap();
        // Fixture checks: the unique object exists and has no retention ref.
        let bytes = git.blob_get_by_tree_oid_and_path(&s, &p).unwrap();
        assert_eq!(bytes, b"UNRETAINED unique object\0");
        assert_no_ref(&repo, &s);
        // When: expire reflogs and collect unreachable objects.
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        let exists = repo
            .run_git(&["cat-file", "-e", s.as_str()], None)
            .status
            .success();
        // Then: the unretained tree object is absent.
        assert!(!exists);
    }
}
#[test]
fn incomplete_tree_and_ref_failure_do_not_pin() {
    // Given: a tree references a missing child object.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let p = parse_path("x");
        let length = repo.get_oid(&["rev-parse", "HEAD"], None).len();
        let missing = "1".repeat(length);
        let incomplete = create_external_tree(&repo, &missing, "x", BLOB_KIND);
        // When: attempt to pin the tree with a missing child.
        let error = git.tree_sync_by_oid(&incomplete).unwrap_err();
        // Then: pinning returns InternalError without creating a ref.
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert_no_ref(&repo, &incomplete);
        // Given: a tree child is replaced by bytes with a different object identity.
        let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"expected"));
        let other = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"CORRUPT_CANARY"));
        let corrupt = create_external_tree(&repo, &blob, "x", BLOB_KIND);
        repo.replace_loose_object(&blob, &other);
        // When: attempt to pin the tree with a corrupt child.
        let error = git.tree_sync_by_oid(&corrupt).unwrap_err();
        // Then: pinning returns InternalError without creating a ref.
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert_no_ref(&repo, &corrupt);
        // Given: a fixture-owned ref lock makes the ref update fail.
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"forced ref failure",
            }])
            .unwrap();
        let lock = repo
            .get_root()
            .join(format!(".git/{}.lock", build_snapshot_ref(&tree)));
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, b"fixture owned lock").unwrap();
        // When: attempt to pin the tree with a locked ref.
        let error = git.tree_sync_by_oid(&tree).unwrap_err();
        // Then: pinning returns InternalError without creating a ref.
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert_no_ref(&repo, &tree);
        assert!(!format!("{error:?} {error}").contains("fixture owned lock"));
    }
}
#[test]
fn symbolic_and_conflicting_refs_are_rejected() {
    // Given: symbolic, dangling, conflicting, and malformed retention refs.
    for format in FORMATS {
        for conflict in ["symbolic", "dangling-symbolic", "other", "corrupt"] {
            let repo = TempRepo::new(format);
            let git = open_git_snapshots(repo.get_root()).unwrap();
            let tree = git.tree_create(&[]).unwrap();
            let before = repo.capture_fixture_state();
            let name = build_snapshot_ref(&tree);
            match conflict {
                "dangling-symbolic" => {
                    repo.run_git_successfully(&["symbolic-ref", &name, "refs/heads/absent"], None);
                }
                "symbolic" => {
                    let branch = repo.get_oid(&["symbolic-ref", "HEAD"], None);
                    repo.run_git_successfully(&["symbolic-ref", &name, &branch], None);
                }
                "other" => {
                    let head = repo.get_oid(&["rev-parse", "HEAD"], None);
                    repo.run_git_successfully(&["update-ref", &name, &head], None);
                }
                _ => {
                    let dest = repo.get_root().join(format!(".git/{name}"));
                    fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    fs::write(dest, b"CORRUPT_REF_CANARY\n").unwrap();
                }
            }
            // When: request lightweight retention and strong pinning.
            let light = git.ref_update_by_oid(&tree).unwrap_err();
            let err = git.tree_sync_by_oid(&tree).unwrap_err();
            // Then: both fail safely and preserve the user state.
            assert_eq!(light.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(err.get_kind(), SnapshotErrorKind::InternalError);
            assert!(!format!("{err} {err:?}").contains("CANARY"));
            assert_eq!(repo.capture_fixture_state(), before);
        }
    }
}
#[test]
fn concurrent_pin_is_idempotent() {
    if let Some(root) = std::env::var_os("LETOPIS_PIN_ROOT") {
        pin_worker(root);
        return;
    }
    // Given: two ready workers will pin the same tree.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let tree = open_git_snapshots(repo.get_root())
            .unwrap()
            .tree_create(&[])
            .unwrap();
        let children = spawn_pin_workers(repo.get_root(), &tree);
        let deadline = Instant::now() + Duration::from_secs(25);
        while !(repo.get_root().join("ready-a").exists()
            && repo.get_root().join("ready-b").exists())
        {
            if Instant::now() > deadline {
                panic!("workers not ready");
            }
            thread::sleep(Duration::from_millis(5));
        }
        // When: release both workers together and wait for their results.
        fs::write(repo.get_root().join("start"), b"go").unwrap();
        let outputs: Vec<_> = children
            .into_iter()
            .map(|child| child.wait(deadline))
            .collect();
        // Then: both workers succeed and the exact ref points to their tree.
        for out in outputs {
            assert!(
                out.status.success(),
                "{} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert_eq!(repo.get_direct_ref(tree.as_str()), tree.as_str());
    }
}
#[test]
fn promisor_miss_never_fetches() {
    if std::env::var_os("LETOPIS_PROMISOR_WORKER").is_none() {
        // Run the isolated worker before setting up the promisor fixture.
        git::supervise_test(
            "promisor_miss_never_fetches",
            &[("LETOPIS_PROMISOR_WORKER", "1")],
        );
        return;
    }
    // Given: a promised blob is missing and the configured transport records any fetch.
    for format in FORMATS {
        let remote = TempRepo::new(format);
        let repo = TempRepo::new(format);
        let blob = remote.get_oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"promised remote bytes"),
        );
        let tree = create_external_tree(&repo, &blob, "missing", BLOB_KIND);
        let marker = repo.get_root().join("FETCH_RAN");
        let script = repo.write_script(
            "transport",
            "#!/bin/sh\ntouch \"$(dirname \"$0\")/FETCH_RAN\"\nexec git-upload-pack \"$1\"\n",
        );
        let url = format!("ext::{} {}", script.display(), remote.get_root().display());
        repo.run_git_successfully(
            &["-c", "protocol.ext.allow=always", "ls-remote", &url],
            None,
        );
        // Fixture check: prove the remote transport trap is reachable.
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
            repo.run_git_successfully(&["config", key, value], None);
        }
        let git = open_git_snapshots(repo.get_root()).unwrap();
        // When: read the missing blob and pin its tree.
        let read = git
            .blob_get_by_tree_oid_and_path(&tree, &parse_path("missing"))
            .unwrap_err();
        let pin = git.tree_sync_by_oid(&tree).unwrap_err();
        // Then: both operations fail without fetching, creating an object, or adding a ref.
        assert_eq!(read.get_kind(), SnapshotErrorKind::InternalError);
        assert_eq!(pin.get_kind(), SnapshotErrorKind::InternalError);
        assert_no_ref(&repo, &tree);
        assert!(!marker.exists(), "no lazy transport is permitted");
        assert!(
            !repo
                .get_root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))
                .exists()
        );
    }
}

#[test]
fn conflicting_descendant_types_do_not_pin() {
    // Given: one OID is declared as both a tree and a blob.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let blob = repo.get_oid(
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
        let tree = parse_tree_oid(&repo.get_oid(
            &[
                "hash-object",
                "-w",
                "--literally",
                "-t",
                TREE_KIND,
                "--stdin",
            ],
            Some(&raw),
        ));
        // When: strongly pin the conflicting tree.
        let error = git.tree_sync_by_oid(&tree).unwrap_err();
        // Then: pinning reports InternalError and creates no ref.
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert_no_ref(&repo, &tree);
    }
}
#[test]
fn gitlinks_are_retained_without_dereferencing() {
    // Given: a tree has a gitlink to a missing commit.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let missing = "1".repeat(repo.get_oid(&["rev-parse", "HEAD"], None).len());
        let tree = parse_tree_oid(&repo.get_oid(
            &["mktree", "--missing", "-z"],
            Some(format!("{GITLINK_MODE} {COMMIT_KIND} {missing}\tmodule\0").as_bytes()),
        ));
        let git = open_git_snapshots(repo.get_root()).unwrap();
        // When: pin the tree and try to read the gitlink as a blob.
        git.tree_sync_by_oid(&tree).unwrap();
        let read = git
            .blob_get_by_tree_oid_and_path(&tree, &parse_path("module"))
            .unwrap_err();
        let integrity = repo.run_fsck_strict_no_reflogs();
        // Then: pinning succeeds, blob reading is invalid, and strict fsck succeeds.
        assert_eq!(read.get_kind(), SnapshotErrorKind::InvalidRequest);
        assert!(integrity.success());
    }
}

#[test]
fn shared_clone_is_refused_before_donor_gc_can_break_retention() {
    let mut refused = Vec::new();
    // Given
    for format in FORMATS {
        let donor = TempRepo::new(format);
        let bytes = b"ALTERNATE_PAYLOAD_CANARY unique unreferenced blob\0";
        let blob = donor.get_oid(&["hash-object", "-w", "--stdin"], Some(bytes));
        let borrower = donor.create_shared_clone();
        let donor_before = donor.capture_fixture_state();
        let borrower_before = borrower.capture_fixture_state();
        let alternates = borrower.get_root().join(".git/objects/info/alternates");
        let configuration = fs::read(&alternates).unwrap();
        // Fixture checks: the shared clone borrows the donor blob.
        assert!(
            borrower
                .run_git(&["cat-file", "-e", &blob], None)
                .status
                .success()
        );
        let local_blob =
            borrower
                .get_root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]));
        assert!(
            !local_blob.exists(),
            "the ordinary shared clone borrows this blob"
        );
        let p = parse_path("captured");
        // When: the helper preserves the unfixed adapter's accepted-snapshot path.
        let accepted = try_borrowed_snapshot(&donor, &borrower, &p, bytes, &local_blob);
        // When: actually remove the donor object before checking the outcome.
        donor.expire_reflogs_gc_prune_now_and_pack_refs();
        // Then
        assert_shared_clone_gc_outcome(&donor, &borrower, &blob, &p, &accepted, format);
        assert_eq!(donor.capture_fixture_state(), donor_before);
        assert_eq!(borrower.capture_fixture_state(), borrower_before);
        assert_eq!(fs::read(&alternates).unwrap(), configuration);
        refused.push(accepted.is_none());
    }
    // Then: neither object format may accept a borrowed snapshot.
    assert!(
        refused.iter().all(|r| *r),
        "shared clones must be refused before any snapshot is accepted: {refused:?}"
    );
}

#[test]
fn alternates_added_after_open_refuse_operations_without_writes() {
    // Given: open a repository before adding an alternates file.
    for format in FORMATS {
        let donor = TempRepo::new(format);
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let p = parse_path("captured");
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"local bytes",
            }])
            .unwrap();
        let alternates = repo.get_root().join(".git/objects/info/alternates");
        let configuration = format!("{}\n", donor.get_root().join(".git/objects").display());
        fs::write(&alternates, &configuration).unwrap();
        let before = repo.capture_fixture_state();
        let objects = repo.run_git_successfully(&["count-objects", "-v"], None);
        // When: create, pin, and read through the opened repository.
        let create = git.tree_create(&[SnapshotFile {
            path: &p,
            bytes: b"new ALTERNATE_PAYLOAD_CANARY",
        }]);
        let pin = git.tree_sync_by_oid(&tree);
        let read = git.blob_get_by_tree_oid_and_path(&tree, &p);
        // Then: each operation fails without changing objects, refs, or user state.
        for error in [create.err(), pin.err(), read.err()] {
            assert_alternates_error(
                &error.expect("alternates introduced after open must be refused"),
                &donor,
            );
        }
        assert_no_ref(&repo, &tree);
        assert_eq!(
            repo.run_git_successfully(&["count-objects", "-v"], None),
            objects
        );
        assert_eq!(repo.capture_fixture_state(), before);
        assert_eq!(fs::read(&alternates).unwrap(), configuration.as_bytes());
    }
}

#[test]
fn linked_worktree_checks_common_object_store_and_refuses_inactive_alternates() {
    // Given: a linked worktree uses the common object store.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let linked = add_linked_worktree(&repo);
        let git = open_git_snapshots(&linked).unwrap();
        let p = parse_path("linked");
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"linked bytes",
            }])
            .unwrap();
        // Fixture checks: the linked checkout works before alternates are added.
        git.tree_sync_by_oid(&tree).unwrap();
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(&tree, &p).unwrap(),
            b"linked bytes"
        );
        let alternates = repo.get_root().join(".git/objects/info/alternates");
        let before = repo.capture_fixture_state();
        let refs = repo.run_git_successfully(&["for-each-ref"], None);
        let objects = repo.run_git_successfully(&["count-objects", "-v"], None);
        // Given: the common store now has an empty alternates file and then an absent donor.
        for configuration in [b"".as_slice(), b"/ABSENT_DONOR_PATH_CANARY/objects\n"] {
            fs::write(&alternates, configuration).unwrap();
            // When: reopen and attempt snapshot pinning.
            let opened = open_git_snapshots(&linked);
            let pin = git.tree_sync_by_oid(&tree);
            // Then: both configurations are rejected without changing shared state.
            match opened {
                Ok(_) => {
                    panic!("linked worktree must reject even empty/inactive common alternates")
                }
                Err(error) => assert_alternates_error(&error, &repo),
            }
            assert_alternates_error(&pin.unwrap_err(), &repo);
            assert_eq!(fs::read(&alternates).unwrap(), configuration);
            assert_eq!(repo.capture_fixture_state(), before);
            assert_eq!(repo.run_git_successfully(&["for-each-ref"], None), refs);
        }
        // Given: remove the inactive alternates configuration.
        fs::remove_file(alternates).unwrap();
        // When: read the previously available object again.
        let bytes = open_git_snapshots(&linked)
            .unwrap()
            .blob_get_by_tree_oid_and_path(&tree, &p)
            .unwrap();
        // Then: access returns and the object count is unchanged.
        assert_eq!(
            repo.run_git_successfully(&["count-objects", "-v"], None),
            objects
        );
        assert_eq!(bytes, b"linked bytes");
    }
}

fn parse_path(text: &str) -> RepoPath {
    RepoPath::parse(text).unwrap()
}

fn create_external_tree(repo: &TempRepo, child: &str, name: &str, kind: &str) -> TreeOid {
    parse_tree_oid(
        &repo.get_oid(
            &["mktree", "-z", "--missing"],
            Some(
                format!(
                    "{} {kind} {child}\t{name}\0",
                    if kind == TREE_KIND {
                        TREE_MODE
                    } else {
                        REGULAR_FILE_MODE
                    }
                )
                .as_bytes(),
            ),
        ),
    )
}

fn assert_no_ref(repo: &TempRepo, tree: &TreeOid) {
    assert!(
        !repo
            .run_git(&["show-ref", "--verify", &build_snapshot_ref(tree)], None)
            .status
            .success()
    );
}

fn build_snapshot_ref(tree: &TreeOid) -> String {
    format!("{SNAPSHOT_REF_PREFIX}{}", tree.as_str())
}

fn pin_worker(root: OsString) {
    // Given: announce readiness, then wait for the parent barrier.
    let root = PathBuf::from(root);
    let tree = parse_tree_oid(&std::env::var("LETOPIS_PIN_OID").unwrap());
    let id = std::env::var("LETOPIS_PIN_WORKER").unwrap();
    fs::write(root.join(format!("ready-{id}")), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("start").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    // When
    let git = open_git_snapshots(&root).unwrap();
    git.tree_sync_by_oid(&tree).unwrap();
    // Then
}

fn parse_tree_oid(text: &str) -> TreeOid {
    TreeOid::parse(text).unwrap()
}

fn spawn_pin_workers(root: &Path, tree: &TreeOid) -> Vec<git::OwnedChild> {
    let mut children = Vec::new();
    for id in ["a", "b"] {
        let mut c = Command::new(std::env::current_exe().unwrap());
        git::isolate_command(&mut c);
        c.args(["--exact", "concurrent_pin_is_idempotent", "--nocapture"])
            .env("LETOPIS_PIN_ROOT", root)
            .env("LETOPIS_PIN_OID", tree.as_str())
            .env("LETOPIS_PIN_WORKER", id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        children.push(git::OwnedChild::spawn(&mut c));
    }
    children
}

fn try_borrowed_snapshot(
    donor: &TempRepo,
    borrower: &TempRepo,
    p: &RepoPath,
    bytes: &[u8],
    local_blob: &Path,
) -> Option<(impl GitSnapshots, TreeOid)> {
    // When
    let opened = open_git_snapshots(borrower.get_root());
    match opened {
        Ok(git) => {
            // On the unfixed adapter this recreates the public-API failure:
            // hash-object -w reuses the donor blob and pin appears successful.
            let tree = git.tree_create(&[SnapshotFile { path: p, bytes }]).unwrap();
            git.tree_sync_by_oid(&tree).unwrap();
            assert_eq!(git.blob_get_by_tree_oid_and_path(&tree, p).unwrap(), bytes);
            assert!(
                !local_blob.exists(),
                "hash-object -w must not be mistaken for materialization"
            );
            Some((git, tree))
        }
        Err(error) => {
            assert_alternates_error(&error, donor);
            None
        }
    }
}

fn assert_alternates_error(error: &SnapshotError, donor: &TempRepo) {
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert!(error.get_message().contains("alternates"));
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains(donor.get_root().to_str().unwrap()));
    assert!(!diagnostic.contains("CANARY"));
}

fn assert_shared_clone_gc_outcome(
    donor: &TempRepo,
    borrower: &TempRepo,
    blob: &str,
    p: &RepoPath,
    accepted: &Option<(impl GitSnapshots, TreeOid)>,
    format: ObjectFormat,
) {
    assert!(
        !donor
            .run_git(&["cat-file", "-e", blob], None)
            .status
            .success(),
        "donor GC really removed the unreferenced blob"
    );
    assert!(
        !borrower
            .run_git(&["cat-file", "-e", blob], None)
            .status
            .success()
    );
    if let Some((git, tree)) = accepted {
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(tree, p)
                .unwrap_err()
                .get_kind(),
            SnapshotErrorKind::InternalError
        );
        assert_eq!(borrower.get_direct_ref(tree.as_str()), tree.as_str());
        eprintln!(
            "{format:?}: accepted shared snapshot lost its donor blob after GC while its service ref remained exact"
        );
    } else {
        assert!(
            borrower
                .run_git_successfully(&["for-each-ref", SNAPSHOT_REF_PREFIX], None)
                .is_empty()
        );
    }
}

fn add_linked_worktree(repo: &TempRepo) -> PathBuf {
    let linked = repo.get_root().join("linked");
    repo.run_git_successfully(
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
        None,
    );
    linked
}
