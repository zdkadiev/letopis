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
mod tests {
    use super::ObjectReader;
    use crate::git::reader::VerifiedObject;
    use crate::service::SnapshotErrorKind;
    use std::env;

    mod allocation {
        use std::{
            alloc::{GlobalAlloc, Layout, System},
            sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        };

        static ENABLED: AtomicBool = AtomicBool::new(false);
        static LARGEST: AtomicUsize = AtomicUsize::new(0);
        static TOTAL: AtomicUsize = AtomicUsize::new(0);
        #[global_allocator]
        static ALLOCATOR: Meter = Meter;
        struct Meter;

        pub(super) fn start() {
            LARGEST.store(0, Ordering::Relaxed);
            TOTAL.store(0, Ordering::Relaxed);
            ENABLED.store(true, Ordering::SeqCst);
        }

        pub(super) fn stop() -> (usize, usize) {
            ENABLED.store(false, Ordering::SeqCst);
            (
                LARGEST.load(Ordering::Relaxed),
                TOTAL.load(Ordering::Relaxed),
            )
        }

        pub(super) fn get_peak_rss() -> u64 {
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            // SAFETY: getrusage initializes this correctly sized output on success.
            assert_eq!(
                unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
                0
            );
            // SAFETY: the successful call initialized usage above.
            let rss = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).unwrap();
            if cfg!(target_os = "macos") {
                rss
            } else {
                rss * 1024
            }
        }

        // SAFETY: every allocation is delegated unchanged to System; counters allocate nothing.
        unsafe impl GlobalAlloc for Meter {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                record(layout.size());
                // SAFETY: GlobalAlloc's caller supplies a valid layout.
                unsafe { System.alloc(layout) }
            }
            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                record(layout.size());
                // SAFETY: GlobalAlloc's caller supplies a valid layout.
                unsafe { System.alloc_zeroed(layout) }
            }
            unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
                // SAFETY: the original System allocation and layout are forwarded unchanged.
                unsafe {
                    System.dealloc(pointer, layout);
                }
            }
            unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
                record(size);
                // SAFETY: GlobalAlloc's caller supplies the original allocation and valid new size.
                unsafe { System.realloc(pointer, layout, size) }
            }
        }

        fn record(size: usize) {
            if ENABLED.load(Ordering::Relaxed) {
                LARGEST.fetch_max(size, Ordering::Relaxed);
                TOTAL.fetch_add(size, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn run_owned_reader() {
        // Given: only the integration harness supplies an isolated repository.
        let Some(root) = env::var_os("LETOPIS_READER_ROOT") else {
            return;
        };
        let repo = gix::open(root).unwrap();
        let oid = env::var("LETOPIS_READER_OID").unwrap();
        let kind = env::var("LETOPIS_READER_KIND").unwrap_or_else(|_| "tree".into());
        let outcome = env::var("LETOPIS_READER_OUTCOME").unwrap_or_else(|_| "ok".into());
        let mut reader = ObjectReader::new(&repo, oid.len()).unwrap();
        // When: the raw library reader verifies one object, measuring only that call's allocations.
        allocation::start();
        let result = reader.get_object(
            &oid,
            &kind,
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        );
        let allocations = allocation::stop();
        // Then: identity/type/missing failures are classified and oversized capacity is released.
        match outcome.as_str() {
            "ok" => match (kind.as_str(), result.unwrap()) {
                ("blob", VerifiedObject::Blob) => {}
                ("tree", VerifiedObject::Tree(bytes)) => assert!(bytes.is_empty()),
                unexpected => panic!("unexpected verified object: {unexpected:?}"),
            },
            "missing" => assert_eq!(result.unwrap_err().get_kind(), SnapshotErrorKind::NotFound),
            "wrong_type" => assert_eq!(
                result.unwrap_err().get_kind(),
                SnapshotErrorKind::InvalidRequest
            ),
            "corrupt" => {
                let error = result.unwrap_err();
                assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
                assert!(!format!("{error:?}").contains("CANARY"));
            }
            _ => panic!("unknown outcome"),
        }
        assert!(reader.buffer.capacity() <= 4 * 1024 * 1024);
        println!(
            "READER_RECEIPT {}",
            serde_json::json!({
                "oid": oid, "outcome": outcome, "retained_bytes": reader.buffer.capacity(),
                "largest_allocation_bytes": allocations.0, "total_allocation_bytes": allocations.1,
                "rss_peak_bytes": allocation::get_peak_rss(),
            })
        );
    }

    #[test]
    fn raw_reads_ignore_configured_allocation_caps_replacements_and_payload_caches() {
        // Given: valid raw objects, a one-byte gix cap and a Git replacement for the requested identity.
        let fixture =
            crate::process_test_support::TempRepo::new(crate::process_test_support::FORMATS[0]);
        let original = fixture.get_oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"raw original bytes"),
        );
        let replacement = fixture.get_oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"replacement bytes"),
        );
        fixture.run_git_successfully(&["config", "gitoxide.objects.allocLimit", "1"], None);
        fixture.run_git_successfully(&["replace", &original, &replacement], None);
        let repo = gix::open(fixture.get_root()).unwrap();
        let mut reader = ObjectReader::new(&repo, original.len()).unwrap();
        // When: the reader accesses raw local identity, without gix's optional cap or replacement.
        let result = reader.get_object(
            &original,
            "blob",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        );
        // Then: canonical bytes pass, and a later corrupt replacement cannot be hidden by a payload cache.
        assert!(result.is_ok(), "{result:?}");
        fixture.replace_loose_object(&original, &replacement);
        let result = reader.get_object(
            &original,
            "blob",
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        );
        assert_eq!(
            result.unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}
