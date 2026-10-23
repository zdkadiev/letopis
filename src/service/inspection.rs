use super::{
    CommandError, CommandErrorKind, CommandResult, CorpusFiles, DocumentError, DocumentErrorKind,
    Documents, ElementAddress, FileSystem, GitSnapshots, InspectedElement, Inspection, Inspections,
    Markdown, ParsedFile, SnapshotEntry, SnapshotError, SnapshotErrorKind,
};
use crate::domain::{EdgeTarget, RepoPath};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct InspectionService<D, G, F, M> {
    documents: D,
    git: G,
    filesystem: F,
    markdown: M,
}
#[derive(Default)]
struct CapturedKnowledge {
    entries: Vec<SnapshotEntry>,
    elements: BTreeMap<String, InspectedElement>,
    sources: BTreeSet<String>,
}
impl<D, G, F, M> InspectionService<D, G, F, M> {
    pub(crate) fn new(documents: D, git: G, filesystem: F, markdown: M) -> Self {
        Self {
            documents,
            git,
            filesystem,
            markdown,
        }
    }
}
impl<D: Documents, G: CorpusFiles + GitSnapshots, F: FileSystem, M: Markdown> Inspections
    for InspectionService<D, G, F, M>
{
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection> {
        self.documents.refresh().map_err(map_document_error)?;
        let requested = ids
            .iter()
            .map(ElementAddress::as_str)
            .collect::<BTreeSet<_>>();
        let mut captured = CapturedKnowledge::default();
        for path in self.git.list_candidates().map_err(map_document_error)? {
            if let Some(file) = self.capture_file(path, &requested)? {
                captured.entries.extend(file.entries);
                captured.elements.extend(file.elements);
                captured.sources.extend(file.sources);
            }
        }
        captured.validate_membership(&requested)?;
        let tree = self
            .git
            .create_tree_from_blobs(&captured.entries)
            .map_err(map_snapshot_error)?;
        let retained = self.git.pin(&tree).map_err(map_snapshot_error)?;
        Ok(Inspection {
            snapshot: retained.get_oid().clone(),
            elements: captured.elements,
        })
    }
}
impl<D, G: GitSnapshots, F: FileSystem, M: Markdown> InspectionService<D, G, F, M> {
    // Each call owns and drops one file's bytes and parsed search text before the next read.
    fn capture_file(
        &self,
        path: RepoPath,
        requested: &BTreeSet<&str>,
    ) -> CommandResult<Option<CapturedKnowledge>> {
        let Some(file) = self
            .filesystem
            .read_file(&path)
            .map_err(map_document_error)?
        else {
            return Ok(None);
        };
        let parsed = self.markdown.parse_file(&path, &file.bytes).map_err(|_| {
            CommandError::new(
                CommandErrorKind::ValidationFailed,
                "current knowledge is invalid",
            )
        })?;
        let Some(parsed) = parsed else {
            return Ok(None);
        };
        let oid = self
            .git
            .write_blob(&file.bytes)
            .map_err(map_snapshot_error)?;
        let mut captured = CapturedKnowledge::default();
        captured.collect_metadata(&path, parsed, requested);
        captured.entries.push(SnapshotEntry { path, oid });
        Ok(Some(captured))
    }
}
impl CapturedKnowledge {
    fn validate_membership(&self, requested: &BTreeSet<&str>) -> CommandResult<()> {
        for id in requested {
            if self.elements.contains_key(*id) {
                continue;
            }
            let kind = if self.sources.contains(*id) {
                CommandErrorKind::InternalError
            } else {
                CommandErrorKind::NotFound
            };
            return Err(CommandError::new(kind, "requested element is unavailable"));
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
                        path: path.clone(),
                        lines: Some(element.lines),
                        hash: Some(element.hash),
                    },
                );
            }
        }
        for edge in parsed.edges {
            if let EdgeTarget::Source(source) = edge.get_target() {
                self.sources.insert(source.as_str().to_owned());
            }
        }
    }
}
fn map_document_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot capture current knowledge", cause)
}
fn map_snapshot_error(cause: SnapshotError) -> CommandError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        SnapshotErrorKind::NotFound => CommandErrorKind::NotFound,
        SnapshotErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot retain current knowledge", cause)
}

#[cfg(test)]
mod tests {
    use super::InspectionService;
    use crate::{
        domain::{BlobOid, RepoPath, TreeOid},
        markdown::MarkdownParser,
        service::{
            CapturedFile, CorpusFiles, DocumentResult, Documents, ElementAddress, FileStat,
            FileSystem, GitSnapshots, Inspections, RefreshOutcome, RetainedTree, SnapshotEntry,
            SnapshotError, SnapshotFile,
        },
    };
    use std::{cell::RefCell, rc::Rc};

    type Events = Rc<RefCell<Vec<String>>>;
    struct RecordingDocuments(Events);
    struct RecordingGit(Events);
    struct RecordingFiles(Events);

    #[test]
    fn capture_writes_each_blob_before_reading_the_next_file() {
        // Arrange: sequencing at the I/O boundaries proves bounded capture.
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut service = InspectionService::new(
            RecordingDocuments(events.clone()),
            RecordingGit(events.clone()),
            RecordingFiles(events.clone()),
            MarkdownParser::new(),
        );
        // Act
        let inspection = service
            .inspect(&[ElementAddress::parse("a.md").unwrap()])
            .unwrap();
        // Assert: unrequested marked files must also belong to the tree.
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

    impl Documents for RecordingDocuments {
        fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
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
    impl CorpusFiles for RecordingGit {
        fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
            self.0.borrow_mut().push("list".into());
            Ok(["a.md", "b.md"]
                .map(|path| RepoPath::parse(path).unwrap())
                .to_vec())
        }
    }
    impl GitSnapshots for RecordingGit {
        fn create_tree(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
            panic!("capture must not retain bytes for create_tree")
        }
        fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
            let name = if bytes.ends_with(b"a") { "a" } else { "b" };
            self.0.borrow_mut().push(format!("write {name}"));
            Ok(BlobOid::parse(&name.repeat(40)).unwrap())
        }
        fn create_tree_from_blobs(
            &self,
            entries: &[SnapshotEntry],
        ) -> Result<TreeOid, SnapshotError> {
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
        fn read_blob(&self, _: &TreeOid, _: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
            panic!("capture must not reread blobs")
        }
        fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
            assert_eq!(tree.as_str(), "c".repeat(40));
            self.0.borrow_mut().push("pin".into());
            Ok(RetainedTree::new(tree.clone()))
        }
    }
    impl FileSystem for RecordingFiles {
        type IndexLock = ();
        fn stat_file(&self, _: &RepoPath) -> DocumentResult<Option<FileStat>> {
            panic!("capture uses verified reads")
        }
        fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
            self.0.borrow_mut().push(format!("read {}", path.as_str()));
            let bytes = format!(
                "<!-- letopis: {{\"created_at\":1}} -->\n{}",
                &path.as_str()[..1]
            )
            .into_bytes();
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
        fn get_time_ns(&self) -> DocumentResult<i64> {
            panic!("capture must not own publication timing")
        }
        fn lock_index(&self) -> DocumentResult<()> {
            panic!("capture must not own publication lock")
        }
    }
}
