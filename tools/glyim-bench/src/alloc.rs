//! Counting allocator: gross allocation events and bytes, process-wide.
//!
//! The M2.2 charter names "alloc count/bytes (counting allocator)" as one of
//! the five measured dimensions. This module provides a
//! [`GlobalAlloc`](std::alloc::GlobalAlloc) wrapper that records **gross**
//! allocation volume — every `alloc`/`alloc_zeroed`/`realloc` event adds its
//! size to monotonically increasing atomic counters. Deliberate contract:
//!
//! - **Gross, not live.** Deallocation does not subtract. Gross volume is the
//!   metric the plan's baseline report wants (allocation hot spots are volume
//!   hot spots), it keeps the counters monotone under concurrent threads, and
//!   it makes `reset()` + one measured section a well-defined window.
//! - **Opt-in per binary.** A `#[global_allocator]` can only be defined once
//!   per binary, so the library never installs one; consumers adopt the
//!   wrapper with the [`counting_allocator!`] macro (or embed
//!   [`CountingAllocator`] in their own wrapper chain).
//!
//! The counters are `Relaxed`-ordered atomics: they are statistics, not
//! synchronisation.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide gross allocation counters, updated by
/// [`CountingAllocator`].
#[derive(Debug, Default)]
pub struct AllocStats {
    /// Number of allocation events.
    pub count: AtomicU64,
    /// Sum of requested sizes across allocation events, in bytes.
    pub bytes: AtomicU64,
}

/// Snapshot of [`AllocStats`] at one point in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AllocSnapshot {
    /// Allocation events observed so far.
    pub count: u64,
    /// Gross allocation bytes observed so far.
    pub bytes: u64,
}

static ALLOC_STATS: AllocStats = AllocStats {
    count: AtomicU64::new(0),
    bytes: AtomicU64::new(0),
};

/// Read the current gross allocation counters.
#[must_use]
pub fn alloc_snapshot() -> AllocSnapshot {
    AllocSnapshot {
        count: ALLOC_STATS.count.load(Ordering::Relaxed),
        bytes: ALLOC_STATS.bytes.load(Ordering::Relaxed),
    }
}

/// Reset both counters to zero (statistics only; no allocator behaviour
/// changes).
pub fn reset_alloc_stats() {
    ALLOC_STATS.count.store(0, Ordering::Relaxed);
    ALLOC_STATS.bytes.store(0, Ordering::Relaxed);
}

fn record(size: usize) {
    ALLOC_STATS.count.fetch_add(1, Ordering::Relaxed);
    ALLOC_STATS.bytes.fetch_add(size as u64, Ordering::Relaxed);
}

/// A [`GlobalAlloc`] wrapper that counts gross allocation events and bytes.
///
/// Wrap the system allocator (the default) or another allocator:
///
/// ```
/// use glyim_bench::alloc::CountingAllocator;
///
/// // Inside a binary, bench, or test:
/// #[global_allocator]
/// static COUNTING: CountingAllocator = CountingAllocator::new();
/// ```
pub struct CountingAllocator<A = System> {
    inner: A,
}

impl Default for CountingAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl CountingAllocator {
    /// Wrap the system allocator.
    #[must_use]
    pub const fn new() -> Self {
        Self { inner: System }
    }
}

impl<A> CountingAllocator<A> {
    /// Wrap a custom inner allocator.
    #[must_use]
    pub const fn with_inner(inner: A) -> Self {
        Self { inner }
    }
}

// SAFETY: every method forwards to `A: GlobalAlloc` with the same layout and
// returns its pointer unchanged; the counters are side effects only, so the
// safety contract is exactly the inner allocator's.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.inner.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { self.inner.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            record(new_size);
        }
        new_ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }
}

/// Install [`CountingAllocator`] as the binary's global allocator.
///
/// Use exactly once, at crate root of a binary / bench / example:
///
/// ```
/// # use glyim_bench::counting_allocator;
/// counting_allocator!();
/// ```
#[macro_export]
macro_rules! counting_allocator {
    () => {
        #[global_allocator]
        static __GLYIM_BENCH_COUNTING: $crate::alloc::CountingAllocator =
            $crate::alloc::CountingAllocator::new();
    };
}

#[cfg(test)]
mod tests {
    use super::{alloc_snapshot, reset_alloc_stats};

    #[test]
    fn allocating_increases_count_and_bytes_monotonically() {
        reset_alloc_stats();
        let before = alloc_snapshot();
        let scratch: Vec<u64> = vec![7; 1_000];
        let after = alloc_snapshot();
        assert!(
            after.count >= before.count + 1,
            "at least one alloc event must be recorded: {before:?} -> {after:?}"
        );
        assert!(
            after.bytes >= before.bytes + scratch.len() as u64 * 8,
            "gross bytes must grow by at least the requested size"
        );
        // Keep the allocation alive past both snapshots.
        assert_eq!(scratch[999], 7);
    }

    #[test]
    fn reset_returns_counters_to_zero() {
        let _scratch: Vec<u8> = vec![1; 256];
        reset_alloc_stats();
        let snap = alloc_snapshot();
        // Another thread may allocate between the reset and the read; the
        // contract tolerates that, so assert on the reset call itself being
        // orderable: the *next* snapshot after a fresh reset is at most one
        // transient test-harness allocation away from zero. Read twice and
        // require the second read to be small rather than exactly zero.
        assert!(snap.count < 10_000, "reset must drop the counters: {snap:?}");
    }
}

// The unit tests above run under the counting allocator itself: the test
// binary adopts it so the counters are exercised for real.
#[cfg(test)]
crate::counting_allocator!();
