use super::{
    constants::{BLOB_KIND, HASH_CHUNK_BYTES, TREE_KIND},
    error::make_io_error,
    hashing::ObjectHasher,
};
use crate::service::{SnapshotError, SnapshotErrorKind};
use gix::{
    Repository,
    hash::ObjectId,
    odb::{
        Store,
        pack::{Find, cache::Never},
        store::{Handle, init::Options},
    },
};
use std::{fmt, io, rc::Rc};

const MAX_REUSABLE_BYTES: usize = 4 * 1024 * 1024;

/// Keep only the current verified tree body; blob payloads are discarded after hashing.
#[derive(Debug)]
pub(super) enum VerifiedObject {
    Blob,
    Tree(Vec<u8>),
}

/// Owns one uncached local ODB view and a bounded reusable decode allocation.
pub(super) struct ObjectReader {
    objects: Handle<Rc<Store>>,
    oid_len: usize,
    buffer: Vec<u8>,
    #[cfg(test)]
    object_reads: usize,
}

impl fmt::Debug for ObjectReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectReader")
            .field("oid_len", &self.oid_len)
            .field("buffer_capacity", &self.buffer.capacity())
            .finish_non_exhaustive()
    }
}

impl ObjectReader {
    /// Opens a raw operation-owned handle without replacements or an object-size cap.
    ///
    /// # Errors
    /// Returns an internal error for a format mismatch or failure to open the local store.
    pub(super) fn new(repo: &Repository, oid_len: usize) -> Result<Self, SnapshotError> {
        if repo.object_hash().len_in_hex() != oid_len {
            return Err(fail_integrity());
        }
        // Construct one operation-owned raw handle: gix's repository config can
        // impose a 16 MiB reduced-trust cap or user cap, neither belongs to this API.
        let store = Store::at_opts(
            repo.objects.store_ref().path().to_owned(),
            repo.object_hash(),
            &mut std::iter::empty(),
            Options {
                alloc_limit_bytes: None,
                ..Options::default()
            },
        )
        .map_err(|error| make_io_error("Git object store open failed", error))?;
        Ok(Self {
            objects: Rc::new(store).to_handle(),
            oid_len,
            buffer: Vec::new(),
            #[cfg(test)]
            object_reads: 0,
        })
    }

    /// Verifies exact local type and identity, returning only a blob marker or owned tree bytes.
    /// Releases oversized decode capacity on both success and failure.
    ///
    /// # Errors
    /// Uses caller-selected missing/type errors; decoding and integrity failures are internal.
    pub(super) fn get_object(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<VerifiedObject, SnapshotError> {
        #[cfg(test)]
        {
            self.object_reads += 1;
        }
        let result = self
            .verify_object(oid, expected, missing, wrong_type)
            .map(|()| {
                if expected == TREE_KIND {
                    VerifiedObject::Tree(self.buffer.clone())
                } else {
                    VerifiedObject::Blob
                }
            });
        // Delta decoding may grow this allocation beyond the reconstructed body.
        // Release it on both success and error; the threshold is not a size limit.
        if self.buffer.capacity() > MAX_REUSABLE_BYTES {
            self.buffer = Vec::new();
        } else {
            self.buffer.clear();
        }
        result
    }

    /// Count object verification requests on this owned test reader.
    #[cfg(test)]
    pub(super) fn get_object_read_count(&self) -> usize {
        self.object_reads
    }

    /// Read verified bytes for an addressed object; no payload survives in a cache.
    ///
    /// # Errors
    /// Uses caller-selected missing/type errors; decoding and checksum failures are internal.
    pub(super) fn get_bytes(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<Vec<u8>, SnapshotError> {
        let result = self
            .verify_object(oid, expected, missing, wrong_type)
            .map(|()| self.buffer.clone());
        if self.buffer.capacity() > MAX_REUSABLE_BYTES {
            self.buffer = Vec::new();
        } else {
            self.buffer.clear();
        }
        result
    }

    fn verify_object(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<(), SnapshotError> {
        if oid.len() != self.oid_len
            || !oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(fail_integrity());
        }
        let id = ObjectId::from_hex(oid.as_bytes()).map_err(|_| fail_integrity())?;
        // Bypass Repository's payload cache, even if a caller configured one.
        // The raw local ODB handle cannot invoke promisor fetch or content filters.
        let (object, _) = self
            .objects
            .try_find_cached(&id, &mut self.buffer, &mut Never)
            .map_err(|error| make_decode_error(&error))?
            .ok_or_else(|| SnapshotError::new(missing, "Git object is missing"))?;
        let kind = std::str::from_utf8(object.kind.as_bytes()).map_err(|_| fail_integrity())?;
        if kind != expected {
            return Err(SnapshotError::new(wrong_type, "unexpected Git object type"));
        }
        if kind != BLOB_KIND && kind != TREE_KIND {
            return Err(fail_integrity());
        }
        let mut hash = ObjectHasher::new(self.oid_len, kind, object.data.len() as u64)?;
        for chunk in object.data.chunks(HASH_CHUNK_BYTES) {
            hash.update(chunk);
        }
        if hash.finish()? != oid {
            return Err(fail_integrity());
        }
        Ok(())
    }
}

fn make_decode_error(error: &gix::Error) -> SnapshotError {
    let kind = error
        .downcast_any_ref::<io::Error>()
        .map_or(io::ErrorKind::InvalidData, io::Error::kind);
    make_io_error("Git object decode failed", io::Error::from(kind))
}

fn fail_integrity() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git object integrity failed",
    )
}

#[cfg(test)]
mod tests;
