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

pub(super) fn read_peak_rss() -> u64 {
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
