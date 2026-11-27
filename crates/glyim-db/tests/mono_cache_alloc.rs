//! Plan M2.7 quick-win pin — "`set_mono_cache` without per-item string
//! clones".
//!
//! The win is allocation-shaped, so the pin is allocation-shaped too:
//! [`glyim_bench`]'s counting allocator (the M2.2 harness's opt-in
//! `counting_allocator!` seam) makes the clone count a *deterministic*
//! observable, unlike wall clock on this two-core sandbox. One test binary,
//! one test, counters reset around the single measured call:
//!
//! - **old implementation** (`seen.insert(item.clone())`): one `String`
//!   clone allocation per input item — ≥ `N` events for `N` items;
//! - **M2.7 implementation**: the dedup borrows the input and moves
//!   first-seen strings into place, so the only allocations are the
//!   ordered index collections' internal nodes (≈ `N/11` per `BTree`
//!   level structure — no string events at all).
//!
//! The budget sits far below the old behavior's floor, so this test fails
//! against the pre-M2.7 code and pins the win from here on.

// Install the counting allocator for this (single-test) test binary.
glyim_bench::counting_allocator!();

use glyim_db::Database;

/// The clone budget for one `set_mono_cache` call over `N` items. The old
/// implementation allocates one clone per item (`>= N` string events); the
/// clone-free implementation allocates only the index collections' node
/// internals (measured: 1 961 events at `N = 8_000`). The budget carries
/// ~1.5× headroom over the measured value — the same discipline as
/// `perf/budgets.toml` — while staying far below the old floor.
const CLONE_BUDGET_EVENTS: u64 = 3_000;
const ITEMS: usize = 8_000;

#[test]
fn set_mono_cache_dedups_without_per_item_clones() {
    let db = Database::default();
    let items: Vec<String> = (0..ITEMS)
        .map(|i| format!("__glyim_fn_{i}_instantiation"))
        .collect();

    // Every item distinct: the old implementation clones all of them into
    // the dedup set even though nothing is a duplicate — the worst case
    // for the win, and the shape the emit phase feeds on real compiles.
    glyim_bench::alloc::reset_alloc_stats();
    db.set_mono_cache(items);
    let delta = glyim_bench::alloc::alloc_snapshot();

    let cache = db.mono_cache();
    let stored = cache.as_ref().expect("cache should be populated");
    assert_eq!(stored.len(), ITEMS, "distinct inputs must be kept whole");

    // Visible under `--nocapture`: the pin's own evidence line.
    println!(
        "set_mono_cache alloc events for {ITEMS} distinct symbols: {}",
        delta.count
    );
    assert!(
        delta.count < CLONE_BUDGET_EVENTS,
        "set_mono_cache allocated {} events for {ITEMS} symbols \
         (budget {CLONE_BUDGET_EVENTS}); the old per-item-clone \
         implementation allocates >= {ITEMS}",
        delta.count
    );
}
