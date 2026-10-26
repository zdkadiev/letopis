use super::{
    CommandError, CommandErrorKind, CommandResult, CorpusLister, DocumentError, DocumentErrorKind,
    ElementAddress, ElementLocation, FileSystem, GitSnapshots, InspectedElement, Inspection,
    Inspector, MarkdownCodec, ParsedFile, Refresher, SnapshotEntry, SnapshotError,
    SnapshotErrorKind,
};
use crate::domain::{BlobOid, EdgeTarget, RepoPath, Sha256, SourceUri, TreeOid};
use std::collections::{BTreeMap, BTreeSet};

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
impl<D: Refresher, G: CorpusLister + GitSnapshots, F: FileSystem, M: MarkdownCodec> Inspector
    for InspectionService<D, G, F, M>
{
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection> {
        self.refresher.refresh().map_err(map_document_error)?;
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
        if !captured.sources.is_empty() {
            captured = self.capture_sources(captured, &requested)?;
        }
        let tree = self.assemble_tree(&captured)?;
        let retained = self.git.pin(&tree).map_err(map_snapshot_error)?;
        Ok(Inspection {
            snapshot: retained.get_oid().clone(),
            elements: captured.elements,
        })
    }
}
impl<D, G: GitSnapshots, F: FileSystem, M: MarkdownCodec> InspectionService<D, G, F, M> {
    // Each call drops the file's bytes and parsed search text before the next read.
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
        captured.entries.insert(path, oid);
        Ok(Some(captured))
    }

    fn capture_sources(
        &self,
        mut captured: CapturedKnowledge,
        requested: &BTreeSet<&str>,
    ) -> CommandResult<CapturedKnowledge> {
        let corpus = self.assemble_tree(&captured)?;
        captured.collect_remote_metadata(requested);
        for (path, sources) in captured.group_local_sources() {
            let bytes = self.capture_source_bytes(&corpus, &captured.entries, &path)?;
            if let Some(bytes) = &bytes
                && !captured.entries.contains_key(&path)
            {
                let oid = self.git.write_blob(bytes).map_err(map_snapshot_error)?;
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
        self.git
            .create_tree_from_blobs(&entries)
            .map_err(map_snapshot_error)
    }

    fn capture_source_bytes(
        &self,
        corpus: &TreeOid,
        entries: &BTreeMap<RepoPath, BlobOid>,
        path: &RepoPath,
    ) -> CommandResult<Option<Vec<u8>>> {
        // A marked source may change live after capture: reuse its immutable blob.
        if entries.contains_key(path) {
            return self
                .git
                .read_blob(corpus, path)
                .map(Some)
                .map_err(map_snapshot_error);
        }
        Ok(self
            .filesystem
            .read_file(path)
            .map_err(map_document_error)?
            .map(|file| file.bytes))
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
mod tests;
