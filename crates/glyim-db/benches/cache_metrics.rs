//! Compile-cache metrics bench (cache-observability seam).
//!
//! Dependency-free (`harness = false`), mirroring the other seam benches:
//! measures the fold cost of the [`CacheCollector`] over a synthetic
//! 64k-event transcript (the aggregate a long compile session produces),
//! the render cost, and the per-lookup overhead a cache pays when a sink is
//! attached versus the unattached baseline — the seam's zero-cost-when-
//! unused contract in numbers. Run with:
//!
//! ```text
//! cargo bench -p glyim-db --bench cache_metrics
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_db::cache::CompileCache;
use glyim_db::cache_metrics::{CacheCollector, CacheEvent, render_cache_report};

/// Builds a 65,536-event transcript — the realistic long-session shape:
/// every third lookup misses (a real edit), stores trail misses by one
/// step, evictions appear once per 1,536 stored events (the budget trim),
/// the rest are hits. Returns the transcript plus the counts the fold must
/// reproduce (the generator's intent is the contract check).
fn synthetic_transcript() -> (Vec<CacheEvent>, u64, u64, u64, u64) {
    let mut events = Vec::with_capacity(65_536);
    let (mut misses, mut hits, mut stores, mut evictions) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..65_536u64 {
        let key = format!("key{:08x}", i & 0xffff);
        match i % 3 {
            0 => {
                misses += 1;
                events.push(CacheEvent::Miss { key: key.clone() });
            }
            1 => {
                if i % 1536 == 1 {
                    evictions += 1;
                    events.push(CacheEvent::Evicted {
                        key: key.clone(),
                        size_bytes: 4096,
                    });
                } else {
                    stores += 1;
                    events.push(CacheEvent::Stored {
                        key: key.clone(),
                        size_bytes: 4096,
                    });
                }
            }
            _ => {
                hits += 1;
                events.push(CacheEvent::Hit {
                    key,
                    size_bytes: 4096,
                });
            }
        }
    }
    (events, misses, hits, stores, evictions)
}

fn main() {
    let (transcript, misses, hits, stores, evictions) = synthetic_transcript();

    // Fold throughput: the O(1) aggregate over a long session, pinned to
    // the generator's counts (the fold must reproduce them exactly).
    let started = Instant::now();
    let collector = CacheCollector::from_events(black_box(&transcript).iter().cloned());
    let fold_elapsed = started.elapsed();
    assert_eq!(collector.misses(), misses, "fold must reproduce the misses");
    assert_eq!(collector.hits(), hits, "fold must reproduce the hits");
    assert_eq!(collector.stores(), stores, "fold must reproduce the stores");
    assert_eq!(
        collector.evictions(),
        evictions,
        "fold must reproduce the evictions"
    );
    assert_eq!(collector.bytes_evicted(), evictions * 4096);
    assert_eq!(
        collector.bytes_stored() + collector.bytes_served(),
        (stores + hits) * 4096
    );

    // Render cost: the deterministic report listing.
    let report = render_cache_report(black_box(&collector));
    assert_eq!(report, render_cache_report(&collector));
    let started = Instant::now();
    for _ in 0..1_000 {
        black_box(render_cache_report(black_box(&collector)));
    }
    let render_elapsed = started.elapsed();

    // Per-lookup overhead: a real scratch cache driven 2,000 lookups, with
    // and without a sink attached. The delta is the seam's cost on the hot
    // path (one lock-and-check per transition).
    let scratch = tempfile::tempdir().expect("scratch dir");
    let object = scratch.path().join("object.bin");
    std::fs::write(&object, b"OBJECT-BYTES").expect("write object");
    let bare = CompileCache::open(scratch.path());
    let key = "benchkey";
    bare.store(key, &object);
    let started = Instant::now();
    for _ in 0..2_000 {
        black_box(bare.lookup(black_box(key)));
    }
    let bare_elapsed = started.elapsed();
    drop(bare);

    let watched = CompileCache::open(scratch.path()).with_sink(Box::new(CacheCollector::new()));
    watched.store(key, &object);
    let started = Instant::now();
    for _ in 0..2_000 {
        black_box(watched.lookup(black_box(key)));
    }
    let watched_elapsed = started.elapsed();
    drop(watched);

    println!(
        "cache metrics bench: {}-event transcript\n  fold  : {fold_elapsed:?} ({} misses, {} hits, {} stores, {} evictions)\n  render: {render_elapsed:?} (1,000 renders, {} bytes each)",
        transcript.len(),
        misses,
        hits,
        stores,
        evictions,
        report.len(),
    );
    println!(
        "  lookup overhead: bare {bare_elapsed:?} vs sink-attached {watched_elapsed:?} (2,000 hits each)"
    );
}
