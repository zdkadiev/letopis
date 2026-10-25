use super::{
    DocumentResult, FileSystem, NodeMetadata, SourceAvailability, SourceObservation, SourceReader,
    StorageRead,
};
use crate::domain::{EdgeTarget, Sha256, SourceUri};

#[derive(Debug)]
pub(crate) struct SourceService<F> {
    filesystem: F,
}

impl<F: FileSystem> SourceService<F> {
    pub(crate) fn new(filesystem: F) -> Self {
        Self { filesystem }
    }
}

impl<F: FileSystem> SourceReader for SourceService<F> {
    fn read_source<R: StorageRead>(
        &self,
        view: &mut R,
        uri: &SourceUri,
    ) -> DocumentResult<Option<SourceObservation>> {
        let Some(row) = view.read_node(&EdgeTarget::Source(uri.clone()))? else {
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
        let captured = self.filesystem.read_file(local.get_path())?;
        metadata.hash = captured
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
    }
}

#[cfg(test)]
mod tests;
