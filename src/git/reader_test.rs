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
    let result = reader.object_get_by_oid(
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
    let result = reader.object_get_by_oid(
        &original,
        "blob",
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    );
    // Then: canonical bytes pass, and a later corrupt replacement cannot be hidden by a payload cache.
    assert!(result.is_ok(), "{result:?}");
    fixture.replace_loose_object(&original, &replacement);
    let result = reader.object_get_by_oid(
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
