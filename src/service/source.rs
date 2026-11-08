use super::utils::{
    map_document_transaction_error_to_document_error, map_file_result_to_document_result,
};
use super::{
    CacheDatabase, DocumentResult, FileReader, NodeMetadata, SourceAvailability, SourceObservation,
    SourceReader, StorageRead,
};
use crate::domain::{EdgeTarget, Sha256, SourceUri};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct SourceService<F, C> {
    filesystem: F,
    cache: std::rc::Rc<C>,
}

impl<F: FileReader, C> SourceService<F, C> {
    pub(crate) fn new(cache: std::rc::Rc<C>, filesystem: F) -> Self {
        Self { filesystem, cache }
    }
}

impl<F: FileReader, C: CacheDatabase> SourceReader for SourceService<F, C> {
    fn get_source_by_uri(&self, uri: &SourceUri) -> DocumentResult<Option<SourceObservation>> {
        self.cache
            .with_read_tx(|| {
                let Some(row) = StorageRead::nodes_get_by_uri(
                    self.cache.as_ref(),
                    &EdgeTarget::Source(uri.clone()),
                )?
                else {
                    return Ok(None);
                };
                let NodeMetadata::Source(mut metadata) = row.metadata else {
                    return Ok(None);
                };
                let Some(local) = uri.get_local() else {
                    metadata.hash = None;
                    metadata.lines = None;
                    return Ok(Some(SourceObservation {
                        metadata,
                        availability: SourceAvailability::Unfetched,
                    }));
                };
                let file = map_file_result_to_document_result(
                    self.filesystem
                        .file_get_by_path(Path::new(local.get_path().as_str())),
                )?;
                metadata.hash = file
                    .as_ref()
                    .and_then(|file| local.select_bytes(&file.bytes))
                    .map(Sha256::digest);
                metadata.lines = local.get_lines();
                let availability = if metadata.hash.is_some() {
                    SourceAvailability::Available
                } else {
                    SourceAvailability::SourceNotFound
                };
                Ok(Some(SourceObservation {
                    metadata,
                    availability,
                }))
            })
            .map_err(map_document_transaction_error_to_document_error)
    }
}

#[cfg(test)]
#[path = "source_test.rs"]
mod tests;
