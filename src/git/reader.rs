use super::{
    constants::{BLOB_KIND, HASH_CHUNK_BYTES, OBJECT_INTEGRITY_MESSAGE, TREE_KIND},
    error::SafeIo,
    hashing::ObjectHasher,
    validation::tree_bytes_validate,
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
use std::{collections::BTreeSet, fmt, io, rc::Rc};

const MAX_VALIDATED_TREES: usize = 4096;
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
    validated_trees: BTreeSet<String>,
    #[cfg(test)]
    object_reads: usize,
}

impl fmt::Debug for ObjectReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectReader")
            .field("oid_len", &self.oid_len)
            .field("buffer_capacity", &self.buffer.capacity())
            .field("validated_trees", &self.validated_trees.len())
            .finish_non_exhaustive()
    }
}

impl ObjectReader {
    /// Opens an operation-owned object store without replacements or an object-size cap.
    ///
    /// # Arguments
    ///
    /// * `repo` — repository whose object store and object format are opened.
    /// * `oid_len` — expected hexadecimal object ID length matching the hash format.
    ///
    /// # Errors
    /// Returns an internal error for a format mismatch or failure to open the local store.
    pub(super) fn new(repo: &Repository, oid_len: usize) -> Result<Self, SnapshotError> {
        if repo.object_hash().len_in_hex() != oid_len {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                OBJECT_INTEGRITY_MESSAGE,
            ));
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
        .map_err(map_io_error_to_snapshot_object_store_error)?;
        Ok(Self {
            objects: Rc::new(store).to_handle(),
            oid_len,
            buffer: Vec::new(),
            validated_trees: BTreeSet::new(),
            #[cfg(test)]
            object_reads: 0,
        })
    }

    /// Verifies exact local type and identity, returning only a blob marker or owned tree bytes.
    /// Releases oversized decode capacity on both success and failure.
    ///
    /// # Arguments
    ///
    /// * `oid` — hexadecimal object identity to verify.
    /// * `expected` — exact Git object type required for the object.
    /// * `missing` — error kind used when the object is absent.
    /// * `wrong_type` — error kind used when the object type differs.
    ///
    /// # Errors
    /// Uses caller-selected missing/type errors; decoding and integrity failures are internal.
    pub(super) fn object_get_by_oid(
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
            .object_verify_by_oid(oid, expected, missing, wrong_type)
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

    /// Returns the number of object verification requests on this test reader.
    #[cfg(test)]
    pub(super) fn object_get_read_count(&self) -> usize {
        self.object_reads
    }

    /// Read fresh verified tree bytes, reusing only successful structural validation.
    /// The bounded memo retains identities, never payloads, for this reader's lifetime.
    ///
    /// # Arguments
    ///
    /// * `oid` — hexadecimal object ID of the tree to read.
    /// * `missing` — error kind reported when the object is absent.
    /// * `wrong_type` — error kind reported when the object is not a tree.
    ///
    /// # Errors
    /// Uses caller-selected missing/type errors; decoding, identity and structure failures are internal.
    pub(super) fn tree_bytes_get_by_oid(
        &mut self,
        oid: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<Vec<u8>, SnapshotError> {
        let bytes = self.object_bytes_get_by_oid(oid, TREE_KIND, missing, wrong_type)?;
        // Consult metadata only after verifying current bytes, so replacement or
        // disappearance cannot hide behind an earlier structural success.
        if !self.validated_trees.contains(oid) {
            tree_bytes_validate(&bytes, self.oid_len)?;
            if self.validated_trees.len() < MAX_VALIDATED_TREES {
                self.validated_trees.insert(oid.to_owned());
            }
        }
        Ok(bytes)
    }

    /// Returns verified bytes for an addressed object; no payload survives in a cache.
    ///
    /// # Arguments
    ///
    /// * `oid` — hexadecimal object identity to read.
    /// * `expected` — exact Git object type required for the object.
    /// * `missing` — error kind used when the object is absent.
    /// * `wrong_type` — error kind used when the object type differs.
    ///
    /// # Errors
    /// Uses caller-selected missing/type errors; decoding and checksum failures are internal.
    pub(super) fn object_bytes_get_by_oid(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<Vec<u8>, SnapshotError> {
        let result = self
            .object_verify_by_oid(oid, expected, missing, wrong_type)
            .map(|()| self.buffer.clone());
        if self.buffer.capacity() > MAX_REUSABLE_BYTES {
            self.buffer = Vec::new();
        } else {
            self.buffer.clear();
        }
        result
    }

    fn object_verify_by_oid(
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
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                OBJECT_INTEGRITY_MESSAGE,
            ));
        }
        let id = ObjectId::from_hex(oid.as_bytes()).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
        })?;
        // Bypass Repository's payload cache, even if a caller configured one.
        // The raw local ODB handle cannot invoke promisor fetch or content filters.
        let (object, _) = self
            .objects
            .try_find_cached(&id, &mut self.buffer, &mut Never)
            .map_err(make_decode_error)?
            .ok_or_else(|| SnapshotError::new(missing, "Git object is missing"))?;
        let kind = std::str::from_utf8(object.kind.as_bytes()).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
        })?;
        if kind != expected {
            return Err(SnapshotError::new(wrong_type, "unexpected Git object type"));
        }
        if kind != BLOB_KIND && kind != TREE_KIND {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                OBJECT_INTEGRITY_MESSAGE,
            ));
        }
        let mut hash = ObjectHasher::new(self.oid_len, kind, object.data.len() as u64)?;
        for chunk in object.data.chunks(HASH_CHUNK_BYTES) {
            hash.update(chunk);
        }
        if hash.finish()? != oid {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                OBJECT_INTEGRITY_MESSAGE,
            ));
        }
        Ok(())
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "map_err passes the error by value to the mapper"
)]
fn make_decode_error(error: gix::Error) -> SnapshotError {
    let kind = error
        .downcast_any_ref::<io::Error>()
        .map_or(io::ErrorKind::InvalidData, io::Error::kind);
    SnapshotError::new_with_cause("Git object decode failed", SafeIo(io::Error::from(kind)))
}

/// Maps an I/O failure into an object-store snapshot error.
///
/// # Arguments
///
/// * `error` — I/O failure from opening the object store.
fn map_io_error_to_snapshot_object_store_error(error: io::Error) -> SnapshotError {
    SnapshotError::new_with_cause("Git object store open failed", SafeIo(error))
}

#[cfg(test)]
#[path = "reader_test.rs"]
mod tests;
