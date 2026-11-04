use super::LocalFileSystem;
use crate::service::{FileErrorKind, FileReader, FileWriter};
use std::{
    error::Error,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
use tempfile::TempDir;

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
    // Then: bytes and stat agree and observations create no entries.
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
