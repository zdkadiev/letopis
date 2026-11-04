use super::utils::{CandidateSelection, map_document_file};
use super::{
    CommandError, CommandErrorKind, CommandResult, DocumentError, DocumentErrorKind,
    DocumentResult, ElementAddress, ElementLocation, FileReader, GitSnapshots, GitWorktreePaths,
    InspectedElement, Inspection, Inspector, MarkdownCodec, ParsedFile, Refresher, SnapshotEntry,
    SnapshotError, SnapshotErrorKind,
};
use crate::domain::{BlobOid, EdgeTarget, RepoPath, Sha256, SourceUri, TreeOid};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) struct InspectionService<D, G, F, M> {
    refresher: D,
    git: G,
    filesystem: F,
    markdown: M,
}
#[derive(Default)]
struct CapturedKnowledge {
    entries: BTreeMap<RepoPath, BlobOid>,
    elements: BTreeMap<String, InspectedElement>,
    sources: BTreeSet<SourceUri>,
    observations: BTreeMap<RepoPath, Option<Sha256>>,
}
impl<D, G, F, M> InspectionService<D, G, F, M> {
    pub(crate) fn new(refresher: D, git: G, filesystem: F, markdown: M) -> Self {
        Self {
            refresher,
            git,
            filesystem,
            markdown,
        }
    }
}
impl<D: Refresher, G: GitWorktreePaths + GitSnapshots, F: FileReader, M: MarkdownCodec> Inspector
    for InspectionService<D, G, F, M>
{
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection> {
        self.refresher.refresh(&[]).map_err(map_document_error)?;
        let requested = ids
            .iter()
            .map(ElementAddress::as_str)
            .collect::<BTreeSet<_>>();
        let mut captured = CapturedKnowledge::default();
        for path in self.get_candidates().map_err(map_document_error)? {
            let file = self.capture_file(path, &requested)?;
            captured.entries.extend(file.entries);
            captured.elements.extend(file.elements);
            captured.sources.extend(file.sources);
            captured.observations.extend(file.observations);
        }
        captured.validate_membership(&requested)?;
        if !captured.sources.is_empty() {
            captured = self.capture_sources(captured, &requested)?;
        }
        let tree = self.assemble_tree(&captured)?;
        let retained = self
            .git
            .tree_pin_by_oid(&tree)
            .map_err(map_snapshot_error)?;
        Ok(Inspection {
            snapshot: retained.get_oid().clone(),
            elements: captured.elements,
        })
    }
}
impl<D, G: GitWorktreePaths, F: FileReader, M> InspectionService<D, G, F, M> {
    fn get_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        CandidateSelection {
            git: &self.git,
            filesystem: &self.filesystem,
        }
        .get_candidates(&[])
    }
}

impl<D, G: GitSnapshots, F: FileReader, M: MarkdownCodec> InspectionService<D, G, F, M> {
    // Each call drops the file's bytes and parsed search text before the next read.
    fn capture_file(
        &self,
        path: RepoPath,
        requested: &BTreeSet<&str>,
    ) -> CommandResult<CapturedKnowledge> {
        let mut captured = CapturedKnowledge::default();
        let Some(file) =
            map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))
                .map_err(map_document_error)?
        else {
            captured.observations.insert(path, None);
            return Ok(captured);
        };
        let parsed = self.markdown.file_parse(&path, &file.bytes).map_err(|_| {
            CommandError::new(
                CommandErrorKind::ValidationFailed,
                "current knowledge is invalid",
            )
        })?;
        let Some(parsed) = parsed else {
            captured
                .observations
                .insert(path, Some(Sha256::digest(&file.bytes)));
            return Ok(captured);
        };
        let oid = self
            .git
            .blob_write(&file.bytes)
            .map_err(map_snapshot_error)?;
        captured.collect_metadata(&path, parsed, requested);
        captured.entries.insert(path, oid);
        Ok(captured)
    }

    fn capture_sources(
        &self,
        mut captured: CapturedKnowledge,
        requested: &BTreeSet<&str>,
    ) -> CommandResult<CapturedKnowledge> {
        let corpus = self.assemble_tree(&captured)?;
        captured.collect_remote_metadata(requested);
        for (path, sources) in captured.group_local_sources() {
            let bytes = self.capture_source_bytes(&corpus, &captured, &path)?;
            if let Some(bytes) = &bytes
                && !captured.entries.contains_key(&path)
            {
                let oid = self.git.blob_write(bytes).map_err(map_snapshot_error)?;
                captured.entries.insert(path.clone(), oid);
            }
            captured.collect_source_metadata(&sources, bytes.as_deref(), requested);
        }
        Ok(captured)
    }

    fn assemble_tree(&self, captured: &CapturedKnowledge) -> CommandResult<TreeOid> {
        let entries = captured
            .entries
            .iter()
            .map(|(path, oid)| SnapshotEntry {
                path: path.clone(),
                oid: oid.clone(),
            })
            .collect::<Vec<_>>();
        self.git.tree_assemble(&entries).map_err(map_snapshot_error)
    }

    fn capture_source_bytes(
        &self,
        corpus: &TreeOid,
        captured: &CapturedKnowledge,
        path: &RepoPath,
    ) -> CommandResult<Option<Vec<u8>>> {
        // A marked source may change live after capture: reuse its immutable blob.
        if captured.entries.contains_key(path) {
            return self
                .git
                .blob_get_by_tree_oid_and_path(corpus, path)
                .map(Some)
                .map_err(map_snapshot_error);
        }
        let bytes = map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))
            .map_err(map_document_error)?
            .map(|file| file.bytes);
        if let Some(observed) = captured.observations.get(path)
            && *observed != bytes.as_deref().map(Sha256::digest)
        {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "cannot capture current elements",
            ));
        }
        Ok(bytes)
    }
}
impl CapturedKnowledge {
    fn validate_membership(&self, requested: &BTreeSet<&str>) -> CommandResult<()> {
        let sources = self
            .sources
            .iter()
            .map(SourceUri::as_str)
            .collect::<BTreeSet<_>>();
        for id in requested {
            if !self.elements.contains_key(*id) && !sources.contains(id) {
                return Err(CommandError::new(
                    CommandErrorKind::NotFound,
                    "requested element is unavailable",
                ));
            }
        }
        Ok(())
    }

    fn collect_metadata(
        &mut self,
        path: &RepoPath,
        parsed: ParsedFile,
        requested: &BTreeSet<&str>,
    ) {
        for parsed_element in parsed.elements {
            let element = parsed_element.element;
            if requested.contains(element.id.as_str()) {
                self.elements.insert(
                    element.id.as_str().to_owned(),
                    InspectedElement {
                        path: ElementLocation::Local(path.clone()),
                        lines: Some(element.lines),
                        hash: Some(element.hash),
                    },
                );
            }
        }
        for edge in parsed.edges {
            if let EdgeTarget::Source(source) = edge.get_target() {
                self.sources.insert(source.clone());
            }
        }
    }

    fn collect_remote_metadata(&mut self, requested: &BTreeSet<&str>) {
        for uri in &self.sources {
            if uri.get_local().is_none() && requested.contains(uri.as_str()) {
                self.elements.insert(
                    uri.as_str().to_owned(),
                    InspectedElement {
                        path: ElementLocation::Remote(uri.clone()),
                        lines: None,
                        hash: None,
                    },
                );
            }
        }
    }

    fn group_local_sources(&self) -> BTreeMap<RepoPath, Vec<SourceUri>> {
        let mut grouped = BTreeMap::<RepoPath, Vec<SourceUri>>::new();
        for source in &self.sources {
            if let Some(local) = source.get_local() {
                grouped
                    .entry(local.get_path().clone())
                    .or_default()
                    .push(source.clone());
            }
        }
        grouped
    }

    fn collect_source_metadata(
        &mut self,
        sources: &[SourceUri],
        bytes: Option<&[u8]>,
        requested: &BTreeSet<&str>,
    ) {
        for source in sources {
            if !requested.contains(source.as_str()) {
                continue;
            }
            let Some(local) = source.get_local() else {
                continue;
            };
            // A whole-file knowledge identity wins over an identical source selector.
            self.elements
                .entry(source.as_str().to_owned())
                .or_insert_with(|| InspectedElement {
                    path: ElementLocation::Local(local.get_path().clone()),
                    lines: local.get_lines(),
                    hash: bytes
                        .and_then(|bytes| local.select_bytes(bytes))
                        .map(Sha256::digest),
                });
        }
    }
}
fn map_document_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot capture current elements", cause)
}
fn map_snapshot_error(cause: SnapshotError) -> CommandError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        SnapshotErrorKind::NotFound => CommandErrorKind::NotFound,
        SnapshotErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot retain current elements", cause)
}

#[cfg(test)]
mod tests {
    use crate::service::{
        FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult,
    };

    use super::InspectionService;
    use crate::{
        domain::{BlobOid, RepoPath, Sha256, TreeOid},
        markdown::MarkdownParser,
        service::{
            CapturedFile, CommandErrorKind, DocumentResult, ElementAddress, FileStat, GitSnapshots,
            GitWorktreePaths, Inspector, RefreshOutcome, Refresher, RetainedTree, SnapshotEntry,
            SnapshotError, SnapshotErrorKind, SnapshotFile,
        },
    };
    use std::{cell::RefCell, rc::Rc};

    const MARKED_SOURCE: &[u8] =
        b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](a.md#L2-L2 \"letopis:type=external;path=a.md#L2-L2\")\n";
    const MARKED_WITH_SOURCE: &[u8] =
        b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](source.txt \"letopis:type=external;path=source.txt\")\n";
    const UNMARKED_SOURCE: &[u8] = b"PRIVATE_SOURCE_BYTES\n";
    const MARKED_REFERER: &[u8] =
        b"<!-- letopis: {\"created_at\":1} -->\nReferer\n[source](a.md \"letopis:type=external;path=a.md\")\n";
    const CHANGED_MARKED_SOURCE: &[u8] =
        b"<!-- letopis: {\"created_at\":1} -->\nKnowledge\n[more](second.txt \"letopis:type=external;path=second.txt\")\n";
    type Events = Rc<RefCell<Vec<String>>>;
    struct RecordingDocuments(Events);
    struct RecordingGit(Events, Scenario);
    struct RecordingFiles(Events, Scenario);
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Scenario {
        Plain,
        Overlap,
        PinFailure,
        SourceIoFailure,
        UnmarkedToMarked,
        UnmarkedToInvalid,
        UnmarkedUnchanged,
        AbsentToMarked,
        AbsentUnchanged,
    }
    #[test]
    fn inspection_filters_raw_symlinks_before_decoding_and_normalizes_safe_results() {
        use crate::service::{MockFileReader, MockGitWorktreePaths};
        // Given: raw irrelevant/private names, a symlink and a normalized duplicate.
        let mut git = MockGitWorktreePaths::new();
        git.expect_worktree_paths_get().returning(|| {
            Ok(b"bad\xff.txt\0.letopis/bad\xff.md\0link\xff.md\0./note.md\0note.md\0".to_vec())
        });
        let mut files = MockFileReader::new();
        files
            .expect_directory_get_by_path()
            .returning(|_| Ok(Some(std::fs::File::open(".").unwrap().into())));
        files.expect_symlink_is_exist().returning(|path| {
            use std::os::unix::ffi::OsStrExt;
            match path.as_os_str().as_bytes() {
                b"link\xff.md" => Ok(true),
                b"note.md" => Ok(false),
                other => panic!("unexpected raw probe: {other:?}"),
            }
        });
        let service = InspectionService::new((), git, files, ());
        // When: inspection selects candidates before capture.
        let paths = service.get_candidates().unwrap();
        // Then: invalid-byte symlinks and irrelevant names are excluded before decoding.
        assert_eq!(paths, [RepoPath::parse("note.md").unwrap()]);
    }

    #[test]
    fn inspection_preserves_listing_error_context_and_cause() {
        use crate::service::{GitError, MockFileReader, MockGitWorktreePaths};
        // Given: an independent Git enumeration failure and no permitted filesystem calls.
        let mut git = MockGitWorktreePaths::new();
        git.expect_worktree_paths_get()
            .returning(|| Err(GitError::new(std::io::Error::other("private cause"))));
        let service = InspectionService::new((), git, MockFileReader::new(), ());
        // When: the selection error takes inspection's document-error mapping.
        let error = super::map_document_error(service.get_candidates().unwrap_err());
        // Then: the public capture diagnostic and internal cause remain distinct.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.to_string(), "cannot capture current elements");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn inspect_rejects_unmarked_candidate_changed_to_marked_source() {
        // Given / When / Then: the helper prepares, exercises, and checks this scenario.
        assert_changed_candidate_is_rejected(Scenario::UnmarkedToMarked);
    }

    #[test]
    fn inspect_rejects_unmarked_candidate_changed_to_invalid_source() {
        // Given / When / Then: the helper prepares, exercises, and checks this scenario.
        assert_changed_candidate_is_rejected(Scenario::UnmarkedToInvalid);
    }

    #[test]
    fn inspect_rejects_absent_candidate_changed_to_marked_source() {
        // Given / When / Then: the helper prepares, exercises, and checks this scenario.
        assert_changed_candidate_is_rejected(Scenario::AbsentToMarked);
    }

    #[test]
    fn inspect_accepts_unchanged_unmarked_candidate_as_source() {
        // Given
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::UnmarkedUnchanged),
            RecordingFiles(events.clone(), Scenario::UnmarkedUnchanged),
            MarkdownParser::new(),
        );
        // When
        let result = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap();
        // Then
        assert_eq!(result.elements.len(), 1);
        assert_eq!(
            result.elements["a.md"].hash,
            Some(
                Sha256::parse("fd800c29fbc6251cd9c3ba805b5113832e21ae5e0e08853ddcbc53ea64339741")
                    .unwrap()
            )
        );
        assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
    }

    #[test]
    fn inspect_keeps_absent_candidate_source_without_a_hash() {
        // Given
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::AbsentUnchanged),
            RecordingFiles(events.clone(), Scenario::AbsentUnchanged),
            MarkdownParser::new(),
        );
        // When
        let result = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap();
        // Then
        assert_eq!(result.elements.len(), 1);
        assert_eq!(result.elements["a.md"].hash, None);
        assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
    }
    #[test]
    fn capture_writes_each_blob_before_reading_the_next_file() {
        // Given: sequencing at the I/O boundaries proves bounded capture.
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::Plain),
            RecordingFiles(events.clone(), Scenario::Plain),
            MarkdownParser::new(),
        );
        // When
        let inspection = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap();
        // Then: unrequested marked files must also belong to the tree.
        assert_eq!(inspection.elements.len(), 1);
        assert_eq!(
            *events.borrow(),
            [
                "refresh",
                "list",
                "read a.md",
                "write a",
                "read b.md",
                "write b",
                "assemble",
                "pin"
            ]
        );
    }

    #[test]
    fn inspect_reuses_captured_marked_source_after_live_edit() {
        // Given: the filesystem would return different bytes on a second read.
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::Overlap),
            RecordingFiles(events.clone(), Scenario::Overlap),
            MarkdownParser::new(),
        );
        // When
        let result = service
            .inspect(&[ElementAddress::parse("a.md#L2-L2").unwrap()])
            .unwrap();
        // Then: one captured blob serves both knowledge and source identities.
        assert_eq!(result.elements.len(), 1);
        assert_eq!(
            result.elements["a.md#L2-L2"].hash,
            Some(
                Sha256::parse("421dc617d921c24f41441973d8476605718a14a5c2228b8344cc1d6d816e8d39")
                    .unwrap()
            )
        );
        assert_eq!(
            *events.borrow(),
            [
                "refresh",
                "list",
                "read a.md",
                "write a",
                "assemble",
                "read blob a.md",
                "assemble",
                "pin"
            ]
        );
    }

    #[test]
    fn inspect_fails_when_final_pin_fails() {
        // Given
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::PinFailure),
            RecordingFiles(events.clone(), Scenario::PinFailure),
            MarkdownParser::new(),
        );
        // When
        let error = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
    }

    #[test]
    fn inspect_fails_safely_when_declared_source_read_has_io_error() {
        // Given: corpus capture succeeds, then the source read fails with a private cause.
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), Scenario::SourceIoFailure),
            RecordingFiles(events.clone(), Scenario::SourceIoFailure),
            MarkdownParser::new(),
        );
        // When
        let error = service
            .inspect(&[
                ElementAddress::parse("a.md").unwrap(),
                ElementAddress::parse("source.txt").unwrap(),
            ])
            .unwrap_err();
        // Then: failure occurs before final assembly/pin, with no payload in diagnostics.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "cannot capture current elements");
        assert!(!error.to_string().contains("PRIVATE_SOURCE_BYTES"));
        assert_eq!(
            *events.borrow(),
            [
                "refresh",
                "list",
                "read a.md",
                "write a",
                "assemble",
                "read source.txt"
            ]
        );
    }

    fn assert_changed_candidate_is_rejected(scenario: Scenario) {
        // Given
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone(), scenario),
            RecordingFiles(events.clone(), scenario),
            MarkdownParser::new(),
        );
        // When
        let error = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap_err();
        // Then: a changed candidate must never produce a partial retained tree.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "cannot capture current elements");
        assert!(!error.to_string().contains("PRIVATE_SOURCE_BYTES"));
        assert_eq!(
            events.borrow().last().map(String::as_str),
            Some("read a.md")
        );
    }

    impl Refresher for RecordingDocuments {
        fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            panic!("routine reads must not request a full audit")
        }
        fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            panic!("this scenario must not trigger a full reindex")
        }

        fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            self.0.borrow_mut().push("refresh".into());
            Ok(RefreshOutcome {
                revision: 1,
                published: false,
                files_updated: 0,
                files_removed: 0,
                invalid_files_updated: 0,
            })
        }
    }
    impl GitWorktreePaths for RecordingGit {
        fn worktree_paths_get(&self) -> crate::service::GitResult<Vec<u8>> {
            self.0.borrow_mut().push("list".into());
            let paths = if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure) {
                &['a'][..]
            } else {
                &[
                    'a',
                    if self.1.uses_candidate_source() {
                        'z'
                    } else {
                        'b'
                    },
                ]
            };
            Ok(paths
                .iter()
                .map(|letter| format!("{letter}.md"))
                .flat_map(|path| path.bytes().chain(std::iter::once(0)).collect::<Vec<_>>())
                .collect())
        }
    }
    impl GitSnapshots for RecordingGit {
        fn tree_retain_by_oid(&self, _: &TreeOid) -> Result<(), SnapshotError> {
            panic!("inspection must not retain author roots")
        }
        fn tree_create(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
            panic!("capture must not retain bytes for tree_create")
        }
        fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
            let name = if self.1.uses_candidate_source() {
                if bytes == MARKED_REFERER { "d" } else { "a" }
            } else if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure)
                || bytes.ends_with(b"a")
            {
                "a"
            } else {
                "b"
            };
            self.0.borrow_mut().push(format!("write {name}"));
            Ok(BlobOid::parse(&name.repeat(40)).unwrap())
        }
        fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
            if self.1.uses_candidate_source() {
                assert_eq!(
                    entries.last().map(|entry| entry.path.as_str()),
                    Some("z.md")
                );
                self.0.borrow_mut().push("assemble".into());
                return Ok(TreeOid::parse(&"c".repeat(40)).unwrap());
            }
            if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure) {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].path.as_str(), "a.md");
                self.0.borrow_mut().push("assemble".into());
                return Ok(TreeOid::parse(&"c".repeat(40)).unwrap());
            }
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.path.as_str())
                    .collect::<Vec<_>>(),
                ["a.md", "b.md"]
            );
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.oid.as_str())
                    .collect::<Vec<_>>(),
                ["a".repeat(40), "b".repeat(40)]
            );
            self.0.borrow_mut().push("assemble".into());
            Ok(TreeOid::parse(&"c".repeat(40)).unwrap())
        }
        fn blob_get_by_tree_oid_and_path(
            &self,
            _: &TreeOid,
            path: &RepoPath,
        ) -> Result<Vec<u8>, SnapshotError> {
            assert_eq!(self.1, Scenario::Overlap);
            self.0
                .borrow_mut()
                .push(format!("read blob {}", path.as_str()));
            Ok(MARKED_SOURCE.to_vec())
        }
        fn tree_pin_by_oid(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
            assert_eq!(tree.as_str(), "c".repeat(40));
            self.0.borrow_mut().push("pin".into());
            if self.1 == Scenario::PinFailure {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "pin failed",
                ));
            }
            Ok(RetainedTree::new(tree.clone()))
        }
    }
    impl FileReader for RecordingFiles {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }
        fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
            panic!("capture uses verified reads")
        }
        fn file_get_bytes_by_path(&self, _: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
            panic!("unexpected file_get_bytes_by_path")
        }
        fn file_get_by_path(&self, path: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            if self.1 == Scenario::SourceIoFailure && path.to_str().unwrap() == "source.txt" {
                self.0.borrow_mut().push("read source.txt".into());
                return Err(FileError::new_with_cause(
                    FileErrorKind::Io,
                    std::io::Error::other("PRIVATE_SOURCE_BYTES"),
                ));
            }
            let read_again = path.to_str().unwrap() == "a.md"
                && self.0.borrow().iter().any(|event| event == "read a.md");
            self.0
                .borrow_mut()
                .push(format!("read {}", path.to_str().unwrap()));
            if matches!(self.1, Scenario::AbsentToMarked | Scenario::AbsentUnchanged)
                && (!read_again || self.1 == Scenario::AbsentUnchanged)
                && path.to_str().unwrap() == "a.md"
            {
                return Ok(None);
            }
            let bytes = if read_again {
                match self.1 {
                    Scenario::UnmarkedToMarked | Scenario::AbsentToMarked => {
                        CHANGED_MARKED_SOURCE.to_vec()
                    }
                    Scenario::UnmarkedToInvalid => {
                        b"<!-- letopis: {broken} -->\nPRIVATE_SOURCE_BYTES\n".to_vec()
                    }
                    Scenario::UnmarkedUnchanged => UNMARKED_SOURCE.to_vec(),
                    _ => b"live edited after capture\n".to_vec(),
                }
            } else if self.1.uses_candidate_source() && path.to_str().unwrap() == "a.md" {
                UNMARKED_SOURCE.to_vec()
            } else if self.1.uses_candidate_source() {
                MARKED_REFERER.to_vec()
            } else if self.1 == Scenario::Overlap {
                MARKED_SOURCE.to_vec()
            } else if self.1 == Scenario::SourceIoFailure {
                MARKED_WITH_SOURCE.to_vec()
            } else {
                format!(
                    "<!-- letopis: {{\"created_at\":1}} -->\n{}",
                    &path.to_str().unwrap()[..1]
                )
                .into_bytes()
            };
            Ok(Some(CapturedFile {
                stat: FileStat {
                    size: bytes.len() as u64,
                    mtime_ns: 1,
                    inode: 1,
                    device: 1,
                },
                checked_at_ns: 1,
                bytes,
            }))
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
    impl FileLocker for RecordingFiles {
        type Lock = ();
        fn lock_acquire_by_path(&self, _path: &std::path::Path) -> FileResult<()> {
            panic!("capture must not own publication lock")
        }
        fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
            panic!("unexpected lock probe")
        }
    }

    impl Scenario {
        fn uses_candidate_source(self) -> bool {
            matches!(
                self,
                Self::UnmarkedToMarked
                    | Self::UnmarkedToInvalid
                    | Self::UnmarkedUnchanged
                    | Self::AbsentToMarked
                    | Self::AbsentUnchanged
            )
        }
    }
}
