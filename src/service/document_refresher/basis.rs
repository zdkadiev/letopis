use super::{Observation, PublicationMode, SnapshotVerification, fail_changed};
use crate::{
    domain::{EdgeTarget, RepoPath, Sha256, SourceUri, TreeOid},
    service::{
        BasisRetainer, DocumentResult, FileRow, FileSystem, MarkdownCodec, ProvenanceRequest,
        SourceProvenance, projection::EffectiveGraph,
    },
};
use std::collections::{BTreeMap, BTreeSet};

/// Metadata captured before the SQL write, never document or search text.
#[derive(Debug)]
pub(super) struct BasisPreflight {
    snapshots: BTreeSet<TreeOid>,
    bases: Vec<PreparedBasis>,
}
#[derive(Debug)]
struct PreparedBasis {
    source_snapshot: Option<TreeOid>,
    sources: BTreeSet<SourceUri>,
}
/// Borrowed inputs for metadata-only preflight against one pinned cache revision.
#[derive(Clone, Copy)]
pub(super) struct BasisPreparation<'a> {
    pub(super) graph: &'a EffectiveGraph,
    pub(super) updated: &'a BTreeMap<RepoPath, Observation>,
    pub(super) removed: &'a BTreeMap<RepoPath, bool>,
    pub(super) previous: &'a BTreeMap<RepoPath, FileRow>,
    pub(super) mode: PublicationMode,
}
impl BasisPreflight {
    /// Observe new valid source bindings and retain all roots before SQL publication.
    /// The caller holds index.lock; diagnostic findings do not reject valid Markdown.
    ///
    /// # Errors
    /// Reports unsafe source reads, Git corruption, and retention failures before SQL begins.
    pub(super) fn validate_basis<P: SourceProvenance + BasisRetainer>(
        &self,
        provenance: &P,
        verification: SnapshotVerification,
    ) -> DocumentResult<()> {
        for basis in &self.bases {
            provenance.observe_sources(ProvenanceRequest {
                source_snapshot: basis.source_snapshot.as_ref(),
                sources: &basis.sources,
            })?;
        }
        verification.validate(provenance, &self.snapshots)
    }
}

/// Prepare only changed-content metadata, releasing each parsed file before the next.
///
/// # Errors
/// Reports capture failures and observed stat/hash changes; invalid Markdown keeps its old basis.
pub(super) fn prepare_basis<F: FileSystem, M: MarkdownCodec>(
    filesystem: &F,
    markdown: &M,
    request: BasisPreparation<'_>,
) -> DocumentResult<BasisPreflight> {
    let BasisPreparation {
        graph,
        updated,
        removed,
        previous,
        mode,
    } = request;
    let mut preflight = BasisPreflight {
        snapshots: BTreeSet::new(),
        bases: Vec::new(),
    };
    let mut replaced: BTreeSet<_> = removed.keys().cloned().collect();
    for (path, observed) in updated {
        // Hash-equal files reuse their last valid projection already represented by graph.
        if mode
            .get_previous(previous.get(path))
            .is_some_and(|previous| previous.hash == observed.hash)
        {
            continue;
        }
        let captured = filesystem.read_file(path)?.ok_or_else(fail_changed)?;
        if captured.stat != observed.stat || Sha256::digest(&captured.bytes) != observed.hash {
            return Err(fail_changed());
        }
        let Ok(parsed) = markdown.parse_file(path, &captured.bytes) else {
            continue;
        };
        replaced.insert(path.clone());
        let Some(parsed) = parsed else {
            continue;
        };
        for element in parsed.elements {
            let id = element.element.id;
            let source_snapshot = element.element.metadata.source_snapshot;
            preflight.snapshots.extend(source_snapshot.iter().cloned());
            let sources = parsed
                .edges
                .iter()
                .filter(|edge| edge.get_source() == &id)
                .filter_map(|edge| match edge.get_target() {
                    EdgeTarget::Source(uri) => Some(uri.clone()),
                    EdgeTarget::Knowledge(_) => None,
                })
                .collect();
            preflight.bases.push(PreparedBasis {
                source_snapshot,
                sources,
            });
        }
    }
    preflight
        .snapshots
        .extend(graph.list_source_snapshots(&replaced));
    Ok(preflight)
}
