mod discovery {
    use crate::process_test_support::{ObjectFormat, TempRepo};
    use crate::service::{DocumentErrorKind, Refresher};
    use crate::wire_document_refresher;
    use std::fs;

    #[test]
    fn discovery_policy_updates_membership_and_preserves_noop_revision() {
        // Given: an indexed untracked file and an already-open service.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join("note.md"), b"ordinary Markdown").unwrap();
        let mut service = wire_document_refresher(repo.get_root()).unwrap();
        assert_eq!(service.refresh(&[]).unwrap().revision, 1);
        // When: a new pattern excludes the existing owner.
        fs::write(repo.get_root().join(".letopis/ignore"), b"note.md\n").unwrap();
        let excluded = service.refresh(&[]).unwrap();
        // Then: exactly that file leaves the next revision.
        assert_eq!((excluded.files_removed, excluded.revision), (1, 2));
        // When: patterns are cleared and later replaced by a comment.
        fs::write(repo.get_root().join(".letopis/ignore"), b"").unwrap();
        let restored = service.refresh(&[]).unwrap();
        fs::write(
            repo.get_root().join(".letopis/ignore"),
            b"# no exclusions\n",
        )
        .unwrap();
        let unchanged = service.refresh(&[]).unwrap();
        // Then: restoration publishes once and equivalent policy does not publish.
        assert_eq!((restored.files_updated, restored.revision), (1, 3));
        assert!(!unchanged.is_published);
        assert_eq!(unchanged.revision, 3);
    }

    #[test]
    fn discovery_toggle_is_reread_by_existing_service() {
        // Given: standard Git ignores hide an untracked Markdown file.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join("ignored.md"), b"local knowledge").unwrap();
        fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
        let mut service = wire_document_refresher(repo.get_root()).unwrap();
        service.refresh(&[]).unwrap();
        // When: valid discovery settings disable Git exclusions despite invalid embedding settings.
        let config = repo.get_root().join(".letopis/config.json");
        fs::write(
            &config,
            br#"{"embedding_model":null,"discovery":{"respect_gitignore":false}}"#,
        )
        .unwrap();
        let included = service.refresh(&[]).unwrap();
        fs::write(
            &config,
            br#"{"embedding_model":null,"discovery":{"respect_gitignore":true}}"#,
        )
        .unwrap();
        let removed = service.refresh(&[]).unwrap();
        // Then: the same service observes both changes without resetting its repository.
        assert_eq!((included.files_updated, included.revision), (1, 2));
        assert_eq!((removed.files_removed, removed.revision), (1, 3));
    }

    #[test]
    fn discovery_same_stat_ignore_edit_is_observed() {
        // Given: two indexed files and an ignore file excluding the first.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        for name in ["a.md", "b.md"] {
            fs::write(repo.get_root().join(name), b"note").unwrap();
        }
        let mut service = wire_document_refresher(repo.get_root()).unwrap();
        let ignore = repo.get_root().join(".letopis/ignore");
        fs::write(&ignore, b"a.md\n").unwrap();
        service.refresh(&[]).unwrap();
        let modified = fs::metadata(&ignore).unwrap().modified().unwrap();
        // When: different equal-length patterns restore the original mtime.
        fs::write(&ignore, b"b.md\n").unwrap();
        fs::File::open(&ignore)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let outcome = service.refresh(&[]).unwrap();
        // Then: policy bytes are re-read and both membership transitions publish.
        assert_eq!(
            (
                outcome.files_updated,
                outcome.files_removed,
                outcome.revision
            ),
            (1, 1, 2)
        );
    }

    #[test]
    fn invalid_discovery_input_preserves_the_previous_publication() {
        // Given: an initialized publication and a malformed configuration file.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let mut service = wire_document_refresher(repo.get_root()).unwrap();
        service.refresh(&[]).unwrap();
        let config = repo.get_root().join(".letopis/config.json");
        fs::write(&config, b"{invalid}").unwrap();
        // When: refresh cannot interpret its discovery input.
        let error = service.refresh(&[]).unwrap_err();
        // Then: malformed input is fatal and removing it reveals the unchanged publication.
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        fs::remove_file(config).unwrap();
        let restored = service.refresh(&[]).unwrap();
        assert!(!restored.is_published);
        assert_eq!(restored.revision, 1);
    }
}

use crate::service::{
    Clock, FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult,
};

use std::rc::Rc;

use super::DocumentRefreshService;
use crate::service::TransactionError;
use crate::{
    domain::{RepoPath, TreeOid},
    markdown::MarkdownParser,
    service::{
        CacheDatabase, CapturedFile, Diagnostic, DocumentError, DocumentErrorKind, DocumentResult,
        FileStat, MarkdownCodec, MockGitWorktreeListing, ParsedFile, ProvenanceObservation,
        ProvenanceRequest, Refresher, SnapshotError, SnapshotRefsUpdater, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, StorageRead,
    },
};
use rusqlite::Connection;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::Path,
};
use tempfile::TempDir;

const INITIAL: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nAlphaToken\n";
const EDITED: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nBravoToken\n";
const NOW: i64 = 5_000_000_000;
type Candidates = MockGitWorktreeListing;

struct TestFiles {
    bytes: RefCell<Vec<u8>>,
    stat: Cell<FileStat>,
    now: Rc<Cell<i64>>,
    reads: Cell<usize>,
    locks: Cell<usize>,
    replace_on_read: Cell<Option<usize>>,
    fail_on_read: Cell<Option<usize>>,
    missing: Cell<bool>,
}

#[derive(Debug)]
struct TestClock(Rc<Cell<i64>>);

impl Clock for TestClock {
    fn clock_get_time_ns(&self) -> std::io::Result<i64> {
        Ok(self.0.get())
    }

    fn clock_get_time_secs(&self) -> std::io::Result<u64> {
        panic!("unused seconds")
    }

    fn clock_wait(&self, _: std::time::Duration) -> std::io::Result<()> {
        panic!("unused wait")
    }
}

#[derive(Debug)]
struct FailedClock;

impl Clock for FailedClock {
    fn clock_get_time_ns(&self) -> std::io::Result<i64> {
        Err(std::io::Error::other("clock unavailable"))
    }

    fn clock_get_time_secs(&self) -> std::io::Result<u64> {
        panic!("refresh does not request seconds")
    }

    fn clock_wait(&self, _: std::time::Duration) -> std::io::Result<()> {
        panic!("refresh does not wait")
    }
}

impl TestFiles {
    fn new() -> Self {
        Self {
            bytes: RefCell::new(INITIAL.to_vec()),
            stat: Cell::new(FileStat {
                size: u64::try_from(INITIAL.len()).unwrap(),
                mtime_ns: 1,
                inode: 2,
                device: 3,
            }),
            now: Rc::new(Cell::new(NOW)),
            reads: Cell::new(0),
            locks: Cell::new(0),
            replace_on_read: Cell::new(None),
            fail_on_read: Cell::new(None),
            missing: Cell::new(false),
        }
    }

    fn reset_counts(&self) {
        self.reads.set(0);
        self.locks.set(0);
    }
}

impl FileReader for TestFiles {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        Ok((!self.missing.get()).then(|| self.stat.get()))
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        assert!(matches!(
            path.to_str(),
            Some(".letopis/config.json" | ".letopis/ignore")
        ));
        Ok(None)
    }

    fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        let reads = self.reads.get() + 1;
        self.reads.set(reads);
        if self.fail_on_read.get() == Some(reads) {
            return Err(FileError::new(FileErrorKind::Io));
        }
        if self.replace_on_read.get() == Some(reads) {
            self.bytes.replace(EDITED.to_vec());
        }
        Ok((!self.missing.get()).then(|| CapturedFile {
            bytes: self.bytes.borrow().clone(),
            stat: self.stat.get(),
            checked_at_ns: self.now.get(),
        }))
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unexpected file_get_identity_by_path")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unexpected file_is_exist")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_identity_by_path")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_distinct_by_paths")
    }

    fn directory_get_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::os::fd::OwnedFd>> {
        Ok(Some(std::fs::File::open(".").unwrap().into()))
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        panic!("unexpected lock_get_owner_by_path")
    }
}

impl FileLocker for TestFiles {
    type Lock = ();

    fn lock_acquire_by_path(&self, _path: &std::path::Path) -> FileResult<Self::Lock> {
        self.locks.set(self.locks.get() + 1);
        Ok(())
    }

    fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
        panic!("unexpected lock probe")
    }
}

struct CountingMarkdown(Cell<usize>);

impl MarkdownCodec for CountingMarkdown {
    fn file_parse(
        &self,
        path: &RepoPath,
        bytes: &[u8],
    ) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
        self.0.set(self.0.get() + 1);
        MarkdownParser::new().file_parse(path, bytes)
    }
}

struct TestProvenance;

impl SourceProvenance for TestProvenance {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("this scenario does not validate requested revisions")
    }

    fn observe_sources(&self, _: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation> {
        Ok(ProvenanceObservation {
            flags: BTreeSet::new(),
            issues: BTreeSet::new(),
        })
    }

    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("refresh does not compare requested revisions")
    }
}

impl SnapshotRefsUpdater for TestProvenance {
    fn snapshot_sync_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert!(snapshots.is_empty());
        Ok(())
    }

    fn snapshot_refs_update_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert!(snapshots.is_empty());
        Ok(())
    }
}
type TestService = DocumentRefreshService<
    Candidates,
    TestFiles,
    CountingMarkdown,
    TestProvenance,
    crate::storage::CacheStorage,
>;

thread_local! {
    static ON_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

#[test]
fn nested_publication_rolls_back_with_its_parent() {
    use crate::service::CacheDatabase;
    // Given: a document service shares the parent's cache and an independent observer.
    let (directory, mut service) = build_service();
    let cache = std::rc::Rc::clone(&service.cache);
    let observer = Connection::open(directory.path().join("cache.db")).unwrap();
    // When: a nested refresh succeeds but its parent rejects the operation.
    let result = cache.with_write_tx(|| -> DocumentResult<()> {
        assert_eq!(service.refresh(&[])?.revision, 1);
        assert_eq!(
            observer
                .query_row("select revision from project", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        Err(DocumentError::new(
            DocumentErrorKind::InvalidRequest,
            "parent rejected publication",
        ))
    });
    // Then: neither the document nor its provisional revision is published.
    assert!(matches!(
        result,
        Err(TransactionError::Operation(error)) if error.get_kind() == DocumentErrorKind::InvalidRequest && error.get_message() == "parent rejected publication"
    ));
    assert_eq!(
        observer
            .query_row("select count(*) from nodes", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        observer
            .query_row("select revision from project", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(service.refresh(&[]).unwrap().revision, 1);
}

#[test]
fn joined_publications_advance_the_revision_once() {
    use crate::service::CacheDatabase;
    // Given: both refreshes share the same owning cache write.
    let (directory, mut service) = build_service();
    let cache = std::rc::Rc::clone(&service.cache);
    let observer = Connection::open(directory.path().join("cache.db")).unwrap();
    // When: the parent joins two successful publication operations.
    cache
        .with_write_tx(|| -> DocumentResult<()> {
            assert_eq!(service.refresh(&[])?.revision, 1);
            assert_eq!(service.refresh(&[])?.revision, 1);
            Ok(())
        })
        .unwrap();
    // Then: one node and one revision are published at the outer commit.
    assert_eq!(
        observer
            .query_row("select count(*) from nodes", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        observer
            .query_row("select revision from project", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn publication_returns_its_own_revision_after_a_later_commit() {
    // Given: an independent publisher runs immediately after the first commit.
    let (directory, mut service) = build_service();
    let path = directory.path().join("cache.db");
    ON_COMMIT.set(Some(Box::new(move || {
        Connection::open(path)
            .unwrap()
            .execute("update project set revision=2", [])
            .unwrap();
    })));
    // When: refresh finishes after the second publisher advances the database.
    let outcome = service.refresh(&[]).unwrap();
    // Then: it returns revision 1 even though revision 2 is already durable.
    assert_eq!(outcome.revision, 1);
    assert_eq!(
        Connection::open(directory.path().join("cache.db"))
            .unwrap()
            .query_row("select revision from project", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn skip_body_reads_for_unchanged_non_racy_marked_unmarked_and_invalid_files() {
    // Given
    for bytes in [
        INITIAL,
        &b"ordinary markdown\n"[..],
        &b"<!-- letopis: invalid -->"[..],
    ] {
        let (_dir, mut service) = build_service();
        service.filesystem.bytes.replace(bytes.to_vec());
        service.filesystem.stat.update(|mut stat| {
            stat.size = u64::try_from(bytes.len()).unwrap();
            stat
        });
        service.refresh(&[]).unwrap();
        service.filesystem.reset_counts();
        let parses = service.markdown.0.get();
        // When
        let result = service.refresh(&[]).unwrap();
        // Then
        assert!(!result.is_published);
        assert_eq!(result.revision, 1);
        assert_eq!(
            (
                service.filesystem.reads.get(),
                service.filesystem.locks.get()
            ),
            (0, 0)
        );
        assert_eq!(service.markdown.0.get(), parses);
    }
}

#[test]
fn detect_same_tick_equal_stat_edit_and_do_not_publish_unchanged_racy_bytes() {
    // Given: same-tick bytes with unchanged file statistics.
    let (_dir, mut service) = build_service();
    service.filesystem.stat.update(|mut stat| {
        stat.mtime_ns = NOW;
        stat
    });
    service.refresh(&[]).unwrap();
    let before = service
        .cache
        .with_read_tx(|| service.cache.files_get())
        .unwrap();
    service.filesystem.reset_counts();
    // When: refresh checks the captured bytes.
    let unchanged = service.refresh(&[]).unwrap();
    // Then: only changed stable bytes advance the publication.
    assert!(!unchanged.is_published);
    assert_eq!(
        (
            service.filesystem.reads.get(),
            service.filesystem.locks.get()
        ),
        (1, 0)
    );
    assert_eq!(
        service
            .cache
            .with_read_tx(|| service.cache.files_get())
            .unwrap(),
        before
    );
    // Given: exact same size, inode and mtime; only bytes differ.
    service.filesystem.bytes.replace(EDITED.to_vec());
    // When: refresh checks the captured bytes.
    let changed = service.refresh(&[]).unwrap();
    // Then: only changed stable bytes advance the publication.
    assert_eq!(changed.revision, 2);
    assert_eq!(get_matches(&mut service, "BravoToken"), ["note.md"]);
    assert!(get_matches(&mut service, "AlphaToken").is_empty());
}

#[test]
fn clock_failure_falls_back_to_bytes_without_publishing_unchanged_content() {
    // Given: an unchanged non-racy publication and an unavailable application clock.
    let (_dir, mut service) = build_service();
    service.refresh(&[]).unwrap();
    service.filesystem.reset_counts();
    service.clock = Box::new(FailedClock);
    // When: refresh cannot establish a safe age for the cached observation.
    let outcome = service.refresh(&[]).unwrap();
    // Then: verified bytes are read once, while publication and lock acquisition are skipped.
    assert!(!outcome.is_published);
    assert_eq!(service.filesystem.reads.get(), 1);
    assert_eq!(service.filesystem.locks.get(), 0);
}

#[test]
fn treat_racy_boundary_clock_regression_and_checked_arithmetic_conservatively() {
    // Given
    let cases = [
        (NOW - 1_000_000_000, NOW, NOW),
        (1, NOW, 2),
        (i64::MAX - 1, i64::MAX, i64::MAX),
        (0, i64::MIN, i64::MIN),
    ];
    for (mtime, checked, now) in cases {
        let (_dir, mut service) = build_service();
        service.filesystem.stat.update(|mut stat| {
            stat.mtime_ns = mtime;
            stat
        });
        service.filesystem.now.set(checked);
        service.refresh(&[]).unwrap();
        service.filesystem.reset_counts();
        service.filesystem.now.set(now);
        // When
        let result = service.refresh(&[]).unwrap();
        // Then
        assert!(!result.is_published);
        assert_eq!(
            service.filesystem.reads.get(),
            1,
            "case {mtime}/{checked}/{now}"
        );
        assert_eq!(service.filesystem.locks.get(), 0);
    }
}

#[test]
fn stat_only_change_preserves_projection_and_invalid_state_without_parsing() {
    // Given: valid and invalid publications with stat-only changes.
    for bytes in [INITIAL, &b"<!-- letopis: invalid -->"[..]] {
        let (dir, mut service) = build_service();
        service.filesystem.bytes.replace(bytes.to_vec());
        service.filesystem.stat.update(|mut stat| {
            stat.size = u64::try_from(bytes.len()).unwrap();
            stat
        });
        service.refresh(&[]).unwrap();
        let before_nodes = service
            .cache
            .with_read_tx(|| service.cache.nodes_get())
            .unwrap();
        let before_files = service
            .cache
            .with_read_tx(|| service.cache.files_get())
            .unwrap();
        let before_fts = get_fts_blocks(dir.path());
        let parses = service.markdown.0.get();
        service.filesystem.stat.update(|mut stat| {
            stat.inode += 1;
            stat
        });
        service.filesystem.now.set(NOW + 1);
        // When: refresh processes the changed file statistics.
        let outcome = service.refresh(&[]).unwrap();
        // Then: file state updates without reparsing or replacing the projection.
        assert!(outcome.is_published);
        assert_eq!(outcome.files_updated, 1);
        assert_eq!(service.markdown.0.get(), parses);
        assert_eq!(get_fts_blocks(dir.path()), before_fts);
        assert_eq!(
            service
                .cache
                .with_read_tx(|| service.cache.nodes_get())
                .unwrap(),
            before_nodes
        );
        let after_files = service
            .cache
            .with_read_tx(|| service.cache.files_get())
            .unwrap();
        assert_eq!(after_files[0].inode, before_files[0].inode + 1);
        assert_eq!(after_files[0].valid, before_files[0].valid);
        assert_eq!(after_files[0].error, before_files[0].error);
        assert_eq!(after_files[0].hash, before_files[0].hash);
        assert_eq!(after_files[0].checked_at_ns, NOW + 1);
    }
}

#[test]
fn changed_capture_between_locked_detection_and_preparation_aborts() {
    // Given: bytes that change between detection and preparation.
    let (_dir, mut service) = build_service();
    service.filesystem.replace_on_read.set(Some(3));
    // When: refresh captures the candidate.
    let result = service.refresh(&[]);
    // Then: inconsistent captures abort publication.
    assert_eq!(service.filesystem.reads.get(), 3);
    assert!(result.is_err());
    let cache = std::rc::Rc::clone(&service.cache);
    cache
        .with_read_tx(|| -> crate::service::StorageResult<()> {
            let read = cache.as_ref();
            assert_eq!(read.project_get_by_id().unwrap(), 0);
            assert!(read.files_get().unwrap().is_empty());
            assert!(read.nodes_get().unwrap().is_empty());

            Ok(())
        })
        .unwrap();
}

#[test]
fn reject_candidate_reappearing_before_publication() {
    // Given: an indexed document disappears from enumeration and then reappears.
    let (_dir, mut service) = build_service();
    service.refresh(&[]).unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = std::sync::Arc::clone(&calls);
    let mut candidates = Candidates::new();
    candidates
        .expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
        .returning(move |_, _| {
            let count = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if count < 3 {
                Ok(Vec::new())
            } else {
                Ok(b"note.md\0".to_vec())
            }
        });
    service.git = candidates;
    // When: refresh tries to publish the removal.
    let result = service.refresh(&[]);
    // Then: the race aborts publication and preserves the old projection.
    assert!(result.is_err());
    let cache = std::rc::Rc::clone(&service.cache);
    cache
        .with_read_tx(|| -> crate::service::StorageResult<()> {
            let read = cache.as_ref();
            assert_eq!(read.project_get_by_id().unwrap(), 1);
            assert_eq!(read.files_get().unwrap().len(), 1);
            assert_eq!(
                read.text_index_get_by_expression(
                    "AlphaToken",
                    std::num::NonZeroUsize::new(100).unwrap(),
                )
                .unwrap()
                .len(),
                1
            );

            Ok(())
        })
        .unwrap();
}

#[test]
fn fatal_read_is_not_an_invalid_markdown_publication() {
    // Given: a published projection and a filesystem failure on the next changed read.
    let (_dir, mut service) = build_service();
    service.refresh(&[]).unwrap();
    let before = service
        .cache
        .with_read_tx(|| service.cache.files_get())
        .unwrap();
    service.filesystem.reset_counts();
    service.filesystem.stat.update(|mut stat| {
        stat.inode += 1;
        stat
    });
    service.filesystem.fail_on_read.set(Some(3));
    // When / Then: refresh fails rather than publishing an invalid Markdown result.
    assert!(service.refresh(&[]).is_err());
    let cache = std::rc::Rc::clone(&service.cache);
    cache
        .with_read_tx(|| -> crate::service::StorageResult<()> {
            let read = cache.as_ref();
            // Then: revision, file state and searchable text retain the prior publication.
            assert_eq!(read.project_get_by_id().unwrap(), 1);
            assert_eq!(read.files_get().unwrap(), before);
            assert_eq!(
                read.text_index_get_by_expression(
                    "AlphaToken",
                    std::num::NonZeroUsize::new(100).unwrap(),
                )
                .unwrap()
                .len(),
                1
            );

            Ok(())
        })
        .unwrap();
}

#[test]
fn never_published_missing_candidate_is_not_a_delta_after_empty_initial_build() {
    // Given
    let (_dir, mut service) = build_service();
    service.filesystem.missing.set(true);
    assert_eq!(service.refresh(&[]).unwrap().revision, 1);
    service.filesystem.reset_counts();
    // When
    let result = service.refresh(&[]).unwrap();
    // Then
    assert!(!result.is_published);
    assert_eq!(
        (
            service.filesystem.reads.get(),
            service.filesystem.locks.get()
        ),
        (0, 0)
    );
    // Given
    service.filesystem.missing.set(false);
    // When / Then
    assert_eq!(service.refresh(&[]).unwrap().files_updated, 1);
}

#[test]
fn discard_preliminary_capture_and_publish_the_locked_observation() {
    // Given
    let (_dir, mut service) = build_service();
    service.refresh(&[]).unwrap();
    service.filesystem.reset_counts();
    service.filesystem.stat.update(|mut stat| {
        stat.inode += 1;
        stat
    });
    service.filesystem.replace_on_read.set(Some(2));
    // When
    let outcome = service.refresh(&[]).unwrap();
    // Then
    assert_eq!(outcome.revision, 2);
    assert_eq!(get_matches(&mut service, "BravoToken"), ["note.md"]);
    assert!(get_matches(&mut service, "AlphaToken").is_empty());
}

// A same-text DELETE/INSERT changes FTS segment bytes even if search results agree.

fn get_fts_blocks(root: &Path) -> Vec<(i64, Vec<u8>)> {
    let connection = Connection::open(root.join("cache.db")).unwrap();
    connection
        .prepare("select id, block from text_index_data order by id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn build_service() -> (TempDir, TestService) {
    let dir = tempfile::tempdir().unwrap();
    let storage = crate::storage::open_test_storage(
        &dir.path().join("cache.db"),
        &dir.path().join("vectors.db"),
    )
    .unwrap();
    let files = TestFiles::new();
    (
        dir,
        DocumentRefreshService {
            git: build_candidates(),
            clock: Box::new(crate::service::document_refresher::tests::TestClock(
                Rc::clone(&files.now),
            )),
            filesystem: files,
            markdown: CountingMarkdown(Cell::new(0)),
            cache: std::rc::Rc::new(storage.cache_storage),
            provenance: TestProvenance,
        },
    )
}

fn get_matches(service: &mut TestService, text: &str) -> Vec<String> {
    service
        .cache
        .with_read_tx(|| {
            service
                .cache
                .text_index_get_by_expression(text, std::num::NonZeroUsize::new(100).unwrap())
        })
        .unwrap()
        .into_iter()
        .map(|row| row.id.as_str().to_owned())
        .collect()
}

mod basis {
    use crate::service::{FileIdentity, FileLocker, FileReader, FileResult};

    use super::{Candidates, CountingMarkdown, TestFiles, build_service};
    use crate::service::TransactionError;
    use crate::service::{CacheDatabase, StorageRead};
    use crate::{
        domain::TreeOid,
        service::{
            CapturedFile, DocumentError, DocumentErrorKind, DocumentRefreshService, DocumentResult,
            FileStat, ProvenanceObservation, ProvenanceRequest, Refresher, SnapshotError,
            SnapshotRefsUpdater, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
        },
    };
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
    use tempfile::TempDir;

    const INJECTED_PROVENANCE_FAILURE: &str = "injected provenance failure";

    type Events = Rc<RefCell<Vec<&'static str>>>;
    type Service = DocumentRefreshService<
        Candidates,
        BoundaryFiles,
        CountingMarkdown,
        BoundaryProvenance,
        BoundaryStorage,
    >;
    struct BoundaryFiles {
        inner: TestFiles,
        events: Events,
    }
    struct BoundaryProvenance {
        batches: RefCell<Vec<usize>>,
        events: Events,
        failure: Option<&'static str>,
    }
    struct BoundaryStorage {
        inner: Rc<crate::storage::CacheStorage>,
        events: Events,
    }

    #[test]
    fn preflight_batches_only_prepared_bindings_in_bounded_chunks() {
        // Given: one document with 257 independently marked knowledge sections.
        let (_dir, mut service, events) = build_boundary_service(None);
        let bytes: String = (0..257)
            .map(|n| {
                format!("## Item {{#item{n}}}\n<!-- letopis: {{\"created_at\":1}} -->\nKnowledge\n")
            })
            .collect();
        service.filesystem.inner.stat.update(|mut stat| {
            stat.size = bytes.len() as u64;
            stat
        });
        service.filesystem.inner.bytes.replace(bytes.into_bytes());
        // When: preflight validates the prepared projection before publishing it.
        let outcome = service.refresh(&[]).unwrap();
        // Then: each binding is observed in bounded batches, before retention and SQL write.
        assert!(outcome.is_published);
        assert_eq!(*service.provenance.batches.borrow(), [256, 1]);
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "observe")
                .count(),
            257
        );
        let events = events.borrow();
        let retain = events.iter().position(|event| *event == "retain").unwrap();
        let begin = events.iter().position(|event| *event == "begin").unwrap();
        let last_observe = events
            .iter()
            .rposition(|event| *event == "observe")
            .unwrap();
        assert!(last_observe < retain);
        assert!(retain < begin);
    }

    #[test]
    fn verified_unchanged_refresh_audits_without_sql_write() {
        // Given: an already published document and no file delta.
        let (_dir, mut service, events) = build_boundary_service(None);
        service.refresh(&[]).unwrap();
        events.borrow_mut().clear();
        let parses = service.markdown.0.get();
        // When: full-corpus check requests a verified incremental refresh.
        let outcome = service.refresh_verified(&[]).unwrap();
        // Then: audit runs under lock, with no SQL write or body parse.
        assert_eq!(*events.borrow(), ["lock", "verify"]);
        assert!(!outcome.is_published);
        assert_eq!(service.markdown.0.get(), parses);
    }

    #[test]
    fn verified_refresh_failure_preserves_previous_publication() {
        // Given: a new projection whose source audit fails.
        let (_dir, mut service, events) = build_boundary_service(Some("verify"));
        // When: verification fails before any SQL write begins.
        assert!(service.refresh_verified(&[]).is_err());
        // Then: no partial graph is published.
        assert_eq!(
            *events.borrow(),
            ["lock", "read", "read", "observe", "verify"]
        );
        assert_eq!(
            service
                .cache
                .with_read_tx(|| service.cache.project_get_by_id())
                .unwrap(),
            0
        );
    }

    #[test]
    fn retain_under_index_lock_before_sql_publication() {
        // Given
        let (_dir, mut service, events) = build_boundary_service(None);
        // When
        let outcome = service.refresh(&[]).unwrap();
        // Then
        assert!(outcome.is_published);
        assert_eq!(outcome.revision, 1);
        assert_eq!(
            *events.borrow(),
            [
                "lock", "read", "read", "observe", "retain", "begin", "read", "commit"
            ]
        );
    }

    #[test]
    fn preflight_read_failure_never_opens_sql_write() {
        // Given
        let (_dir, mut service, events) = build_boundary_service(None);
        service.filesystem.inner.fail_on_read.set(Some(2));
        // When / Then
        assert!(service.refresh(&[]).is_err());
        // Then
        assert_eq!(*events.borrow(), ["lock", "read", "read"]);
    }

    #[test]
    fn observation_failure_never_opens_sql_write() {
        // Given
        let (_dir, mut service, events) = build_boundary_service(Some("observe"));
        // When / Then
        assert!(service.refresh(&[]).is_err());
        // Then
        assert_eq!(*events.borrow(), ["lock", "read", "read", "observe"]);
    }

    #[test]
    fn pin_failure_never_opens_sql_write() {
        // Given
        let (_dir, mut service, events) = build_boundary_service(Some("retain"));
        // When / Then
        assert!(service.refresh(&[]).is_err());
        // Then
        assert_eq!(
            *events.borrow(),
            ["lock", "read", "read", "observe", "retain"]
        );
    }

    #[test]
    fn changed_capture_after_preflight_rolls_back_without_commit() {
        // Given: a capture that changes after preflight.
        let (_dir, mut service, events) = build_boundary_service(None);
        service.filesystem.inner.replace_on_read.set(Some(3));
        // When / Then: refresh rejects the inconsistent capture; no cache commit occurs.
        assert!(service.refresh(&[]).is_err());
        // Then: no cache commit occurs.
        assert_eq!(
            *events.borrow(),
            ["lock", "read", "read", "observe", "retain", "begin", "read"]
        );
        assert_eq!(
            service
                .cache
                .with_read_tx(|| service.cache.project_get_by_id())
                .unwrap(),
            0
        );
    }

    #[test]
    fn unchanged_basis_retention_skips_sql_write_and_body_parse() {
        // Given
        let (_dir, mut service, events) = build_boundary_service(None);
        let tree = TreeOid::parse(&"1".repeat(40)).unwrap();
        let note = format!(
            "<!-- letopis: {{\"created_at\":1,\"source_snapshot\":\"{}\"}} -->\nKnowledge\n",
            tree.as_str()
        );
        service
            .filesystem
            .inner
            .bytes
            .replace(note.as_bytes().to_vec());
        service.filesystem.inner.stat.update(|mut stat| {
            stat.size = note.len() as u64;
            stat
        });
        service.refresh(&[]).unwrap();
        events.borrow_mut().clear();
        let parses = service.markdown.0.get();
        // When
        let outcome = service.refresh(&[]).unwrap();
        // Then
        assert!(!outcome.is_published);
        assert_eq!(outcome.revision, 1);
        assert_eq!(service.markdown.0.get(), parses);
        assert_eq!(*events.borrow(), ["lock", "retain"]);
    }

    #[test]
    fn initial_build_and_full_reindex_lock_before_capturing_and_verify_basis() {
        // Given.
        let (_dir, mut service, events) = build_boundary_service(None);
        // When: the first build follows the full path, not preliminary file capture.
        service.refresh(&[]).unwrap();
        // Then.
        assert_eq!(events.borrow().first(), Some(&"lock"));
        // Given
        events.borrow_mut().clear();
        let parses = service.markdown.0.get();
        // When
        service.reindex(&[]).unwrap();
        // Then
        assert_eq!(
            *events.borrow(),
            [
                "lock", "read", "read", "observe", "verify", "begin", "read", "commit"
            ]
        );
        assert!(service.markdown.0.get() > parses);
    }

    fn build_boundary_service(failure: Option<&'static str>) -> (TempDir, Service, Events) {
        let (dir, service) = build_service();
        let events = Rc::new(RefCell::new(Vec::new()));
        let service = DocumentRefreshService {
            git: service.git,
            clock: service.clock,
            filesystem: BoundaryFiles {
                inner: service.filesystem,
                events: Rc::clone(&events),
            },
            markdown: service.markdown,
            cache: Rc::new(BoundaryStorage {
                inner: service.cache,
                events: Rc::clone(&events),
            }),
            provenance: BoundaryProvenance {
                batches: RefCell::new(Vec::new()),
                events: Rc::clone(&events),
                failure,
            },
        };
        (dir, service, events)
    }

    impl FileReader for BoundaryFiles {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }

        fn file_get_stat_by_path(&self, path: &std::path::Path) -> FileResult<Option<FileStat>> {
            self.inner.file_get_stat_by_path(path)
        }

        fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
            self.inner.file_get_bytes_by_path(path)
        }

        fn file_get_by_path(&self, path: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            self.events.borrow_mut().push("read");
            self.inner.file_get_by_path(path)
        }

        fn file_get_identity_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<FileIdentity>> {
            panic!("unexpected file_get_identity_by_path")
        }

        fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            panic!("unexpected file_is_exist")
        }

        fn file_verify_identity_by_path(
            &self,
            _: &std::path::Path,
            _: Option<FileIdentity>,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_identity_by_path")
        }

        fn file_verify_distinct_by_paths(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_distinct_by_paths")
        }

        fn directory_get_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::os::fd::OwnedFd>> {
            Ok(Some(std::fs::File::open(".").unwrap().into()))
        }

        fn lock_get_owner_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::num::NonZeroU32>> {
            panic!("unexpected lock_get_owner_by_path")
        }
    }

    impl FileLocker for BoundaryFiles {
        type Lock = ();

        fn lock_acquire_by_path(&self, path: &std::path::Path) -> FileResult<Self::Lock> {
            self.events.borrow_mut().push("lock");
            self.inner.lock_acquire_by_path(path)
        }

        fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
            panic!("unexpected lock probe")
        }
    }

    impl SourceProvenance for BoundaryProvenance {
        fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            panic!("this scenario does not validate requested revisions")
        }

        fn observe_sources(
            &self,
            _: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            self.events.borrow_mut().push("observe");
            if self.failure == Some("observe") {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    INJECTED_PROVENANCE_FAILURE,
                ));
            }
            Ok(ProvenanceObservation {
                flags: BTreeSet::new(),
                issues: BTreeSet::new(),
            })
        }

        fn observe_sources_batch(
            &self,
            requests: &[ProvenanceRequest<'_>],
        ) -> DocumentResult<Vec<ProvenanceObservation>> {
            self.batches.borrow_mut().push(requests.len());
            requests
                .iter()
                .map(|request| self.observe_sources(*request))
                .collect()
        }

        fn compare_revision(
            &self,
            _: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            panic!("refresh must not compare requested revisions")
        }
    }

    impl SnapshotRefsUpdater for BoundaryProvenance {
        fn snapshot_sync_by_oids(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
            self.events.borrow_mut().push("verify");
            if self.failure == Some("verify") {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    INJECTED_PROVENANCE_FAILURE,
                ));
            }
            Ok(())
        }

        fn snapshot_refs_update_by_oids(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
            self.events.borrow_mut().push("retain");
            if self.failure == Some("retain") {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    INJECTED_PROVENANCE_FAILURE,
                ));
            }
            Ok(())
        }
    }
    use crate::service::cache_test_support::{
        delegate_cache_reads, delegate_cache_writes, delegate_graph_reads,
    };

    impl crate::service::StorageRead for BoundaryStorage {
        delegate_cache_reads!(self => self.inner.as_ref());
    }

    impl crate::service::GraphRead for BoundaryStorage {
        delegate_graph_reads!(self => self.inner.as_ref());
    }

    impl crate::service::CacheDatabase for BoundaryStorage {
        delegate_cache_writes!(self => self.inner.as_ref());

        fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_read_tx(operation)
        }

        fn with_status_read_tx<T, E, F>(
            &self,
            operation: F,
        ) -> Result<Option<T>, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_status_read_tx(operation)
        }

        fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.events.borrow_mut().push("begin");
            let result = self.inner.with_write_tx(operation);
            if result.is_ok() {
                self.events.borrow_mut().push("commit");
            }
            result
        }
    }
}
pub(super) mod process {
    //! Production-process schedules. Only this private test driver reads control variables.
    mod discovery {
        use super::{Fixture, NEW, Worker, get_state, get_vectors};
        use std::fs;

        #[test]
        fn discovery_edit_before_lock_is_redetected() {
            // Given: prepared file changes and either direction of an ignore-policy edit.
            for (initial, changed, includes_a, count) in [
                (b"".as_slice(), b"a.md\n".as_slice(), false, 2),
                (b"a.md\n".as_slice(), b"".as_slice(), true, 3),
            ] {
                let fixture = Fixture::new();
                fixture.edit(NEW);
                fs::write(fixture.root.path().join(".letopis/ignore"), initial).unwrap();
                let mut worker = Worker::spawn(fixture.root.path(), "publish");
                worker.reach("detected");
                // When: the author edits policy before locked redetection.
                fs::write(fixture.root.path().join(".letopis/ignore"), changed).unwrap();
                worker.resume();
                worker.finish();
                let state = get_state(fixture.root.path());
                // Then: the locked observation publishes only the newly selected owners.
                assert_eq!(state.revision, 2);
                assert_eq!(state.files.len(), count);
                assert_eq!(
                    state.files.iter().any(|row| row.path.as_str() == "a.md"),
                    includes_a
                );
            }
        }

        #[test]
        fn discovery_edit_during_preparation_rolls_back() {
            // Given / When / Then: changing policy after locked detection preserves the old publication.
            assert_policy_race(
                "redetected",
                "publish-policy-race",
                ".letopis/ignore",
                b"a.md\n",
            );
        }

        #[test]
        fn discovery_edit_before_commit_rolls_back() {
            // Given / When / Then: new exclusions and re-inclusions from either input cannot publish mixed policy.
            assert_policy_race(
                "before_commit",
                "publish-policy-race",
                ".letopis/ignore",
                b"a.md\n",
            );
            assert_policy_race(
                "before_commit",
                "publish-policy-race",
                ".letopis/config.json",
                br#"{"discovery":{"respect_gitignore":false}}"#,
            );
        }

        #[test]
        fn discovery_edit_before_noop_return_is_rejected() {
            // Given / When / Then: a preliminary no-op rechecks its original policy bytes.
            assert_policy_race(
                "noop_policy",
                "noop-policy-race",
                ".letopis/ignore",
                b"a.md\n",
            );
        }

        #[test]
        fn discovery_edit_before_locked_noop_return_is_rejected() {
            // Given / When / Then: verified no-op also rechecks after source validation.
            assert_policy_race(
                "locked_noop_policy",
                "verified-policy-race",
                ".letopis/ignore",
                b"a.md\n",
            );
        }

        fn assert_policy_race(checkpoint: &str, mode: &str, path: &str, changed: &[u8]) {
            let fixture = Fixture::new();
            if mode.starts_with("publish") {
                fixture.edit(NEW);
            }
            if path.ends_with("config.json") {
                fs::write(fixture.root.path().join(".gitignore"), b"a.md\n").unwrap();
            }
            let mut worker = Worker::spawn(fixture.root.path(), mode);
            worker.reach(checkpoint);
            fs::write(fixture.root.path().join(path), changed).unwrap();
            worker.resume();
            worker.finish();
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            assert_eq!(get_vectors(fixture.root.path()), fixture.vector_state);
        }
    }

    use super::super::DocumentRefreshService;
    use crate::service::TransactionError;
    use crate::service::{CacheDatabase, StorageRead};
    use crate::storage::get_current;
    use crate::{
        domain::{Edge, RepoPath, Sha256},
        filesystem::LocalFileSystem,
        git::GitRepository,
        markdown::MarkdownParser,
        service::{
            EmbeddingFailure, FileLocker, FileReader, FileRow, FileWriter, GraphDiagnosticsService,
            NodeMetadata, ProvenanceService, QueryError, Refresher, StatusReader, StatusReport,
            StatusService, StorageError, StorageResult, StoredNode, VectorDatabase, VectorOutcome,
        },
        wire_document_refresher,
    };
    use rusqlite::Connection;
    use std::{
        cell::RefCell,
        fs::{self, File},
        io::{BufRead, BufReader, Read, Write},
        os::unix::{
            fs::MetadataExt,
            net::{UnixListener, UnixStream},
            process::ExitStatusExt,
        },
        path::{Path, PathBuf},
        process::{Child, Command, ExitStatus, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    const TIME: i64 = 42;
    const WAIT: Duration = Duration::from_secs(20);
    const CHILD: &str = "service::document_refresher::tests::process::run_child";
    const SOURCE: &[u8] = b"external source\r\nexact bytes\n";
    const OLD: &str = "<!-- letopis: {\"created_at\":1} -->\nAlphaToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
    const NEW: &str = "<!-- letopis: {\"created_at\":3} -->\nBravoToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
    const LATEST: &str = "<!-- letopis: {\"created_at\":3} -->\nCharlieToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
    const DIAGNOSTIC_CANARY: &str = "TK28_DIAGNOSTIC_CANARY";
    const INVALID: &str = "<!-- letopis: invalid TK28_DIAGNOSTIC_CANARY -->\n";
    const RETAINED: &str = "<!-- letopis: {\"created_at\":2} -->\nRetainedToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
    const CUTS: [&str; 10] = [
        "detected",
        "locked",
        "redetected",
        "begun",
        "cleared",
        "projections",
        "fts",
        "fingerprints",
        "before_commit",
        "after_commit",
    ];
    type Service = DocumentRefreshService<
        GitRepository,
        LocalFileSystem,
        MarkdownParser,
        ProvenanceService<LocalFileSystem>,
        crate::storage::CacheStorage,
    >;
    type Tokens = Vec<(String, i64, String, i64)>;
    thread_local! {
        static CONTROL: RefCell<Option<UnixStream>> = const { RefCell::new(None) };
        static FAIL_LATE: RefCell<bool> = const { RefCell::new(false) };
    }

    pub(in crate::service) fn reach_checkpoint(name: &str) {
        if name == "after_commit"
            && let Some(hook) = super::ON_COMMIT.take()
        {
            hook();
        }
        CONTROL.with_borrow_mut(|control| {
            if let Some(stream) = control {
                writeln!(stream, "{name}").unwrap();
                eprintln!("checkpoint {name}");
                let mut response = [0];
                stream.read_exact(&mut response).unwrap();
                assert_eq!(response, [b'c']);
            }
        });
    }

    pub(in crate::service) fn fail_late<C: crate::service::CacheDatabase>(
        unit: &C,
    ) -> StorageResult<()> {
        if FAIL_LATE.with_borrow(|enabled| *enabled) {
            return unit.files_upsert_by_path(&FileRow {
                path: RepoPath::parse("invalid-size.md").unwrap(),
                size: u64::MAX,
                mtime_ns: 0,
                inode: 0,
                hash: Sha256::digest(b"overflow"),
                checked_at_ns: TIME,
                valid: true,
                error: None,
            });
        }
        Ok(())
    }

    #[test]
    #[ignore = "subprocess entry; invoked only by the production schedule tests"]
    fn run_child() {
        // Given
        let socket = std::env::var_os("TK10_SOCKET").expect("private child control");
        let root = PathBuf::from(std::env::var_os("TK10_ROOT").unwrap());
        let mode = std::env::var("TK10_MODE").unwrap();
        let stream = UnixStream::connect(socket).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        CONTROL.with_borrow_mut(|control| *control = Some(stream));
        reach_checkpoint("connected");
        // When
        if let Some(mode) = mode.strip_prefix("normal-") {
            run_normal_service(&root, mode);
            reach_checkpoint("finished");
            return;
        }
        let mut service = build_service(&root);
        if mode == "hold" {
            let _guard = service
                .filesystem
                .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                .unwrap();
            reach_checkpoint("holding");
        } else {
            FAIL_LATE.with_borrow_mut(|enabled| *enabled = mode.ends_with("fail"));
            let result = if mode.starts_with("reindex") {
                service.reindex(&[])
            } else if mode.starts_with("verified") {
                service.refresh_verified(&[])
            } else {
                service.refresh(&[])
            };
            eprintln!("refresh result: {result:?}");
            if mode.ends_with("policy-race") {
                assert_eq!(
                    result.unwrap_err().get_message(),
                    "documents changed during refresh"
                );
            } else if mode.ends_with("fail") {
                let error = result.unwrap_err();
                // Then
                assert!(
                    std::error::Error::source(&error)
                        .unwrap()
                        .downcast_ref::<StorageError>()
                        .is_some()
                );
                assert!(!format!("{error:?}").contains(DIAGNOSTIC_CANARY));
            } else {
                let result = result.unwrap();
                assert_eq!(result.is_published, mode != "noop");
            }
        }
        drop(service);
        reach_checkpoint("finished");
    }

    #[test]
    fn normal_open_waits_at_index_lock_during_cache_write_then_redetects() {
        // Given: the owner holds a real cache transaction, not just index.lock.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let mut owner = Worker::spawn(fixture.root.path(), "publish");
        owner.reach("begun");
        // When: normal composition must open before the owner releases its SQLite writer.
        let mut waiter = Worker::spawn(fixture.root.path(), "normal-noop");
        waiter.reach("opened");
        waiter.resume();
        waiter.reach("contended");
        owner.resume();
        owner.finish();
        waiter.resume();
        waiter.finish();
        // Then: the waiter redetects the committed publication and does not republish.
        assert_eq!(get_state(fixture.root.path()), expected);
        fixture.assert_preserved(NEW);
    }

    #[test]
    fn normal_open_redetects_after_active_cache_writer_is_killed() {
        // Given a real transaction with an old publication still visible.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let mut owner = Worker::spawn(fixture.root.path(), "publish");
        owner.reach("begun");
        let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
        waiter.reach("contended");
        // When: change the file while waiting, then release the writer through SIGKILL.
        fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
        let expected = fixture.build_expected(LATEST, "CharlieToken");
        owner.kill();
        waiter.resume();
        waiter.finish();
        // Then: only the newest complete publication becomes visible.
        assert_eq!(get_state(fixture.root.path()), expected);
        fixture.assert_preserved(LATEST);
    }

    #[test]
    fn normal_open_and_noop_finish_during_vector_write_and_held_index_lock() {
        // Given: independent live vector and kernel-lock owners.
        let fixture = Fixture::new();
        let mut owner = Worker::spawn(fixture.root.path(), "hold");
        owner.reach("holding");
        let storage = crate::storage::open_test_storage(
            &fixture.root.path().join(".letopis/cache.db"),
            &fixture.root.path().join(".letopis/vectors.db"),
        )
        .unwrap();
        let identity = Sha256::digest(b"model identity");
        assert!(matches!(
            storage.vector_storage.with_write_tx(&identity, || {
                let unit = &storage.vector_storage;
                unit.query_error_upsert_by_id(99, "uncommitted").unwrap();
                // When: normal composition and a no-op must finish with both owners still active.
                Worker::spawn(fixture.root.path(), "normal-noop").finish();
                // Then: neither the cache nor the committed vector state was changed.
                assert_eq!(get_state(fixture.root.path()), fixture.old);
                assert_eq!(get_vectors(fixture.root.path()), fixture.vector_state);

                Err::<(), _>(())
            }),
            Err(TransactionError::Operation(()))
        ));

        owner.kill();
    }

    struct Worker {
        child: Child,
        stream: BufReader<UnixStream>,
        _socket: TempDir,
        receipt: File,
    }

    impl Worker {
        fn spawn(root: &Path, mode: &str) -> Self {
            let socket = tempfile::Builder::new()
                .prefix("tk10-")
                .tempdir_in("/tmp")
                .unwrap();
            let listener = UnixListener::bind(socket.path().join("control")).unwrap();
            let evidence = std::env::var_os("LETOPIS_TEST_EVIDENCE")
                .map_or_else(|| PathBuf::from(".artifacts/tk10/process"), PathBuf::from);
            fs::create_dir_all(&evidence).unwrap();
            let name = socket.path().file_name().unwrap().to_str().unwrap();
            let log = File::create(evidence.join(format!("{name}.log"))).unwrap();
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD, "--ignored", "--nocapture"])
                .env("TK10_SOCKET", socket.path().join("control"))
                .env("TK10_ROOT", root)
                .env("TK10_MODE", mode)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(log)
                .spawn()
                .unwrap();
            let (sender, receiver) = mpsc::channel();
            let accept = thread::spawn(move || sender.send(listener.accept()).ok());
            let accepted = receiver.recv_timeout(WAIT);
            if accepted.is_err() {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                // Unblock the accept thread before unwinding on a failed startup.
                let _ = UnixStream::connect(socket.path().join("control"));
                let _ = accept.join();
                panic!("child did not connect: {accepted:?}");
            }
            let (stream, _) = accepted.unwrap().unwrap();
            accept.join().unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            stream.set_write_timeout(Some(WAIT)).unwrap();
            let receipt = File::create(evidence.join(format!("{name}.receipt"))).unwrap();
            let mut worker = Self {
                child,
                stream: BufReader::new(stream),
                _socket: socket,
                receipt,
            };
            writeln!(worker.receipt, "mode={mode} pid={}", worker.child.id()).unwrap();
            assert_eq!(worker.get_event(), "connected");
            worker.resume();
            worker
        }

        fn get_event(&mut self) -> String {
            let mut line = String::new();
            assert!(
                self.stream.read_line(&mut line).unwrap() > 0,
                "child exited before acknowledgement"
            );
            writeln!(self.receipt, "{}", line.trim()).unwrap();
            line.trim().to_owned()
        }

        fn resume(&mut self) {
            self.stream.get_mut().write_all(b"c").unwrap();
        }

        fn reach(&mut self, target: &str) {
            loop {
                let event = self.get_event();
                if event == target {
                    return;
                }
                assert_ne!(event, "finished", "checkpoint {target} was never reached");
                self.resume();
            }
        }

        fn finish(mut self) {
            self.reach("finished");
            self.resume();
            let status = self.wait_exit();
            writeln!(self.receipt, "exit={status}").unwrap();
            assert!(status.success(), "{status}");
        }

        fn wait_exit(&mut self) -> ExitStatus {
            let deadline = Instant::now() + WAIT;
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "child did not exit within 20 seconds"
                );
                // Only an exit watchdog; schedules advance exclusively by acknowledgements.
                thread::sleep(Duration::from_millis(1));
            }
        }

        fn kill(mut self) {
            self.child.kill().unwrap();
            let status = self.wait_exit();
            writeln!(self.receipt, "signal={:?}", status.signal()).unwrap();
            assert_eq!(status.signal(), Some(9));
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct State {
        revision: u64,
        nodes: Vec<StoredNode>,
        edges: Vec<Edge>,
        files: Vec<FileRow>,
        tokens: Tokens,
    }
    #[derive(Debug, PartialEq)]
    struct VectorState {
        identity: Option<Sha256>,
        vector: VectorOutcome<Option<Vec<f32>>>,
        failure: VectorOutcome<Option<EmbeddingFailure>>,
        query_error: Option<QueryError>,
    }
    struct Fixture {
        root: TempDir,
        old: State,
        vectors: Vec<u8>,
        vector_state: VectorState,
        git_objects: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            assert!(
                Command::new("git")
                    .args(["init", "-q"])
                    .arg(root.path())
                    .status()
                    .unwrap()
                    .success()
            );
            for (name, bytes) in [
                ("a.md", OLD),
                (
                    "b.md",
                    "<!-- letopis: {\"created_at\":1} -->\nDeletedToken\n",
                ),
                ("i.md", RETAINED),
                (
                    "z.md",
                    "<!-- letopis: {\"created_at\":1} -->\nStableToken\n",
                ),
            ] {
                fs::write(root.path().join(name), bytes).unwrap();
            }
            fs::write(root.path().join("source.txt"), SOURCE).unwrap();
            let git_objects = get_git_objects(root.path());
            let mut service = build_service(root.path());
            assert_eq!(service.refresh(&[]).unwrap().revision, 1);
            assert_eq!(get_git_objects(root.path()), git_objects);
            seed_vectors(
                &crate::storage::open_test_storage(
                    &root.path().join(".letopis/cache.db"),
                    &root.path().join(".letopis/vectors.db"),
                )
                .unwrap(),
            );
            drop(service);
            let old = get_state(root.path());
            let vectors = fs::read(root.path().join(".letopis/vectors.db")).unwrap();
            let vector_state = get_vectors(root.path());
            Self {
                root,
                old,
                vectors,
                vector_state,
                git_objects,
            }
        }

        fn edit(&self, bytes: &str) {
            fs::write(self.root.path().join("a.md"), bytes).unwrap();
            fs::write(self.root.path().join("i.md"), INVALID).unwrap();
            fs::remove_file(self.root.path().join("b.md")).unwrap();
        }

        fn invalidate_retained_file(&self) {
            fs::write(self.root.path().join("i.md"), INVALID).unwrap();
        }

        fn restore_retained_file(&self) {
            fs::write(self.root.path().join("i.md"), RETAINED).unwrap();
        }

        fn assert_preserved(&self, bytes: &str) {
            assert_eq!(
                fs::read(self.root.path().join("a.md")).unwrap(),
                bytes.as_bytes()
            );
            assert_eq!(
                fs::read(self.root.path().join("i.md")).unwrap(),
                INVALID.as_bytes()
            );
            assert_eq!(
                fs::read(self.root.path().join("source.txt")).unwrap(),
                SOURCE
            );
            assert_eq!(
                fs::read(self.root.path().join("z.md")).unwrap(),
                b"<!-- letopis: {\"created_at\":1} -->\nStableToken\n"
            );
            assert!(!self.root.path().join("b.md").exists());
            assert_eq!(get_git_objects(self.root.path()), self.git_objects);
            assert_eq!(get_vectors(self.root.path()), self.vector_state);
            assert_eq!(
                fs::read(self.root.path().join(".letopis/vectors.db")).unwrap(),
                self.vectors
            );
        }

        fn build_expected(&self, bytes: &str, token: &str) -> State {
            let mut expected = self.old.clone();
            expected.revision = 2;
            expected.nodes.retain(
                |node| !matches!(&node.metadata, NodeMetadata::Knowledge(k) if k.id.as_str() == "b.md"),
            );
            for node in &mut expected.nodes {
                match &mut node.metadata {
                    NodeMetadata::Knowledge(k) if k.id.as_str() == "a.md" => {
                        k.hash = Sha256::digest(bytes.as_bytes());
                        k.search_hash = Sha256::digest(format!("{token} source").as_bytes());
                        k.created_at = 3;
                    }
                    NodeMetadata::Source(s) => s.created_at = 2,
                    _ => {}
                }
            }
            expected.files.retain(|file| file.path.as_str() != "b.md");
            for file in &mut expected.files {
                let contents = match file.path.as_str() {
                    "a.md" => bytes,
                    "i.md" => INVALID,
                    _ => continue,
                };
                // Independent oracle: observe the file with std, not the adapter under test.
                let metadata = fs::metadata(self.root.path().join(file.path.as_str())).unwrap();
                file.size = metadata.len();
                file.mtime_ns = metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec();
                file.inode = metadata.ino();
                file.hash = Sha256::digest(contents.as_bytes());
                file.checked_at_ns = TIME;
                if file.path.as_str() == "i.md" {
                    file.valid = false;
                    file.error = Some("invalid letopis metadata".to_owned());
                }
            }
            expected
                .tokens
                .retain(|(term, _, _, _)| term != "deletedtoken");
            for (term, _, _, _) in &mut expected.tokens {
                if term == "alphatoken" {
                    *term = token.to_lowercase();
                }
            }
            expected.tokens.sort();
            expected
        }
    }
    #[test]
    fn sigkill_keeps_exact_old_or_complete_new_publication() {
        for cut in CUTS {
            // Given: independently captured old state and literal expected new state.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let expected = fixture.build_expected(NEW, "BravoToken");
            let mut worker = Worker::spawn(fixture.root.path(), "publish");
            worker.reach(cut);
            if matches!(cut, "cleared" | "projections" | "fts" | "fingerprints") {
                // The first file is a deletion. Cut the second file after real replacements.
                worker.resume();
                worker.reach(cut);
            }
            // When: genuine process death at an acknowledged operation boundary.
            worker.kill();
            // Then: reopen, including SQLite recovery, before retrying refresh.
            let actual = get_state(fixture.root.path());
            assert_eq!(
                actual,
                if cut == "after_commit" {
                    &expected
                } else {
                    &fixture.old
                }
                .clone(),
                "cut={cut}"
            );
            let filesystem = LocalFileSystem::new(fixture.root.path()).unwrap();
            let guard = filesystem
                .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
                .unwrap();
            drop(guard);
            fixture.assert_preserved(NEW);
            Worker::spawn(
                fixture.root.path(),
                if cut == "after_commit" {
                    "noop"
                } else {
                    "publish"
                },
            )
            .finish();
            assert_eq!(
                get_state(fixture.root.path()),
                expected,
                "restart cut={cut}"
            );
            fixture.assert_preserved(NEW);
        }
    }

    #[test]
    fn duplicate_publishers_commit_once_after_real_contention() {
        // Given
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let mut owner = Worker::spawn(fixture.root.path(), "publish");
        owner.reach("locked");
        let mut waiter = Worker::spawn(fixture.root.path(), "noop");
        waiter.reach("contended");
        // When
        owner.resume();
        owner.finish();
        waiter.resume();
        waiter.finish();
        // Then
        assert_eq!(get_state(fixture.root.path()), expected);
        fixture.assert_preserved(NEW);
    }

    #[test]
    fn waiting_publisher_redetects_newer_edit_after_owner_is_killed() {
        // Given
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let mut owner = Worker::spawn(fixture.root.path(), "hold");
        owner.reach("holding");
        let mut waiter = Worker::spawn(fixture.root.path(), "publish");
        waiter.reach("contended");
        fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
        let expected = fixture.build_expected(LATEST, "CharlieToken");
        let lock_before = fs::metadata(fixture.root.path().join(".letopis/index.lock")).unwrap();
        // When
        owner.kill();
        waiter.resume();
        waiter.finish();
        // Then
        assert_eq!(
            fs::metadata(fixture.root.path().join(".letopis/index.lock"))
                .unwrap()
                .ino(),
            lock_before.ino()
        );
        assert_eq!(get_state(fixture.root.path()), expected);
        fixture.assert_preserved(LATEST);
    }

    #[test]
    fn noop_finishes_while_another_process_holds_index_lock() {
        // Given
        let fixture = Fixture::new();
        let mut owner = Worker::spawn(fixture.root.path(), "hold");
        owner.reach("holding");
        // When
        Worker::spawn(fixture.root.path(), "noop").finish();
        // Then
        assert_eq!(get_state(fixture.root.path()), fixture.old);
        owner.kill();
    }

    #[test]
    fn pinned_wal_reader_keeps_old_snapshot_and_new_reader_sees_complete_commit() {
        // Given: a WAL reader pinned before another publication.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let storage = build_service(fixture.root.path()).cache;
        storage
            .with_read_tx(|| -> crate::service::StorageResult<()> {
                let pinned = storage.as_ref();
                let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
                fts.execute_batch("begin;").unwrap();
                assert_eq!(get_tokens(&fts), fixture.old.tokens);
                // When: a separate worker publishes changed contents.
                Worker::spawn(fixture.root.path(), "publish").finish();
                // Then: old and new readers see complete distinct revisions.
                assert_eq!(pinned.project_get_by_id().unwrap(), fixture.old.revision);
                assert_eq!(pinned.nodes_get().unwrap(), fixture.old.nodes);
                assert_eq!(
                    pinned.edges_get_by_source_id(None).unwrap(),
                    fixture.old.edges
                );
                assert_eq!(pinned.files_get().unwrap(), fixture.old.files);
                assert_eq!(get_tokens(&fts), fixture.old.tokens);
                assert_eq!(get_state(fixture.root.path()), expected);
                fixture.assert_preserved(NEW);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn late_storage_error_rolls_back_every_cache_mutation() {
        // Given
        let fixture = Fixture::new();
        fixture.edit(NEW);
        // When
        Worker::spawn(fixture.root.path(), "fail").finish();
        // Then
        assert_eq!(get_state(fixture.root.path()), fixture.old);
        fixture.assert_preserved(NEW);
        // When
        Worker::spawn(fixture.root.path(), "publish").finish();
        // Then
        assert_eq!(
            get_state(fixture.root.path()),
            fixture.build_expected(NEW, "BravoToken")
        );
    }

    #[test]
    fn uncommitted_file_diagnostic_is_absent_from_status() {
        // Given: a valid publication, then one authored file becomes invalid.
        let fixture = Fixture::new();
        fixture.invalidate_retained_file();

        // When: the cache commit fails after the invalid file projection is staged.
        Worker::spawn(fixture.root.path(), "fail").finish();

        // Then: the old publication remains and a fresh status derives no file problem.
        assert_eq!(get_state(fixture.root.path()), fixture.old);
        let status = get_status(fixture.root.path());
        assert_eq!(status.revision, Some(1));
        assert!(get_invalid_file_problems(&status).is_empty());
        assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
    }

    #[test]
    fn failed_publication_retry_persists_the_file_diagnostic() {
        // Given: a failed publication with one invalid authored file.
        let fixture = Fixture::new();
        fixture.invalidate_retained_file();
        Worker::spawn(fixture.root.path(), "fail").finish();

        // When: the injected fault is gone and publication retries.
        Worker::spawn(fixture.root.path(), "publish").finish();

        // Then: the real safe diagnostic is persisted and survives a fresh status.
        let status = get_status(fixture.root.path());
        assert_eq!(status.revision, Some(2));
        assert_eq!(
            get_invalid_file_problems(&status),
            vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
        );
        assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
    }

    #[test]
    fn repaired_file_problem_disappears_after_publication() {
        // Given: a publication that stored the invalid-file diagnostic.
        let fixture = Fixture::new();
        fixture.invalidate_retained_file();
        Worker::spawn(fixture.root.path(), "publish").finish();
        assert_eq!(
            get_invalid_file_problems(&get_status(fixture.root.path())),
            vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
        );

        // When: the authored file is repaired and republished.
        fixture.restore_retained_file();
        Worker::spawn(fixture.root.path(), "publish").finish();

        // Then: the current problem disappears.
        let status = get_status(fixture.root.path());
        assert_eq!(status.revision, Some(3));
        assert!(get_invalid_file_problems(&status).is_empty());
    }

    fn run_normal_service(root: &Path, mode: &str) {
        crate::process::SystemClock::configure_next(TIME);
        LocalFileSystem::configure_next_file_test(TIME, Box::new(|| reach_checkpoint("contended")));
        let mut service = wire_document_refresher(root).unwrap();
        reach_checkpoint("opened");
        let outcome = service.refresh(&[]).unwrap();
        assert_eq!(outcome.is_published, mode != "noop");
    }

    fn get_state(root: &Path) -> State {
        let storage = build_service(root).cache;
        storage
            .with_read_tx(|| {
                let read = storage.as_ref();
                let connection = Connection::open(root.join(".letopis/cache.db")).unwrap();
                Ok::<_, StorageError>(State {
                    revision: read.project_get_by_id().unwrap(),
                    nodes: read.nodes_get().unwrap(),
                    edges: read.edges_get_by_source_id(None).unwrap(),
                    files: read.files_get().unwrap(),
                    tokens: get_tokens(&connection),
                })
            })
            .unwrap()
    }

    fn get_tokens(connection: &Connection) -> Tokens {
        connection.execute_batch("create virtual table if not exists temp.tokens using fts5vocab(main, text_index, instance);").unwrap();
        connection
            .prepare(
                "select term, doc, col, offset from temp.tokens order by term, doc, col, offset",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn get_vectors(root: &Path) -> VectorState {
        let storage = build_service(root).cache;
        storage
            .with_read_tx(|| {
                let read = storage.as_ref();
                let identity = Sha256::digest(b"model identity");
                Ok::<_, StorageError>(VectorState {
                    identity: read.meta_get_by_id().unwrap(),
                    vector: read
                        .vector_keys_get_by_search_hash(
                            &identity,
                            &Sha256::digest(b"retained vector"),
                        )
                        .unwrap(),
                    failure: read
                        .embed_failures_get_by_search_hash(
                            &identity,
                            &Sha256::digest(b"failed vector"),
                        )
                        .unwrap(),
                    query_error: read.query_error_get_by_id().unwrap(),
                })
            })
            .unwrap()
    }

    fn get_status(root: &Path) -> StatusReport {
        let git = GitRepository::open(root, &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let files = LocalFileSystem::new(git.worktree_root_get()).unwrap();
        let pinned = files
            .directory_get_by_path(std::path::Path::new(".letopis"))
            .unwrap()
            .unwrap();
        let storage =
            crate::open_observed_cache(&git.worktree_root_get().join(".letopis"), &pinned).unwrap();
        let provenance = ProvenanceService::new(
            Box::new(GitRepository::open(root, &[crate::utils::SERVICE_DIRECTORY]).unwrap()),
            LocalFileSystem::new(root).unwrap(),
        );
        let provider = crate::build_embedding_provider().unwrap();
        let storage = std::rc::Rc::new(storage);
        let graph = GraphDiagnosticsService::new(std::rc::Rc::clone(&storage));
        let mut status = StatusService {
            cache: storage,
            file_reader: &files,
            graph: &graph,
            provenance: &provenance,
            provider: &provider,
        };
        status.get_status().unwrap()
    }

    fn get_invalid_file_problems(report: &StatusReport) -> Vec<(String, String)> {
        report
            .problems
            .iter()
            .filter_map(|problem| {
                let path = problem.details.get("path")?.as_str()?.to_owned();
                let reason = problem.details.get("reason")?.as_str()?.to_owned();
                (path == "i.md" && reason == "invalid_metadata").then_some((path, reason))
            })
            .collect()
    }

    fn build_service(root: &Path) -> Service {
        let git = GitRepository::open(root, &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let mut filesystem = LocalFileSystem::new(git.worktree_root_get()).unwrap();
        filesystem.configure_file_test(TIME, Box::new(|| reach_checkpoint("contended")));
        let directory = filesystem
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();
        let storage = crate::storage::open_confined_test_storage(
            &git.worktree_root_get().join(".letopis"),
            &directory,
        )
        .unwrap();
        let provenance = ProvenanceService::new(
            Box::new(GitRepository::open(root, &[crate::utils::SERVICE_DIRECTORY]).unwrap()),
            LocalFileSystem::new(root).unwrap(),
        );
        DocumentRefreshService {
            git,
            clock: Box::new(crate::process::SystemClock::at(TIME)),
            filesystem,
            markdown: MarkdownParser::new(),
            cache: std::rc::Rc::new(storage.cache_storage),
            provenance,
        }
    }

    fn get_git_objects(root: &Path) -> Vec<u8> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["count-objects", "-v"])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }

    fn seed_vectors(storage: &crate::storage::TestDatabases) {
        let identity = Sha256::digest(b"model identity");
        storage
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        storage
            .vector_storage
            .with_write_tx(&identity, || {
                let unit = &storage.vector_storage;
                unit.store_vector(&Sha256::digest(b"retained vector"), &[1.0, 0.0], 2)
                    .unwrap();
                let failure = Sha256::digest(b"failed vector");
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&failure, 3)
                        .unwrap()
                );
                unit.embed_failures_update_by_search_hash(&failure, "safe failure")
                    .unwrap();
                unit.query_error_upsert_by_id(7, "safe query error")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
    }

    mod reindex {
        use super::{Fixture, LATEST, NEW, Worker, build_service, get_state, get_tokens};
        use crate::service::{CacheDatabase, Refresher, StorageRead};
        use rusqlite::Connection;
        use std::{fs, os::unix::process::ExitStatusExt};

        #[test]
        fn reindex_sigint_preserves_exact_old_or_complete_new_state() {
            for cut in ["locked", "begun", "fts", "before_commit", "after_commit"] {
                // Given: the actual child pauses at an acknowledged operation boundary.
                let fixture = Fixture::new();
                fixture.edit(NEW);
                let expected = fixture.build_expected(NEW, "BravoToken");
                let mut worker = Worker::spawn(fixture.root.path(), "reindex");
                worker.reach(cut);
                // When.
                let pid = rustix::process::Pid::from_raw(i32::try_from(worker.child.id()).unwrap())
                    .unwrap();
                rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
                let status = worker.wait_exit();
                // Then: signal death is distinct from TK-29 handled JSON cancellation.
                assert_eq!(status.signal(), Some(2));
                assert_eq!(
                    get_state(fixture.root.path()),
                    if cut == "after_commit" {
                        expected
                    } else {
                        fixture.old.clone()
                    }
                );
                fixture.assert_preserved(NEW);
                // The process's kernel lease has been released; a fresh call can publish.
                let mut service = build_service(fixture.root.path());
                assert!(service.reindex(&[]).unwrap().is_published);
            }
        }

        #[test]
        fn reindex_keeps_pinned_wal_view_and_publishes_one_complete_new_view() {
            // Given: pinned graph and FTS views with a worker paused before commit.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let expected = fixture.build_expected(NEW, "BravoToken");
            let storage = build_service(fixture.root.path()).cache;
            storage
                .with_read_tx(|| -> crate::service::StorageResult<()> {
                    let pinned = storage.as_ref();
                    let fts =
                        Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
                    fts.execute_batch("begin").unwrap();
                    assert_eq!(get_tokens(&fts), fixture.old.tokens);
                    let mut worker = Worker::spawn(fixture.root.path(), "reindex");
                    worker.reach("before_commit");
                    assert_eq!(get_state(fixture.root.path()), fixture.old);
                    // When: the reindex worker is allowed to commit.
                    worker.resume();
                    worker.finish();
                    // Then: old readers keep the old state and a fresh read sees the complete new state.
                    assert_eq!(pinned.project_get_by_id().unwrap(), 1);
                    assert_eq!(pinned.nodes_get().unwrap(), fixture.old.nodes);
                    assert_eq!(
                        pinned.edges_get_by_source_id(None).unwrap(),
                        fixture.old.edges
                    );
                    assert_eq!(pinned.files_get().unwrap(), fixture.old.files);
                    assert_eq!(get_tokens(&fts), fixture.old.tokens);
                    assert_eq!(get_state(fixture.root.path()), expected);

                    Ok(())
                })
                .unwrap();
        }

        #[test]
        fn publisher_waits_for_reindex_and_redetects_latest_file_bytes() {
            // Given: reindex finishes capturing the old bytes before the next edit.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let mut owner = Worker::spawn(fixture.root.path(), "reindex");
            owner.reach("before_commit");
            let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
            waiter.reach("contended");
            fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
            let mut expected = fixture.build_expected(LATEST, "CharlieToken");
            expected.revision = 3;
            // When.
            owner.resume();
            owner.finish();
            waiter.resume();
            waiter.finish();
            // Then.
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(LATEST);
        }

        #[test]
        fn reindex_late_sql_error_rolls_back_all_state_and_releases_lock() {
            // Given.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            // When.
            Worker::spawn(fixture.root.path(), "reindex-fail").finish();
            // Then.
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            fixture.assert_preserved(NEW);
            Worker::spawn(fixture.root.path(), "reindex").finish();
            assert_eq!(
                get_state(fixture.root.path()),
                fixture.build_expected(NEW, "BravoToken")
            );
        }
    }
}

mod reading {
    use super::{CountingMarkdown, TestFiles, TestProvenance, build_candidates, build_service};
    use crate::service::CacheDatabase;
    use crate::service::TransactionError;
    use crate::{
        domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
        service::{
            DocumentRefreshService, DocumentResult, ElementAddress, EmbeddingFailure,
            EmbeddingWork, FileRow, GraphNeighbor, GraphNodeRef, GraphRead, NeighborPageRequest,
            NodeMetadata, ProvenanceObservation, ProvenanceRequest, QueryError, Refresher,
            SnapshotError, SnapshotRefsUpdater, SourceProvenance, SourceRevisionComparison,
            SourceRevisionRequest, StorageRead, StorageResult, StoredNode, TextMatch, VectorMatch,
            VectorOutcome,
        },
    };
    use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};

    struct ReadingStorage {
        inner: Rc<crate::storage::CacheStorage>,
        reads: Rc<Cell<usize>>,
    }

    struct RetainingProvenance;

    #[test]
    fn warm_refresh_reads_basis_roots_without_loading_nodes() {
        // Given: a warm projection with declared basis roots.
        let (_root, mut initial) = build_service();
        initial.refresh(&[]).unwrap();
        let mut node = initial
            .cache
            .with_read_tx(|| initial.cache.nodes_get())
            .unwrap()
            .remove(0)
            .metadata;
        let NodeMetadata::Knowledge(ref mut knowledge) = node else {
            panic!()
        };
        knowledge.source_snapshot = Some(TreeOid::parse(&"a".repeat(40)).unwrap());
        initial
            .cache
            .with_write_tx(|| initial.cache.nodes_upsert_by_id(&node))
            .unwrap();
        let reads = Rc::new(Cell::new(0));
        let files = TestFiles::new();
        let mut service = DocumentRefreshService {
            git: build_candidates(),
            clock: Box::new(crate::service::document_refresher::tests::TestClock(
                Rc::clone(&files.now),
            )),
            filesystem: files,
            markdown: CountingMarkdown(Cell::new(0)),
            cache: Rc::new(ReadingStorage {
                inner: initial.cache,
                reads: Rc::clone(&reads),
            }),
            provenance: RetainingProvenance,
        };
        // When: refresh checks the unchanged corpus.
        let result = service.refresh(&[]).unwrap();
        // Then: basis roots are observed without loading node metadata.
        assert!(!result.is_published);
        assert_eq!(result.revision, 2);
        assert_eq!(reads.get(), 2);
        assert_eq!(service.filesystem.locks.get(), 1);
        assert_eq!(service.filesystem.reads.get(), 0);
        assert_eq!(service.markdown.0.get(), 0);
    }

    use crate::service::cache_test_support::delegate_cache_writes;

    impl crate::service::CacheDatabase for ReadingStorage {
        delegate_cache_writes!(self => self.inner.as_ref());

        fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_read_tx(operation)
        }

        fn with_status_read_tx<T, E, F>(
            &self,
            operation: F,
        ) -> Result<Option<T>, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_status_read_tx(operation)
        }

        fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_write_tx(operation)
        }
    }

    impl GraphRead for ReadingStorage {
        fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
            GraphRead::nodes_get_by_uri(self.inner.as_ref(), uri)
        }

        fn edges_get_by_row_id_and_type(
            &self,
            request: &NeighborPageRequest<'_>,
        ) -> StorageResult<Vec<GraphNeighbor>> {
            self.inner.edges_get_by_row_id_and_type(request)
        }
    }

    impl StorageRead for ReadingStorage {
        fn nodes_get_by_kind(&self) -> StorageResult<u64> {
            self.inner.nodes_get_by_kind()
        }

        fn nodes_get_coverage_by_kind(&self) -> StorageResult<u64> {
            self.inner.nodes_get_coverage_by_kind()
        }

        fn embed_failures_get_by_kind_and_search_hash_and_attempts(
            &self,
            max_attempts: u64,
        ) -> StorageResult<Vec<EmbeddingFailure>> {
            self.inner
                .embed_failures_get_by_kind_and_search_hash_and_attempts(max_attempts)
        }

        fn nodes_get(&self) -> StorageResult<Vec<StoredNode>> {
            panic!("warm refresh loaded node metadata")
        }

        fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>> {
            self.reads.set(self.reads.get() + 1);
            self.inner.nodes_get_by_source_snapshot()
        }

        fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
            &self,
            expected: &Sha256,
            max_attempts: u64,
            after: Option<&KnowledgeId>,
        ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
            self.inner
                .nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
                    expected,
                    max_attempts,
                    after,
                )
        }

        fn vector_index_config_get_by_id(
            &self,
            expected: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
            self.inner.vector_index_config_get_by_id(expected)
        }

        fn nodes_get_by_uri(&self, uri: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
            StorageRead::nodes_get_by_uri(self.inner.as_ref(), uri)
        }

        fn edges_get_by_source_id(&self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
            self.inner.edges_get_by_source_id(source)
        }

        fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
            self.inner.files_get_by_path(path)
        }

        fn files_get(&self) -> StorageResult<Vec<FileRow>> {
            self.inner.files_get()
        }

        fn text_index_get_by_expression(
            &self,
            expression: &str,
            limit: NonZeroUsize,
        ) -> StorageResult<Vec<TextMatch>> {
            self.inner.text_index_get_by_expression(expression, limit)
        }

        fn vector_keys_get_by_search_hash(
            &self,
            expected: &Sha256,
            search_hash: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
            self.inner
                .vector_keys_get_by_search_hash(expected, search_hash)
        }

        fn vector_index_get_by_vector(
            &self,
            expected: &Sha256,
            query: &[f32],
            dimensions: usize,
            limit: NonZeroUsize,
        ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
            self.inner
                .vector_index_get_by_vector(expected, query, dimensions, limit)
        }

        fn embed_failures_get_by_search_hash(
            &self,
            expected: &Sha256,
            search_hash: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
            self.inner
                .embed_failures_get_by_search_hash(expected, search_hash)
        }

        fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>> {
            self.inner.query_error_get_by_id()
        }

        fn project_get_by_id(&self) -> StorageResult<u64> {
            self.inner.project_get_by_id()
        }

        fn meta_get_by_id(&self) -> StorageResult<Option<Sha256>> {
            self.inner.meta_get_by_id()
        }
    }

    impl SourceProvenance for RetainingProvenance {
        fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            TestProvenance.validate_revisions(revisions)
        }

        fn observe_sources(
            &self,
            request: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            TestProvenance.observe_sources(request)
        }

        fn compare_revision(
            &self,
            request: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            TestProvenance.compare_revision(request)
        }
    }

    impl SnapshotRefsUpdater for RetainingProvenance {
        fn snapshot_sync_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
            assert_eq!(
                snapshots,
                &BTreeSet::from([
                    TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
                ])
            );
            Ok(())
        }

        fn snapshot_refs_update_by_oids(
            &self,
            snapshots: &BTreeSet<TreeOid>,
        ) -> DocumentResult<()> {
            assert_eq!(
                snapshots,
                &BTreeSet::from([
                    TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
                ])
            );
            Ok(())
        }
    }
}

mod reindex {
    use super::{INITIAL, build_service, get_matches};
    use crate::service::{CacheDatabase, Refresher, StorageRead};
    use rusqlite::Connection;

    #[test]
    fn reindex_rebuilds_unchanged_fts_and_advances_once() {
        // Given: FTS is deliberately missing while file fingerprints are unchanged.
        let (dir, mut service) = build_service();
        service.refresh(&[]).unwrap();
        let db = Connection::open(dir.path().join("cache.db")).unwrap();
        db.execute("delete from text_index", []).unwrap();
        assert!(!service.refresh(&[]).unwrap().is_published);
        assert!(get_matches(&mut service, "AlphaToken").is_empty());
        // When.
        let rebuilt = service.reindex(&[]).unwrap();
        // Then: this must not be an incremental no-op.
        assert_eq!(
            (
                rebuilt.revision,
                rebuilt.is_published,
                rebuilt.files_updated
            ),
            (2, true, 1)
        );
        assert_eq!(get_matches(&mut service, "AlphaToken"), ["note.md"]);
        assert_eq!(service.reindex(&[]).unwrap().revision, 3);
    }

    #[test]
    fn reindex_reparses_identical_valid_unmarked_and_invalid_bytes() {
        for bytes in [
            INITIAL,
            &b"ordinary markdown\n"[..],
            &b"<!-- letopis: invalid -->"[..],
        ] {
            // Given.
            let (_dir, mut service) = build_service();
            service.filesystem.bytes.replace(bytes.to_vec());
            service.filesystem.stat.update(|mut stat| {
                stat.size = bytes.len() as u64;
                stat
            });
            service.refresh(&[]).unwrap();
            let parses = service.markdown.0.get();
            service.filesystem.reset_counts();
            // When.
            let outcome = service.reindex(&[]).unwrap();
            // Then.
            assert_eq!(outcome.revision, 2);
            assert!(service.markdown.0.get() > parses);
            assert_eq!(service.filesystem.locks.get(), 1);
            assert_eq!(outcome.files_updated, 1);
        }
    }

    #[test]
    fn reindex_capture_failure_preserves_complete_publication() {
        // Given: fail the publication capture after detection and preflight.
        let (_dir, mut service) = build_service();
        service.refresh(&[]).unwrap();
        let before = service
            .cache
            .with_read_tx(|| service.cache.files_get())
            .unwrap();
        service.filesystem.reset_counts();
        service.filesystem.fail_on_read.set(Some(3));
        // When: reindex fails during publication capture.
        assert!(service.reindex(&[]).is_err());
        // Then: the prior revision, file state and FTS entry remain intact.
        let cache = std::rc::Rc::clone(&service.cache);
        cache
            .with_read_tx(|| -> crate::service::StorageResult<()> {
                let read = cache.as_ref();
                assert_eq!(read.project_get_by_id().unwrap(), 1);
                assert_eq!(read.files_get().unwrap(), before);
                assert_eq!(
                    read.text_index_get_by_expression(
                        "AlphaToken",
                        std::num::NonZeroUsize::new(100).unwrap(),
                    )
                    .unwrap()[0]
                        .id
                        .as_str(),
                    "note.md"
                );

                Ok(())
            })
            .unwrap();
    }
}

fn build_candidates() -> Candidates {
    let mut candidates = Candidates::new();
    candidates
        .expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
        .returning(|_, _| Ok(b"note.md\0".to_vec()));
    candidates
}

mod corpus {
    use crate::filesystem::LocalFileSystem;
    use crate::git::GitRepository;
    use crate::process_test_support::{ObjectFormat, TempRepo};
    use crate::service::document_refresher::DocumentRefreshService;
    use crate::service::utils::parse_candidates;
    use crate::{domain::RepoPath, service::DocumentErrorKind};
    use std::fs;
    use std::{path::Path, rc::Rc};

    #[test]
    fn candidates_ignore_symlinks_with_unsupported_names() {
        // Given: a tracked symlink with a name invalid as a document path.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let root = repo.get_root();
        std::os::unix::fs::symlink("target", root.join("bad#anchor.md")).unwrap();
        repo.run_git_successfully(&["add", "bad#anchor.md"], None);
        // When: the application selects document candidates from raw Git paths.
        let candidates = build_selector(root)
            .unwrap()
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: the symlink is not validated as a document path.
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            ["live.md"]
        );
    }

    #[test]
    fn candidates_exclude_live_symlinks_independently_of_index_mode() {
        // Given: tracked and untracked paths whose live types differ from the index.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let root = repo.get_root();
        fs::write(root.join("regular.md"), b"regular").unwrap();
        fs::write(root.join("changed-to-link.md"), b"regular").unwrap();
        std::os::unix::fs::symlink("regular.md", root.join("changed-to-file.md")).unwrap();
        std::os::unix::fs::symlink("regular.md", root.join("tracked-link.md")).unwrap();
        repo.run_git_successfully(
            &[
                "add",
                "regular.md",
                "changed-to-link.md",
                "changed-to-file.md",
                "tracked-link.md",
            ],
            None,
        );
        fs::remove_file(root.join("changed-to-link.md")).unwrap();
        std::os::unix::fs::symlink("regular.md", root.join("changed-to-link.md")).unwrap();
        fs::remove_file(root.join("changed-to-file.md")).unwrap();
        fs::write(root.join("changed-to-file.md"), b"now regular").unwrap();
        std::os::unix::fs::symlink("regular.md", root.join("untracked-link.md")).unwrap();
        // When: application selection observes current filesystem types for raw Git paths.
        let candidates = build_selector(root)
            .unwrap()
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: only live regular files and the fixture's ordinary Markdown are returned.
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            ["changed-to-file.md", "live.md", "regular.md"]
        );
    }

    #[test]
    fn candidates_follow_git_ignore_and_preserve_deleted_tracked_paths() {
        // Given: deleted tracked, tracked ignored, untracked ignored, and private paths coexist.
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
        let git = build_selector(repo.get_root()).unwrap();
        // When: the application selects current document candidates.
        let candidates = git.get_candidate_observation_by_paths(&[]).unwrap().paths;
        // Then: deleted and ignored tracked paths remain; ignored untracked and private paths do not. User state is unchanged.
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
    fn shared_repository_observes_index_and_ignore_edits_between_listings() {
        for format in crate::process_test_support::FORMATS {
            // Given: one shared repository whose index and ignore stack have already been read.
            let repo = TempRepo::new(format);
            fs::write(repo.get_root().join("ignored.md"), b"ignored").unwrap();
            fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
            let git = build_selector(repo.get_root()).unwrap();
            let shared = DocumentRefreshService {
                git: git.git.clone(),
                filesystem: LocalFileSystem::new(repo.get_root()).unwrap(),
                clock: Box::new(crate::process::SystemClock::new()),
                markdown: (),
                cache: Rc::new(()),
                provenance: (),
            };
            assert_eq!(
                git.get_candidate_observation_by_paths(&[]).unwrap().paths,
                [RepoPath::parse("live.md").unwrap()]
            );
            // When: Git stages an ignored file and removes the previous index entry.
            repo.run_git_successfully(&["add", "-f", "ignored.md"], None);
            repo.run_git_successfully(&["rm", "--cached", "-f", "live.md"], None);
            fs::write(repo.get_root().join(".gitignore"), b"ignored.md\nlive.md\n").unwrap();
            // Then: the clone sees the new index and fresh ignore bytes without reopening.
            assert_eq!(
                shared
                    .get_candidate_observation_by_paths(&[])
                    .unwrap()
                    .paths,
                [RepoPath::parse("ignored.md").unwrap()]
            );
            // When: remove the tracked file and make the other live file eligible again.
            repo.run_git_successfully(&["rm", "--cached", "-f", "ignored.md"], None);
            fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
            // Then: the original handle also refreshes its index/exclude observation.
            assert_eq!(
                git.get_candidate_observation_by_paths(&[]).unwrap().paths,
                [RepoPath::parse("live.md").unwrap()]
            );
        }
    }

    #[test]
    fn candidates_reject_unsupported_raw_paths() {
        use crate::service::DocumentErrorKind;
        for raw in [
            b"bad\xff.md".to_vec(),
            b"bad#anchor.md".to_vec(),
            b"bad\nline.md".to_vec(),
        ] {
            // Given: indexed Markdown names contain invalid UTF-8, a hash, or a newline.
            let repo = TempRepo::new(ObjectFormat::Sha1);
            // Git's index supports raw paths even when the host filesystem rejects them.
            let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
            let mut entry = format!("100644 {oid}\t").into_bytes();
            entry.extend(raw);
            entry.push(0);
            repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
            let git = build_selector(repo.get_root()).unwrap();
            // When: the application selects current document candidates.
            let error = git.get_candidate_observation_by_paths(&[]).unwrap_err();
            // Then: unsupported names produce InvalidRequest without exposing the filename.
            assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
            assert!(!error.to_string().contains("bad"));
        }
    }

    #[test]
    fn candidates_deduplicate_unmerged_index_stages() {
        // Given: three conflict stages address the same conflict.md alongside live.md.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"conflict"));
        let entries = format!(
            "0 {}\tconflict.md\n100644 {oid} 1\tconflict.md\n100644 {oid} 2\tconflict.md\n100644 {oid} 3\tconflict.md\n",
            "0".repeat(40)
        );
        repo.run_git_successfully(&["update-index", "--index-info"], Some(entries.as_bytes()));
        let git = build_selector(repo.get_root()).unwrap();
        // When: the application selects current document candidates.
        let candidates = git.get_candidate_observation_by_paths(&[]).unwrap().paths;
        // Then: conflict.md appears once beside live.md.
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            ["conflict.md", "live.md"]
        );
    }

    #[test]
    fn candidates_exclude_metadata_before_validating_paths() {
        for raw in [
            b".letopis/bad#anchor.md".as_slice(),
            b".letopis/bad\xff.md",
            b"nested/.letopis/bad\xff.markdown",
        ] {
            // Given: Git can list metadata names unsupported by the corpus model.
            let repo = TempRepo::new(ObjectFormat::Sha1);
            fs::write(repo.get_root().join(".letopis-notes.md"), b"candidate").unwrap();
            let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
            let mut entry = format!("100644 {oid}\t").into_bytes();
            entry.extend(raw);
            entry.push(0);
            repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
            let git = build_selector(repo.get_root()).unwrap();
            // When: the application selects current document candidates.
            let candidates = git.get_candidate_observation_by_paths(&[]).unwrap().paths;
            // Then: .letopis components are excluded before decoding; .letopis-notes.md and live.md remain.
            assert_eq!(
                candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
                [".letopis-notes.md", "live.md"]
            );
        }
    }

    #[test]
    fn candidates_exclude_raw_git_metadata_components() {
        // Given: a raw response contains .git components and the ordinary .git-notes.md name.
        let raw = b".git/bad#anchor.md\0nested/.git/bad\xff.markdown\0.git-notes.md\0";
        // When: the application parses the raw response.
        let candidates = parse_candidates(raw).unwrap();
        // Then: only .git-notes.md remains.
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            [".git-notes.md"]
        );
    }

    #[test]
    fn rejects_unterminated_candidates_and_accepts_empty_output() {
        // Given: an unterminated candidate response and an empty response.
        // When: parse each response independently.
        let malformed = parse_candidates(b"note.md").unwrap_err();
        let empty = parse_candidates(b"").unwrap();
        // Then: the unterminated response is InternalError and the empty response has no candidates.
        assert_eq!(malformed.get_kind(), DocumentErrorKind::InternalError);
        assert!(empty.is_empty());
    }

    #[test]
    fn corpus_matches_git_for_nested_excludes_staged_deletion_and_symlinks() {
        // Given: index membership and three exclude sources disagree with live files.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        prepare_exclude_fixture(&repo);
        let before = repo.capture_fixture_state();
        let expected = [
            "dir.md/inside.md",
            "live.md",
            "module.md",
            "nested/keep.md",
            "staged-delete.md",
            "symlink.md",
            "tracked-ignore.md",
        ];
        let oracle = repo.run_git_successfully(
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            None,
        );
        let mut oracle: Vec<_> = oracle
            .split(|byte| *byte == 0)
            .filter(|path| path.ends_with(b".md"))
            .map(|path| std::str::from_utf8(path).unwrap())
            .collect();
        oracle.sort_unstable();
        assert_eq!(oracle, expected);
        // When: the library lists the current worktree's real index and ignore stack.
        let actual = build_selector(repo.get_root())
            .unwrap()
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: all eligible oracle paths except the live symlink are returned.
        assert_eq!(
            actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            [
                "dir.md/inside.md",
                "live.md",
                "module.md",
                "nested/keep.md",
                "staged-delete.md",
                "tracked-ignore.md"
            ]
        );
        assert_eq!(repo.capture_fixture_state(), before);
    }

    #[test]
    fn linked_worktree_listing_uses_its_own_index() {
        // Given: two worktrees share objects but have distinct staged paths.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let parent = tempfile::tempdir().unwrap();
        let linked = parent.path().join("linked");
        repo.run_git_successfully(
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
            None,
        );
        fs::write(linked.join("linked-only.md"), b"linked").unwrap();
        repo.run_git_successfully(
            &["-C", linked.to_str().unwrap(), "add", "linked-only.md"],
            None,
        );
        fs::remove_file(linked.join("linked-only.md")).unwrap();
        let expected = repo.run_git_successfully(
            &[
                "-C",
                linked.to_str().unwrap(),
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            None,
        );
        // When: discovery begins in the linked worktree rather than the common gitdir.
        let actual = build_selector(&linked)
            .unwrap()
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: the deleted-but-staged linked path is retained and the other index is untouched.
        assert!(
            expected
                .split(|byte| *byte == 0)
                .any(|path| path == b"linked-only.md")
        );
        assert!(actual.contains(&RepoPath::parse("linked-only.md").unwrap()));
        assert!(
            !build_selector(repo.get_root())
                .unwrap()
                .get_candidate_observation_by_paths(&[])
                .unwrap()
                .paths
                .contains(&RepoPath::parse("linked-only.md").unwrap())
        );
    }

    #[test]
    fn corpus_ignores_replacements_for_index_backed_ignore_files() {
        // Given: a sparse ignore file has a replacement that would expose a hidden document.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join(".gitignore"), b"hidden.md\n").unwrap();
        fs::write(repo.get_root().join("hidden.md"), b"hidden").unwrap();
        repo.run_git_successfully(&["add", ".gitignore"], None);
        let original = repo.get_oid(&["rev-parse", ":.gitignore"], None);
        let replacement = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b""));
        repo.run_git_successfully(&["replace", &original, &replacement], None);
        repo.run_git_successfully(&["update-index", "--skip-worktree", ".gitignore"], None);
        fs::remove_file(repo.get_root().join(".gitignore")).unwrap();
        // When: candidate enumeration falls back to the index's raw ignore blob.
        let actual = build_selector(repo.get_root())
            .unwrap()
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: replacement refs cannot change corpus membership.
        assert_eq!(
            actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            ["live.md"]
        );
    }

    #[test]
    fn sparse_index_expands_index_tree_membership_without_changing_user_state() {
        for format in crate::process_test_support::FORMATS {
            // Given: a collapsed staged tree differs from HEAD and restored files match ignores.
            let repo = TempRepo::new(format);
            let tree = repo.prepare_sparse_index();
            let expected = [
                "keep/a.md",
                "live.md",
                "omit/b.md",
                "omit/deleted.md",
                "omit/nested/c.markdown",
                "omit/staged.md",
            ];
            let oracle = repo.run_git_successfully(
                &[
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ],
                None,
            );
            let mut oracle: Vec<_> = oracle
                .split(|byte| *byte == 0)
                .filter(|path| path.ends_with(b".md") || path.ends_with(b".markdown"))
                .map(|path| std::str::from_utf8(path).unwrap())
                .collect();
            oracle.sort_unstable();
            assert_eq!(oracle, expected);
            let empty = repo.get_oid(&["mktree"], Some(b""));
            repo.run_git_successfully(&["replace", &tree, &empty], None);
            let before = repo.capture_fixture_state();
            // When: listing expands the actual index's tree, including deleted tracked paths.
            let actual = build_selector(repo.get_root())
                .unwrap()
                .get_candidate_observation_by_paths(&[])
                .unwrap()
                .paths;
            // Then: tracked paths survive ignores, untracked ignored paths do not.
            assert_eq!(
                actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(repo.capture_fixture_state(), before);
        }
    }

    #[test]
    fn sparse_index_rejects_missing_corrupt_trees_and_late_alternates_safely() {
        for format in crate::process_test_support::FORMATS {
            for failure in ["missing", "corrupt", "alternates"] {
                // Given: a valid collapsed index was opened before its object store changed.
                let repo = TempRepo::new(format);
                let tree = repo.prepare_sparse_index();
                let git = build_selector(repo.get_root()).unwrap();
                match failure {
                    "missing" => fs::remove_file(repo.build_object_path(&tree)).unwrap(),
                    "corrupt" => {
                        let empty = repo.get_oid(&["mktree"], Some(b""));
                        repo.replace_loose_object(&tree, &empty);
                    }
                    _ => fs::write(
                        repo.get_root().join(".git/objects/info/alternates"),
                        b"PRIVATE_DONOR_PATH\n",
                    )
                    .unwrap(),
                }
                let before = repo.capture_fixture_state();
                // When: listing needs the collapsed tree rather than trusting HEAD or prior policy.
                let error = git.get_candidate_observation_by_paths(&[]).unwrap_err();
                // Then: no incomplete successful listing or raw object/path payload escapes.
                assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
                assert_eq!(error.to_string(), "cannot list document candidates");
                assert_eq!(repo.capture_fixture_state(), before);
            }
        }
    }

    #[test]
    fn irrelevant_non_utf8_paths_are_filtered_before_decoding() {
        // Given: a raw irrelevant indexed name that the host need not create.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        stage_raw_path(&repo, b"irrelevant\xff.txt", "100644");
        let service = build_selector(repo.get_root()).unwrap();
        // When: the application selects documents from raw Git bytes.
        let candidates = service
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: only the ordinary fixture document remains.
        assert_eq!(candidates, [RepoPath::parse("live.md").unwrap()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_symlink_is_skipped_before_decoding_but_regular_markdown_fails() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        // Given: an actual non-UTF-8 Markdown symlink and its tracked name.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let name = OsStr::from_bytes(b"raw\xff.md");
        std::os::unix::fs::symlink("missing", repo.get_root().join(name)).unwrap();
        stage_raw_path(&repo, b"raw\xff.md", "120000");
        let service = build_selector(repo.get_root()).unwrap();
        // When: the final symlink is observed before UTF-8 decoding.
        let candidates = service
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: the invalid-byte symlink never becomes an unsupported document.
        assert_eq!(candidates, [RepoPath::parse("live.md").unwrap()]);
        // Given: the same tracked raw name is now an ordinary file.
        fs::remove_file(repo.get_root().join(name)).unwrap();
        fs::write(repo.get_root().join(name), b"ordinary").unwrap();
        // When: selection observes the changed type.
        let error = service.get_candidate_observation_by_paths(&[]).unwrap_err();
        // Then: relevant ordinary file encoding remains an invalid request.
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        assert_eq!(error.to_string(), "unsupported document path");
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .is::<std::str::Utf8Error>()
        );
    }

    #[test]
    fn candidate_parent_symlink_is_a_listing_failure() {
        // Given: a tracked path beneath a live symlink to an outside directory.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("note.md"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.get_root().join("linked")).unwrap();
        stage_raw_path(&repo, b"linked/note.md", "100644");
        let service = build_selector(repo.get_root()).unwrap();
        // When: candidate selection observes the path without following parents.
        let error = service.get_candidate_observation_by_paths(&[]).unwrap_err();
        // Then: unsafe traversal is fatal with the preserved public listing diagnostic.
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert_eq!(error.to_string(), "cannot list document candidates");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn unsafe_raw_components_skip_probes_and_keep_lexical_normalization() {
        use crate::service::MockGitWorktreeListing;
        // Given: relevant raw paths whose structural form is unsafe to probe.
        let mut git = MockGitWorktreeListing::new();
        git.expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
            .returning(|_, _| Ok(b"a/../note.md\0./note.md\0a//b.md\0".to_vec()));
        let service = DocumentRefreshService {
            git,
            filesystem: build_root_only_files(),
            clock: Box::new(crate::process::SystemClock::new()),
            markdown: (),
            cache: Rc::new(()),
            provenance: (),
        };
        // When: the service selects and normalizes paths without probing raw file paths.
        let candidates = service
            .get_candidate_observation_by_paths(&[])
            .unwrap()
            .paths;
        // Then: normalization and deduplication remain owned by RepoPath.
        assert_eq!(
            candidates,
            [
                RepoPath::parse("a/b.md").unwrap(),
                RepoPath::parse("note.md").unwrap()
            ]
        );
    }

    #[test]
    fn scoped_candidates_match_normalized_names_without_probing_raw_dot_paths() {
        use crate::service::MockGitWorktreeListing;
        // Given: a raw name escaping the selected directory and another dot path resolving into it.
        let mut git = MockGitWorktreeListing::new();
        git.expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
            .returning(|_, _| Ok(b"notes/../outside.md\0notes/./a.md\0notes/ab.md\0".to_vec()));
        let mut filesystem = build_root_only_files();
        filesystem
            .expect_symlink_is_exist()
            .withf(|path| path == Path::new("notes/ab.md"))
            .returning(|_| Ok(false));
        let service = DocumentRefreshService {
            git,
            filesystem,
            clock: Box::new(crate::process::SystemClock::new()),
            markdown: (),
            cache: Rc::new(()),
            provenance: (),
        };
        let exact = [RepoPath::parse("notes/a.md").unwrap()];
        let directory = [RepoPath::parse("notes").unwrap()];

        // When: scopes select by normalized owner identity.
        let selected = service
            .get_candidate_observation_by_paths(&exact)
            .unwrap()
            .paths;
        let descendants = service
            .get_candidate_observation_by_paths(&directory)
            .unwrap()
            .paths;

        // Then: an outside owner is excluded and the selected dot path is retained.
        assert_eq!(selected, exact);
        assert_eq!(
            descendants,
            [
                RepoPath::parse("notes/a.md").unwrap(),
                RepoPath::parse("notes/ab.md").unwrap()
            ]
        );
    }

    #[test]
    fn root_escape_still_fails_after_skipping_unsafe_raw_probe() {
        use crate::service::MockGitWorktreeListing;
        // Given: an escaping path and a filesystem mock that permits only the root observation.
        let mut git = MockGitWorktreeListing::new();
        git.expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
            .returning(|_, _| Ok(b"../note.md\0".to_vec()));
        let service = DocumentRefreshService {
            git,
            filesystem: build_root_only_files(),
            clock: Box::new(crate::process::SystemClock::new()),
            markdown: (),
            cache: Rc::new(()),
            provenance: (),
        };
        // When: lexical path validation sees the escaping candidate.
        let error = service.get_candidate_observation_by_paths(&[]).unwrap_err();
        // Then: it retains the old unsupported-document classification and cause.
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        assert_eq!(error.to_string(), "unsupported document path");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn replaced_root_is_rejected_even_when_all_raw_paths_are_irrelevant() {
        use crate::service::MockGitWorktreeListing;
        // Given: the filesystem root is replaced after its descriptor was pinned.
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("root");
        fs::create_dir(&root).unwrap();
        let filesystem = LocalFileSystem::new(&root).unwrap();
        fs::rename(&root, parent.path().join("detached")).unwrap();
        fs::create_dir(&root).unwrap();
        let mut git = MockGitWorktreeListing::new();
        git.expect_worktree_paths_get_by_ignore_mode_and_ignore_bytes()
            .returning(|_, _| Ok(b"irrelevant.txt\0".to_vec()));
        let service = DocumentRefreshService {
            git,
            filesystem,
            clock: Box::new(crate::process::SystemClock::new()),
            markdown: (),
            cache: Rc::new(()),
            provenance: (),
        };
        // When: selection has no eligible leaf to probe.
        let error = service.get_candidate_observation_by_paths(&[]).unwrap_err();
        // Then: the original root safety check still fails with the existing diagnostic.
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert_eq!(error.to_string(), "cannot list document candidates");
    }

    fn build_root_only_files() -> crate::service::MockFileReader {
        let mut files = crate::service::MockFileReader::new();
        files
            .expect_directory_get_by_path()
            .withf(|path| matches!(path.to_str(), Some("." | ".letopis")))
            .returning(|_| Ok(Some(std::fs::File::open(".").unwrap().into())));
        files
            .expect_file_get_bytes_by_path()
            .withf(|path| {
                matches!(
                    path.to_str(),
                    Some(".letopis/config.json" | ".letopis/ignore")
                )
            })
            .returning(|_| Ok(None));
        files
    }

    fn stage_raw_path(repo: &TempRepo, path: &[u8], mode: &str) {
        let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("{mode} {oid}\t").into_bytes();
        entry.extend_from_slice(path);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
    }

    fn prepare_exclude_fixture(repo: &TempRepo) {
        let commit = repo.get_oid(&["rev-parse", "HEAD"], None);
        repo.run_git_successfully(
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{commit},module.md"),
            ],
            None,
        );
        fs::create_dir(repo.get_root().join("module.md")).unwrap();
        fs::write(repo.get_root().join("module.md/hidden.md"), b"submodule").unwrap();
        for path in ["tracked-ignore.md", "staged-delete.md"] {
            fs::write(repo.get_root().join(path), b"tracked").unwrap();
        }
        repo.run_git_successfully(&["add", "tracked-ignore.md", "staged-delete.md"], None);
        repo.run_git_successfully(&["rm", "--cached", "staged-delete.md"], None);
        for directory in ["nested", "dir.md"] {
            fs::create_dir(repo.get_root().join(directory)).unwrap();
        }
        for path in [
            "nested/keep.md",
            "nested/drop.md",
            "dir.md/inside.md",
            "info-hidden.md",
            "global-hidden.md",
        ] {
            fs::write(repo.get_root().join(path), b"bytes").unwrap();
        }
        fs::write(repo.get_root().join(".gitignore"), b"tracked-ignore.md\n").unwrap();
        fs::write(
            repo.get_root().join("nested/.gitignore"),
            b"*.md\n!keep.md\n",
        )
        .unwrap();
        fs::write(
            repo.get_root().join(".git/info/exclude"),
            b"info-hidden.md\n",
        )
        .unwrap();
        let global = repo.get_root().join("global-excludes");
        fs::write(&global, b"global-hidden.md\n").unwrap();
        repo.run_git_successfully(
            &["config", "core.excludesFile", global.to_str().unwrap()],
            None,
        );
        std::os::unix::fs::symlink("missing-target", repo.get_root().join("symlink.md")).unwrap();
    }

    fn build_selector(
        root: &Path,
    ) -> Result<
        DocumentRefreshService<GitRepository, LocalFileSystem, (), (), ()>,
        crate::service::SnapshotError,
    > {
        Ok(DocumentRefreshService {
            git: GitRepository::open(root, &[crate::utils::SERVICE_DIRECTORY])?,
            filesystem: LocalFileSystem::new(root).unwrap(),
            clock: Box::new(crate::process::SystemClock::new()),
            markdown: (),
            cache: Rc::new(()),
            provenance: (),
        })
    }
}
