//! Consumer walkthrough of the content-addressed compile cache: the
//! seam that makes a rebuild cheap (`glyip build` and the compiler driver
//! sit on it).
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-db --example compile_cache_walkthrough
//! ```
//!
//! The example opens a scratch cache (tempdir), computes a key for a
//! source/target/opt-level tuple, shows a miss, stores a fake object,
//! shows the hit, and demonstrates that *any* input change — source text,
//! target triple, opt level, entry flag — produces a different key (the
//! stale-object guard). It also walks the same key shapes with
//! [`glyim_db::Database`] interning, so the session-state bundle and the
//! cache are visible in one place. The scratch cache is cleaned up when
//! the process exits.

use std::path::Path;

fn main() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let cache = glyim_db::cache::CompileCache::open(scratch.path());

    // 1. Key computation: every input that can change the object is in
    //    the key (flattened source, target, opt level, entry flag, and the
    //    compiler binary's own identity).
    let source = "fn main() -> i32 { 7 }\n";
    let key = glyim_db::cache::CompileCache::key(source, "x86_64-unknown-linux-gnu", 0, true);
    println!("key({source:?}, opt 0, entry) = {key}");

    // 2. Miss: nothing stored yet.
    assert!(cache.lookup(&key).is_none(), "fresh cache misses");
    println!("lookup: MISS (fresh cache)");

    // 3. Store (atomic temp-file + rename) and hit.
    let fake_object = scratch.path().join("fake.o");
    std::fs::write(&fake_object, b"FAKE-OBJ").expect("write fake object");
    cache.store(&key, &fake_object);
    let hit = cache.lookup(&key).expect("stored object hits");
    assert_eq!(hit, cache.object_path(&key));
    println!("lookup: HIT at {}", hit.display());

    // 4. Stale-object guard: changing any input changes the key.
    let edited = glyim_db::cache::CompileCache::key(
        "fn main() -> i32 { 8 }\n",
        "x86_64-unknown-linux-gnu",
        0,
        true,
    );
    let retargeted =
        glyim_db::cache::CompileCache::key(source, "aarch64-unknown-linux-gnu", 0, true);
    let optimized = glyim_db::cache::CompileCache::key(source, "x86_64-unknown-linux-gnu", 2, true);
    let no_entry = glyim_db::cache::CompileCache::key(source, "x86_64-unknown-linux-gnu", 0, false);
    assert_ne!(key, edited, "source text is in the key");
    assert_ne!(key, retargeted, "target triple is in the key");
    assert_ne!(key, optimized, "opt level is in the key");
    assert_ne!(key, no_entry, "the entry flag is in the key");
    println!("4 key variations (source/target/opt/entry) all differ — no stale hits");

    // 5. The session-state bundle around the cache: one `Database` owns
    //    the interner, the VFS, and the crate config a compile reads from.
    let database = glyim_db::Database::new(glyim_db::CrateConfig {
        name: "walkthrough".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let file_id = database.vfs().add_file_content(
        Path::new("walkthrough/main.g"),
        std::sync::Arc::from(source),
    );
    let _ = file_id;
    println!(
        "database session: crate {:?}, target {:?}, {} file(s) interned",
        database.config().name,
        database.config().target_triple,
        database.vfs().len(),
    );
    assert!(cache.lookup(&key).is_some(), "cache unaffected by sessions");
}
