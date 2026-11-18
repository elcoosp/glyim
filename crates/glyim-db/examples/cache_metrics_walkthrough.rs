//! Consumer walkthrough of the cache-observability seam: the hook that
//! reports what the compile cache actually did (`glyim_db::cache_metrics`).
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-db --example cache_metrics_walkthrough
//! ```
//!
//! The example opens a scratch cache with a [`CacheCollector`] attached and
//! a pinned 2 KiB budget, drives the four lifecycle transitions — miss,
//! store, hit, eviction — and prints the deterministic report the fold
//! renders. The sibling `compile_cache_walkthrough` example walks the
//! cache's key/staleness story; this one answers the *observability*
//! question: how many lookups hit, what did the budget trim, and which
//! object was last served, missed, stored, or evicted. The scratch cache
//! is cleaned up when the process exits.

fn main() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let mut cache = glyim_db::cache::CompileCache::open(scratch.path())
        // Pin the budget explicitly: the builder wins over the
        // GLYIM_CACHE_MAX_BYTES environment default, which is what a
        // deterministic walkthrough needs.
        .with_max_bytes(Some(2048))
        .with_sink(Box::new(glyim_db::cache_metrics::CacheCollector::new()));

    // 1. Miss: a fresh cache reports the lookup that pays a full compile.
    let key = glyim_db::cache::CompileCache::key(
        "fn main() -> i32 { 7 }\n",
        "x86_64-unknown-linux-gnu",
        0,
        true,
    );
    assert!(cache.lookup(&key).is_none(), "fresh cache misses");
    println!("lookup: MISS (reported to the sink)");

    // 2. Store: the atomic temp-file + rename is reported with its size.
    let fake_object = scratch.path().join("fake.o");
    std::fs::write(&fake_object, b"FAKE-OBJ").expect("write fake object");
    cache.store(&key, &fake_object);
    println!("store: STORED (reported to the sink)");

    // 3. Hit: the next lookup is served from the cache.
    assert!(cache.lookup(&key).is_some(), "stored object hits");
    println!("lookup: HIT (reported to the sink)");

    // 4. Eviction: three more 1 KiB objects into the 2 KiB budget trim the
    //    oldest objects out — each removal is reported, never a silent
    //    drop. (The stored object is 8 bytes; the three fresh 1 KiB files
    //    push the directory over budget.)
    for i in 0..3 {
        let src = scratch.path().join(format!("churn{i}.bin"));
        std::fs::write(&src, vec![b'x'; 1024]).expect("write churn object");
        cache.store(&format!("churn{i}"), &src);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("stores: 3 churn objects stored, oldest evicted (reported to the sink)");

    // 5. Take the sink back and render the fold: the aggregate a driver
    //    or embedding would print after a build session.
    let mut sink = cache.take_sink().expect("sink still attached");
    let collector = sink
        .as_any()
        .downcast_ref::<glyim_db::cache_metrics::CacheCollector>()
        .expect("the attached sink was the collector");
    println!(
        "hit rate: {:.1}%, bytes served {} / stored {} / evicted {}",
        collector.hit_rate().expect("lookups ran") * 100.0,
        collector.bytes_served(),
        collector.bytes_stored(),
        collector.bytes_evicted(),
    );
    print!(
        "{}",
        glyim_db::cache_metrics::render_cache_report(collector)
    );
}
