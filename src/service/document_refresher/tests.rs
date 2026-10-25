use super::{DocumentRefresher, RefreshDependencies};
use crate::{
    domain::{RepoPath, TreeOid},
    markdown::MarkdownParser,
    service::{
        BasisRetainer, CapturedFile, CorpusLister, Diagnostic, DocumentError, DocumentErrorKind,
        DocumentResult, FileStat, FileSystem, MarkdownCodec, ParsedFile, ProvenanceObservation,
        ProvenanceRequest, Refresher, SnapshotError, SourceProvenance, SourceRevisionComparison,
        SourceRevisionRequest, Storage, StorageRead,
    },
    storage::SqliteStorage,
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
struct Candidates;
impl CorpusLister for Candidates {
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        Ok(vec![RepoPath::parse("note.md").unwrap()])
    }
}
struct TestFiles {
    bytes: RefCell<Vec<u8>>,
    stat: Cell<FileStat>,
    now: Cell<i64>,
    reads: Cell<usize>,
    locks: Cell<usize>,
    replace_on_read: Cell<Option<usize>>,
    fail_on_read: Cell<Option<usize>>,
    missing: Cell<bool>,
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
            now: Cell::new(NOW),
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
impl FileSystem for TestFiles {
    type IndexLock = ();
    fn stat_file(&self, _: &RepoPath) -> DocumentResult<Option<FileStat>> {
        Ok((!self.missing.get()).then(|| self.stat.get()))
    }
    fn read_file(&self, _: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        let reads = self.reads.get() + 1;
        self.reads.set(reads);
        if self.fail_on_read.get() == Some(reads) {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                "cannot read document file",
            ));
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
    fn get_time_ns(&self) -> DocumentResult<i64> {
        Ok(self.now.get())
    }
    fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
        self.locks.set(self.locks.get() + 1);
        Ok(())
    }
}
struct CountingMarkdown(Cell<usize>);
impl MarkdownCodec for CountingMarkdown {
    fn parse_file(
        &self,
        path: &RepoPath,
        bytes: &[u8],
    ) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
        self.0.set(self.0.get() + 1);
        MarkdownParser::new().parse_file(path, bytes)
    }
}
struct TestProvenance;
impl SourceProvenance for TestProvenance {
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
impl BasisRetainer for TestProvenance {
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert!(snapshots.is_empty());
        Ok(())
    }
}
type TestService =
    DocumentRefresher<Candidates, TestFiles, CountingMarkdown, SqliteStorage, TestProvenance>;
#[test]
fn skip_body_reads_for_unchanged_non_racy_marked_unmarked_and_invalid_files() {
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
        service.refresh().unwrap();
        service.filesystem.reset_counts();
        let parses = service.markdown.0.get();
        let result = service.refresh().unwrap();
        assert!(!result.published);
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
    let (_dir, mut service) = build_service();
    service.filesystem.stat.update(|mut stat| {
        stat.mtime_ns = NOW;
        stat
    });
    service.refresh().unwrap();
    let before = service.storage.begin_read().unwrap().list_files().unwrap();
    service.filesystem.reset_counts();
    let unchanged = service.refresh().unwrap();
    assert!(!unchanged.published);
    assert_eq!(
        (
            service.filesystem.reads.get(),
            service.filesystem.locks.get()
        ),
        (1, 0)
    );
    assert_eq!(
        service.storage.begin_read().unwrap().list_files().unwrap(),
        before
    );
    // Exact same size, inode and mtime; only bytes differ.
    service.filesystem.bytes.replace(EDITED.to_vec());
    let changed = service.refresh().unwrap();
    assert_eq!(changed.revision, 2);
    assert_eq!(read_matches(&mut service, "BravoToken"), ["note.md"]);
    assert!(read_matches(&mut service, "AlphaToken").is_empty());
}

#[test]
fn treat_racy_boundary_clock_regression_and_checked_arithmetic_conservatively() {
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
        service.refresh().unwrap();
        service.filesystem.reset_counts();
        service.filesystem.now.set(now);
        let result = service.refresh().unwrap();
        assert!(!result.published);
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
    for bytes in [INITIAL, &b"<!-- letopis: invalid -->"[..]] {
        let (dir, mut service) = build_service();
        service.filesystem.bytes.replace(bytes.to_vec());
        service.filesystem.stat.update(|mut stat| {
            stat.size = u64::try_from(bytes.len()).unwrap();
            stat
        });
        service.refresh().unwrap();
        let before_nodes = service.storage.begin_read().unwrap().list_nodes().unwrap();
        let before_files = service.storage.begin_read().unwrap().list_files().unwrap();
        let before_fts = read_fts_blocks(dir.path());
        let parses = service.markdown.0.get();
        service.filesystem.stat.update(|mut stat| {
            stat.inode += 1;
            stat
        });
        service.filesystem.now.set(NOW + 1);
        let outcome = service.refresh().unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.files_updated, 1);
        assert_eq!(service.markdown.0.get(), parses);
        assert_eq!(read_fts_blocks(dir.path()), before_fts);
        assert_eq!(
            service.storage.begin_read().unwrap().list_nodes().unwrap(),
            before_nodes
        );
        let after_files = service.storage.begin_read().unwrap().list_files().unwrap();
        assert_eq!(after_files[0].inode, before_files[0].inode + 1);
        assert_eq!(after_files[0].valid, before_files[0].valid);
        assert_eq!(after_files[0].error, before_files[0].error);
        assert_eq!(after_files[0].hash, before_files[0].hash);
        assert_eq!(after_files[0].checked_at_ns, NOW + 1);
    }
}

#[test]
fn changed_capture_between_locked_detection_and_preparation_aborts() {
    let (_dir, mut service) = build_service();
    service.filesystem.replace_on_read.set(Some(3));
    let result = service.refresh();
    assert_eq!(service.filesystem.reads.get(), 3);
    assert!(result.is_err());
    let mut read = service.storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 0);
    assert!(read.list_files().unwrap().is_empty());
    assert!(read.list_nodes().unwrap().is_empty());
}

#[test]
fn fatal_read_is_not_an_invalid_markdown_publication() {
    let (_dir, mut service) = build_service();
    service.refresh().unwrap();
    let before = service.storage.begin_read().unwrap().list_files().unwrap();
    service.filesystem.reset_counts();
    service.filesystem.stat.update(|mut stat| {
        stat.inode += 1;
        stat
    });
    service.filesystem.fail_on_read.set(Some(3));
    assert!(service.refresh().is_err());
    let mut read = service.storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.list_files().unwrap(), before);
    assert_eq!(read.match_text("AlphaToken").unwrap().len(), 1);
}

#[test]
fn never_published_missing_candidate_is_not_a_delta_after_empty_initial_build() {
    let (_dir, mut service) = build_service();
    service.filesystem.missing.set(true);
    assert_eq!(service.refresh().unwrap().revision, 1);
    service.filesystem.reset_counts();
    let result = service.refresh().unwrap();
    assert!(!result.published);
    assert_eq!(
        (
            service.filesystem.reads.get(),
            service.filesystem.locks.get()
        ),
        (0, 0)
    );
    service.filesystem.missing.set(false);
    assert_eq!(service.refresh().unwrap().files_updated, 1);
}

#[test]
fn discard_preliminary_capture_and_publish_the_locked_observation() {
    let (_dir, mut service) = build_service();
    service.filesystem.replace_on_read.set(Some(2));
    let outcome = service.refresh().unwrap();
    assert_eq!(outcome.revision, 1);
    assert_eq!(read_matches(&mut service, "BravoToken"), ["note.md"]);
    assert!(read_matches(&mut service, "AlphaToken").is_empty());
}

// A same-text DELETE/INSERT changes FTS segment bytes even if search results agree.
fn read_fts_blocks(root: &Path) -> Vec<(i64, Vec<u8>)> {
    let connection = Connection::open(root.join("cache.db")).unwrap();
    connection
        .prepare("SELECT id, block FROM text_index_data ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn build_service() -> (TempDir, TestService) {
    let dir = tempfile::tempdir().unwrap();
    let storage =
        SqliteStorage::open(&dir.path().join("cache.db"), &dir.path().join("vectors.db")).unwrap();
    (
        dir,
        DocumentRefresher::new(RefreshDependencies {
            git: Candidates,
            filesystem: TestFiles::new(),
            markdown: CountingMarkdown(Cell::new(0)),
            storage,
            provenance: TestProvenance,
        }),
    )
}

fn read_matches(service: &mut TestService, text: &str) -> Vec<String> {
    service
        .storage
        .begin_read()
        .unwrap()
        .match_text(text)
        .unwrap()
        .into_iter()
        .map(|row| row.id.as_str().to_owned())
        .collect()
}

mod basis;
pub(super) mod process;
