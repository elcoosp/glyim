//! Cache-observability seam: report what the [`CompileCache`](crate::cache::CompileCache)
//! actually did.
//!
//! # Why
//!
//! The compile cache is deliberately best-effort: a miss, a failed store, and
//! a budget eviction all leave the compiler's output unchanged, so nothing
//! *must* be reported. But that silence has a cost for consumers — the driver
//! prints one unconditional `cache hit — skipping compilation` line on the
//! hit path, and nothing at all for misses, stores, and evictions. A
//! rebuild that is unexpectedly slow cannot be told apart from a rebuild
//! that recompiled everything, and an eviction that pushed a hot object out
//! of a size-capped cache is invisible until the next full recompile.
//!
//! # What
//!
//! A hook on the cache's lifecycle transitions, shaped like the workspace's
//! other observability seams (the bytecode VM's `TraceSink` /
//! `TraceCollector` / `render_trace` family): the cache hands one
//! [`CacheEvent`] to the attached [`CacheSink`] just after each transition
//! completes, a [`CacheCollector`] folds the stream into an O(1) aggregate
//! (counts and byte totals — memory is a constant, never a function of how
//! many compiles ran), and [`render_cache_report`] renders the aggregate as
//! the same deterministic diagnostic-text style the other seams use.
//! [`CacheCollector::from_events`] re-folds a captured transcript, and
//! [`CacheTracingSink`] emits one `tracing::debug!` event per transition for
//! the workspace-standard observability layer.
//!
//! # Contract
//!
//! The seam is **read-only** (a sink observes; it cannot change whether an
//! object is served or stored), **total** (every transition is reported: a
//! hit, a miss — including the empty-file guard case, a stored object, and
//! every budget eviction; a failed store reports nothing because nothing was
//! stored), and **zero-cost when unused** (a cache without an attached sink
//! pays one lock-and-check per transition, exactly the pre-seam behavior).
//!
//! Attach a sink with [`CompileCache::with_sink`](crate::cache::CompileCache::with_sink)
//! and take it back with
//! [`CompileCache::take_sink`](crate::cache::CompileCache::take_sink);
//! pin the on-disk budget with
//! [`CompileCache::with_max_bytes`](crate::cache::CompileCache::with_max_bytes)
//! (explicit builder wins over the `GLYIM_CACHE_MAX_BYTES` environment
//! default). See [`crate::cache`] for the cache's own location and
//! correctness story.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One lifecycle transition of the compile cache, handed to the attached
/// [`CacheSink`] just after the transition completed.
///
/// `key` is the cache key exactly as the caller passed it (the lowercase
/// SHA-256 hex string [`CompileCache::key`](crate::cache::CompileCache::key)
/// produces); `size_bytes` is the object's size on the transitions that move
/// bytes in or out of the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheEvent {
    /// A lookup found a present, non-empty object — the hit a driver serves
    /// from. `size_bytes` is the object's file size.
    Hit {
        /// The cache key the lookup asked for.
        key: String,
        /// The cached object's size in bytes.
        size_bytes: u64,
    },
    /// A lookup found nothing: a fresh cache, an evicted object, or the
    /// empty-file guard (a zero-byte file is never a valid object). A miss
    /// costs the caller a full compile — the reason this seam exists.
    Miss {
        /// The cache key the lookup asked for.
        key: String,
    },
    /// `store` completed its atomic temp-file + rename; the object is
    /// visible to future lookups. `size_bytes` is the copied payload's
    /// size. A store whose copy or rename failed reports nothing (nothing
    /// was stored — the best-effort contract keeps the compiler silent
    /// there too).
    Stored {
        /// The cache key the object is stored under.
        key: String,
        /// The stored object's size in bytes.
        size_bytes: u64,
    },
    /// `store`'s budget trim removed an existing object (oldest first, ties
    /// broken by key). The bytes left the cache — a *displayed* fact, never
    /// a silent drop. The object just stored is never the evicted one
    /// (deleting the fresh object would be more hostile than briefly
    /// exceeding the budget).
    Evicted {
        /// The cache key of the removed object (its file stem).
        key: String,
        /// The removed object's size in bytes.
        size_bytes: u64,
    },
}

/// The observer the compile cache hands each lifecycle transition to.
///
/// Implementors must treat [`CacheSink::event`] as an observation only: the
/// sink cannot influence whether a lookup hits, whether an object is stored,
/// or what the budget trim removes. The method must not panic (the cache
/// never defends against a panicking sink — a sink that panics aborts the
/// compile, which is the embedding host's explicit choice). The
/// [`CacheSink::as_any`] hook lets a caller recover the typed sink after
/// `CompileCache::take_sink`.
pub trait CacheSink: 'static {
    /// One cache transition, observed just after it completed.
    fn event(&mut self, event: &CacheEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `CompileCache::take_sink`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that folds every event into an O(1) aggregate.
///
/// The counters are the cache's headline story — how many lookups hit, how
/// many compiles were skipped or paid, what the budget trim removed — kept
/// in constant memory: an unbounded sequence of compiles must not grow the
/// collector. The `last_*_key` readers pin the most recent key of each kind
/// so a report names *which* object was last served, missed, stored, or
/// evicted.
///
/// [`CacheCollector::from_events`] re-folds a captured transcript into the
/// same aggregate (the transcript-replay contract the VM's stream folds
/// follow), and the fold is `PartialEq`-visible, so tests can assert
/// incremental and re-folded aggregates are identical.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheCollector {
    hits: u64,
    misses: u64,
    stores: u64,
    evictions: u64,
    bytes_served: u64,
    bytes_stored: u64,
    bytes_evicted: u64,
    last_hit_key: Option<String>,
    last_miss_key: Option<String>,
    last_store_key: Option<String>,
    last_evicted_key: Option<String>,
}

impl CacheCollector {
    /// Create an empty aggregate (no events folded).
    #[must_use]
    pub fn new() -> CacheCollector {
        CacheCollector::default()
    }

    /// Fold a captured event transcript into the same aggregate an
    /// incremental fold would have produced.
    #[must_use]
    pub fn from_events(events: impl IntoIterator<Item = CacheEvent>) -> CacheCollector {
        let mut collector = CacheCollector::new();
        for event in events {
            collector.event(&event);
        }
        collector
    }

    /// How many lookups ran (hits + misses — every [`CacheEvent::Hit`] and
    /// [`CacheEvent::Miss`] folds here).
    #[must_use]
    pub fn lookups(&self) -> u64 {
        self.hits + self.misses
    }

    /// How many lookups found a cached object.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// How many lookups found nothing (each one paid a full compile).
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// How many objects became visible to future lookups.
    #[must_use]
    pub fn stores(&self) -> u64 {
        self.stores
    }

    /// How many objects the budget trim removed.
    #[must_use]
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Total bytes served from the cache (sum of hit sizes).
    #[must_use]
    pub fn bytes_served(&self) -> u64 {
        self.bytes_served
    }

    /// Total bytes written into the cache (sum of stored sizes).
    #[must_use]
    pub fn bytes_stored(&self) -> u64 {
        self.bytes_stored
    }

    /// Total bytes the budget trim removed (sum of evicted sizes).
    #[must_use]
    pub fn bytes_evicted(&self) -> u64 {
        self.bytes_evicted
    }

    /// Fraction of lookups that hit, or `None` when no lookup ran (an
    /// average over nothing is not a fact).
    #[must_use]
    pub fn hit_rate(&self) -> Option<f64> {
        let lookups = self.lookups();
        if lookups == 0 {
            None
        } else {
            Some(self.hits as f64 / lookups as f64)
        }
    }

    /// The key of the most recent hit, if any hit was observed.
    #[must_use]
    pub fn last_hit_key(&self) -> Option<&str> {
        self.last_hit_key.as_deref()
    }

    /// The key of the most recent miss, if any miss was observed.
    #[must_use]
    pub fn last_miss_key(&self) -> Option<&str> {
        self.last_miss_key.as_deref()
    }

    /// The key of the most recent store, if any store was observed.
    #[must_use]
    pub fn last_store_key(&self) -> Option<&str> {
        self.last_store_key.as_deref()
    }

    /// The key of the most recent eviction, if any eviction was observed.
    #[must_use]
    pub fn last_evicted_key(&self) -> Option<&str> {
        self.last_evicted_key.as_deref()
    }
}

impl CacheSink for CacheCollector {
    fn event(&mut self, event: &CacheEvent) {
        match event {
            CacheEvent::Hit { key, size_bytes } => {
                self.hits += 1;
                self.bytes_served += size_bytes;
                self.last_hit_key = Some(key.clone());
            }
            CacheEvent::Miss { key } => {
                self.misses += 1;
                self.last_miss_key = Some(key.clone());
            }
            CacheEvent::Stored { key, size_bytes } => {
                self.stores += 1;
                self.bytes_stored += size_bytes;
                self.last_store_key = Some(key.clone());
            }
            CacheEvent::Evicted { key, size_bytes } => {
                self.evictions += 1;
                self.bytes_evicted += size_bytes;
                self.last_evicted_key = Some(key.clone());
            }
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A sink that emits one `tracing::debug!` event per cache transition — the
/// seam's counterpart for the workspace-standard observability layer (the
/// same `tracing` dependency the cache crate already carries). Enable it
/// per-subscriber, never by paying the cost unconditionally.
pub struct CacheTracingSink;

impl CacheSink for CacheTracingSink {
    fn event(&mut self, event: &CacheEvent) {
        match event {
            CacheEvent::Hit { key, size_bytes } => {
                debug!(key, size_bytes, "cache hit");
            }
            CacheEvent::Miss { key } => {
                debug!(key, "cache miss");
            }
            CacheEvent::Stored { key, size_bytes } => {
                debug!(key, size_bytes, "cache store");
            }
            CacheEvent::Evicted { key, size_bytes } => {
                debug!(key, size_bytes, "cache eviction");
            }
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a [`CacheCollector`] aggregate into the deterministic
/// diagnostic-text style the other observability seams use.
///
/// The empty state spells an explicit `no cache events recorded` line (never
/// an empty string), each observed kind names its most recent key, and the
/// hit rate is spelled `n/a (no lookups)` when no lookup ran — two renders
/// of the same aggregate are byte-identical, which is the contract the
/// regression gates over rendered listings rely on.
#[must_use]
pub fn render_cache_report(collector: &CacheCollector) -> String {
    if collector.lookups() == 0 && collector.stores() == 0 && collector.evictions() == 0 {
        return "no cache events recorded\n".to_string();
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "cache report: {} lookups (hits {}, misses {})",
        collector.lookups(),
        collector.hits(),
        collector.misses()
    );
    if collector.lookups() == 0 {
        // A pure-writer process (stores or evictions without a lookup in
        // this run): not the empty state, and no hit rate to average.
        let _ = writeln!(out, "  hit rate: n/a (no lookups)");
    } else {
        // Unwrap-free formatting: `lookups() > 0` guarantees the rate.
        let rate = collector.hit_rate().unwrap_or_default();
        let _ = writeln!(out, "  hit rate: {:.1}%", rate * 100.0);
    }
    let _ = writeln!(
        out,
        "  stores: {}, evictions: {}",
        collector.stores(),
        collector.evictions()
    );
    let _ = writeln!(
        out,
        "  bytes served: {}, stored: {}, evicted: {}",
        collector.bytes_served(),
        collector.bytes_stored(),
        collector.bytes_evicted()
    );
    if let Some(key) = collector.last_hit_key() {
        let _ = writeln!(out, "  last hit key: {key}");
    }
    if let Some(key) = collector.last_miss_key() {
        let _ = writeln!(out, "  last miss key: {key}");
    }
    if let Some(key) = collector.last_store_key() {
        let _ = writeln!(out, "  last store key: {key}");
    }
    if let Some(key) = collector.last_evicted_key() {
        let _ = writeln!(out, "  last evicted key: {key}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{CacheCollector, CacheEvent, CacheSink, render_cache_report};
    use crate::cache::CompileCache;
    use std::time::Duration;

    /// One miss → store → hit → second miss → store → eviction cycle, the
    /// exact shape the wire-up tests below drive through a real cache.
    fn event_list() -> Vec<CacheEvent> {
        vec![
            CacheEvent::Miss {
                key: "k1".to_string(),
            },
            CacheEvent::Stored {
                key: "k1".to_string(),
                size_bytes: 128,
            },
            CacheEvent::Hit {
                key: "k1".to_string(),
                size_bytes: 128,
            },
            CacheEvent::Miss {
                key: "k2".to_string(),
            },
            CacheEvent::Stored {
                key: "k2".to_string(),
                size_bytes: 64,
            },
            CacheEvent::Evicted {
                key: "k1".to_string(),
                size_bytes: 128,
            },
        ]
    }

    /// A test sink that records the event transcript in arrival order.
    struct Recorder(Vec<CacheEvent>);

    impl CacheSink for Recorder {
        fn event(&mut self, event: &CacheEvent) {
            self.0.push(event.clone());
        }

        fn as_any(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn collector_folds_counts_and_bytes() {
        let mut collector = CacheCollector::new();
        for event in event_list() {
            collector.event(&event);
        }
        assert_eq!(collector.lookups(), 3);
        assert_eq!(collector.hits(), 1);
        assert_eq!(collector.misses(), 2);
        assert_eq!(collector.stores(), 2);
        assert_eq!(collector.evictions(), 1);
        assert_eq!(collector.bytes_served(), 128);
        assert_eq!(collector.bytes_stored(), 192);
        assert_eq!(collector.bytes_evicted(), 128);
        let rate = collector.hit_rate().expect("lookups ran, rate exists");
        assert!(
            (rate - 1.0 / 3.0).abs() < 1e-9,
            "1 hit over 3 lookups, got {rate}"
        );
        assert_eq!(collector.last_hit_key(), Some("k1"));
        assert_eq!(collector.last_miss_key(), Some("k2"));
        assert_eq!(collector.last_store_key(), Some("k2"));
        assert_eq!(collector.last_evicted_key(), Some("k1"));
    }

    #[test]
    fn hit_rate_is_none_without_lookups() {
        let collector = CacheCollector::new();
        assert_eq!(collector.hit_rate(), None);
        assert_eq!(collector.lookups(), 0);
        assert_eq!(collector.hits(), 0);
        assert_eq!(collector.misses(), 0);
        assert!(collector.last_hit_key().is_none());
    }

    #[test]
    fn from_events_refold_matches_incremental_fold() {
        let incremental = {
            let mut collector = CacheCollector::new();
            for event in event_list() {
                collector.event(&event);
            }
            collector
        };
        let refolded = CacheCollector::from_events(event_list());
        assert_eq!(refolded, incremental, "re-fold must reproduce the fold");
    }

    #[test]
    fn render_is_deterministic_and_explicit_about_the_empty_state() {
        let empty = render_cache_report(&CacheCollector::new());
        assert!(
            empty.contains("no cache events recorded"),
            "empty aggregate must be spelled, got {empty:?}"
        );

        let collector = CacheCollector::from_events(event_list());
        let first = render_cache_report(&collector);
        let second = render_cache_report(&collector);
        assert_eq!(first, second, "two renders must be byte-identical");
        for needle in [
            "3 lookups",
            "hits 1",
            "misses 2",
            "33.3%",
            "stores: 2",
            "evictions: 1",
            "bytes served: 128",
            "stored: 192",
            "evicted: 128",
            "last hit key: k1",
            "last evicted key: k1",
        ] {
            assert!(first.contains(needle), "report {first:?} lacks {needle:?}");
        }
    }

    #[test]
    fn render_spells_na_rate_without_lookups_but_with_stores() {
        // Stores without a lookup in this process (e.g. a pure writer):
        // not the empty state, and no hit rate to average.
        let collector = CacheCollector::from_events([CacheEvent::Stored {
            key: "k1".to_string(),
            size_bytes: 8,
        }]);
        let rendered = render_cache_report(&collector);
        assert!(rendered.contains("0 lookups"), "{rendered:?}");
        assert!(rendered.contains("n/a (no lookups)"), "{rendered:?}");
        assert!(rendered.contains("last store key: k1"), "{rendered:?}");
    }

    #[test]
    fn sink_observes_miss_store_and_hit_in_order() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let mut cache = CompileCache::open(dir.path())
            .with_max_bytes(Some(4096))
            .with_sink(Box::new(Recorder(Vec::new())));
        // A fresh cache misses, a store becomes visible, the next lookup
        // hits — every transition observed, in order, by the attached sink.
        assert!(cache.lookup("k").is_none(), "fresh cache must miss");
        let object = dir.path().join("object.bin");
        std::fs::write(&object, b"OBJECT-BYTES").expect("write object");
        cache.store("k", &object);
        assert!(cache.lookup("k").is_some(), "stored object must be visible");

        let mut sink = cache.take_sink().expect("sink still attached");
        let recorder = sink
            .as_any()
            .downcast_ref::<Recorder>()
            .expect("the attached sink was the recorder");
        assert_eq!(
            recorder.0,
            vec![
                CacheEvent::Miss {
                    key: "k".to_string()
                },
                CacheEvent::Stored {
                    key: "k".to_string(),
                    size_bytes: 12, // b"OBJECT-BYTES".len()
                },
                CacheEvent::Hit {
                    key: "k".to_string(),
                    size_bytes: 12,
                },
            ],
            "every transition observed exactly once, in order"
        );
        assert!(cache.take_sink().is_none(), "take empties the slot");
    }

    #[test]
    fn sink_observes_evictions_when_over_budget() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let mut cache = CompileCache::open(dir.path())
            .with_max_bytes(Some(2048))
            .with_sink(Box::new(CacheCollector::new()));
        // Six 1 KiB objects into a 2 KiB budget: each store trims
        // oldest-first, so the newest two survive and four evictions are
        // observed (the same shape cache.rs's own eviction test drives).
        for i in 0..6 {
            let src = dir.path().join(format!("src{i}.bin"));
            std::fs::write(&src, vec![b'x'; 1024]).expect("write object");
            cache.store(&format!("key{i}"), &src);
            // Distinct mtimes so oldest-first is deterministic on
            // filesystems with 1 s mtime granularity.
            std::thread::sleep(Duration::from_millis(20));
        }

        let mut sink = cache.take_sink().expect("sink still attached");
        let collector = sink
            .as_any()
            .downcast_ref::<CacheCollector>()
            .expect("the attached sink was the collector");
        assert_eq!(collector.stores(), 6, "every store observed");
        assert_eq!(
            collector.evictions(),
            4,
            "6 KiB trimmed to 2 KiB removes exactly four 1 KiB objects"
        );
        assert_eq!(
            collector.bytes_evicted(),
            4096,
            "the eviction bytes equal the removed files' sizes"
        );
        assert_eq!(collector.last_evicted_key(), Some("key3"));
        // The fold agrees with the directory: the newest survived, the
        // oldest is gone.
        assert!(cache.lookup("key5").is_some(), "newest must survive");
        assert!(cache.lookup("key0").is_none(), "oldest must be evicted");
    }

    #[test]
    fn failed_store_reports_nothing() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let mut cache = CompileCache::open(dir.path())
            .with_max_bytes(Some(4096))
            .with_sink(Box::new(Recorder(Vec::new())));
        // A missing source file: the copy fails, nothing is stored, and the
        // total contract says a failed store reports nothing.
        cache.store("k", &dir.path().join("missing.bin"));
        let mut sink = cache.take_sink().expect("sink still attached");
        let recorder = sink
            .as_any()
            .downcast_ref::<Recorder>()
            .expect("the attached sink was the recorder");
        assert!(
            recorder.0.is_empty(),
            "a failed store must report nothing, got {:?}",
            recorder.0
        );
    }

    #[test]
    fn without_sink_the_cache_still_serves_and_stores() {
        // The zero-cost contract: no sink attached — the cache behaves
        // exactly as before the seam existed.
        let dir = tempfile::tempdir().expect("scratch dir");
        let mut cache = CompileCache::open(dir.path());
        assert!(cache.lookup("k").is_none());
        let object = dir.path().join("object.bin");
        std::fs::write(&object, b"OBJECT-BYTES").expect("write object");
        cache.store("k", &object);
        assert!(cache.lookup("k").is_some());
        assert!(cache.take_sink().is_none(), "no sink was ever attached");
    }
}
