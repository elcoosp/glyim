//! M6.1 stable-identity property tests: editing an unrelated body never
//! changes another item's [`glyim_core::DefPathHash`].
//!
//! The model is a tiny item tree: each item has a path (the sole input to
//! `DefPathHash`) and a body hash (stand-in for body content). A seeded RNG
//! drives ≥100 random edit sequences — body edits, item reordering, and
//! unrelated-body edits — and the property asserts that an item's
//! `DefPathHash` changes iff its own path changed. Body content, declaration
//! order, and file order never move the hash.

use glyim_core::{DefPathHash, StableHasher};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// A model item: a path (segments from the crate root) and a body hash
/// (stand-in for the body's content). Only the path feeds `DefPathHash`.
struct ModelItem {
    path: Vec<String>,
    body_hash: u64,
}

impl ModelItem {
    fn def_path_hash(&self, crate_name: &str) -> DefPathHash {
        let segs: Vec<&str> = self.path.iter().map(String::as_str).collect();
        DefPathHash::from_path(crate_name, &segs)
    }
}

/// Build a 10-item tree under `crate_name` with distinct paths.
fn ten_item_tree() -> Vec<ModelItem> {
    (0..10)
        .map(|i| ModelItem {
            path: vec!["mod".to_string(), format!("fn{i}")],
            body_hash: u64::try_from(i).unwrap_or(0),
        })
        .collect()
}

/// M6.1 `DoD`: editing an unrelated body never changes another item's
/// `DefPathHash`. Over 100 seeded random edits, only the item whose path
/// changed (if any) may see its hash move; every other item's hash is
/// byte-identical before and after.
#[test]
fn property_def_path_hash_invariant_under_unrelated_body_edits() {
    const N: usize = 100;
    let mut rng = StdRng::seed_from_u64(0xC0DE_BEEF);
    let crate_name = "glyim";
    let mut tree = ten_item_tree();

    // Snapshot the initial hashes.
    let baseline: Vec<DefPathHash> = tree.iter().map(|i| i.def_path_hash(crate_name)).collect();

    for _ in 0..N {
        // Edit a random item's body (never its path).
        let idx = rng.random_range(0..tree.len());
        tree[idx].body_hash = rng.random_range(0..1_000_000);

        // Every item's DefPathHash is unchanged: body content is not an input.
        for (i, item) in tree.iter().enumerate() {
            assert_eq!(
                item.def_path_hash(crate_name),
                baseline[i],
                "body edit on item {idx} changed item {i}'s DefPathHash"
            );
        }
    }
}

/// M6.1 `DoD` (order independence): reordering the item list — which under the
/// old `LocalDefId` numbering would renumber every downstream item — leaves
/// every `DefPathHash` byte-identical, because the hash is path-based, not
/// position-based.
#[test]
fn property_def_path_hash_invariant_under_item_reordering() {
    let mut rng = StdRng::seed_from_u64(0x1234_5678);
    let crate_name = "glyim";
    let tree = ten_item_tree();
    let baseline: Vec<DefPathHash> = tree.iter().map(|i| i.def_path_hash(crate_name)).collect();

    // Reorder by reversing, then by a Fisher-Yates-ish shuffle of indices.
    let mut order: Vec<usize> = (0..tree.len()).collect();
    for i in 0..order.len() {
        let j = rng.random_range(0..order.len());
        order.swap(i, j);
    }
    let shuffled: Vec<&ModelItem> = order.iter().map(|&i| &tree[i]).collect();
    for (pos, item) in shuffled.iter().enumerate() {
        let original_idx = order[pos];
        assert_eq!(
            item.def_path_hash(crate_name),
            baseline[original_idx],
            "reordering changed a DefPathHash (position leaked into the hash)"
        );
    }
}

/// Positive control: renaming an item (changing its path) DOES change its
/// `DefPathHash`, and only its own — every other item's hash is stable. This
/// guards against the property tests above being vacuous (a hash that never
/// changed at all would pass them too).
#[test]
fn def_path_hash_changes_only_for_the_renamed_item() {
    let crate_name = "glyim";
    let mut tree = ten_item_tree();
    let baseline: Vec<DefPathHash> = tree.iter().map(|i| i.def_path_hash(crate_name)).collect();

    // Rename item 3's path.
    tree[3].path = vec!["mod".to_string(), "renamed".to_string()];

    let after: Vec<DefPathHash> = tree.iter().map(|i| i.def_path_hash(crate_name)).collect();
    assert_ne!(after[3], baseline[3], "renaming must change the hash");
    for (i, (a, b)) in after.iter().zip(baseline.iter()).enumerate() {
        if i != 3 {
            assert_eq!(*a, *b, "rename of item 3 leaked into item {i}'s hash");
        }
    }
}

/// `StableHasher` is deterministic across separate instances (no per-process
/// seed), so query caches keyed by its output are reproducible. Two independent
/// hashers over the same bytes produce identical digests.
#[test]
fn stable_hasher_is_unseeded_and_reproducible() {
    let mut a = StableHasher::new();
    let mut b = StableHasher::new();
    for s in &["glyim", "std", "vec", "Vec", "push"] {
        a.write_str(s);
        b.write_str(s);
    }
    assert_eq!(a.finish(), b.finish());
}

/// M6.3 `DoD`: a body-only edit leaves the item-tree fingerprint unchanged.
/// The `Database` impl of `QueryContext::item_tree_fingerprint` hashes a
/// body-agnostic signature summary (each signature cut at the first `{`), so
/// editing a function's body moves neither the signature lines nor their
/// order, and the fingerprint is byte-identical. A signature change (renaming
/// an item) does move it — the positive control that guards the property
/// against being vacuous.
#[test]
fn property_item_tree_fingerprint_invariant_under_body_only_edit() {
    use glyim_db::{CrateConfig, Database, QueryContext};
    use std::path::Path;
    use std::sync::Arc;

    let db = Database::new(CrateConfig {
        name: "app".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 2,
    });
    let path = Path::new("src.g");
    let v1 = "fn foo() -> i32 { 1 }\nfn bar() -> i32 { 2 }\n";
    let file = db
        .vfs()
        .add_file_content(path, Arc::<str>::from(v1))
        .expect("fixture sources are far below the VFS length gate");
    let fp0 = db.item_tree_fingerprint(file);

    // Body-only edit: change `bar`'s body, not its signature.
    let v2 = "fn foo() -> i32 { 1 }\nfn bar() -> i32 { 999 }\n";
    db.vfs()
        .set_file_content(file, Arc::<str>::from(v2))
        .expect("set_file_content");
    let fp1 = db.item_tree_fingerprint(file);
    assert_eq!(
        fp0, fp1,
        "a body-only edit must leave the item-tree fingerprint unchanged"
    );

    // Signature change: rename `bar` → `baz` (moves the fingerprint).
    let v3 = "fn foo() -> i32 { 1 }\nfn baz() -> i32 { 999 }\n";
    db.vfs()
        .set_file_content(file, Arc::<str>::from(v3))
        .expect("set_file_content");
    let fp2 = db.item_tree_fingerprint(file);
    assert_ne!(
        fp0, fp2,
        "a signature change must move the item-tree fingerprint"
    );
}

/// M6.4 `DoD` (c): editing a function body does not invalidate the `def_map`
/// query or `item_tree_fingerprint`. Over 50 seeded random body-only edits,
/// the `def_map` query returns a pointer-identical `Arc<CrateDefMap>` (cache
/// hit via the body-agnostic `item_tree_fingerprint` key) and the fingerprint
/// is byte-identical before and after. A signature change (renaming an item)
/// moves the fingerprint and cache-misses — the positive control that guards
/// the property against being vacuous.
///
/// The `def_map` query lives on `Database` via the `BodyQueries` trait
/// (`glyim-pipeline::body_queries`); its cache key is
/// `QueryContext::item_tree_fingerprint` (M6.3), so a body-only edit is a
/// cache hit: the `compute` closure (which would `panic!` on recompute) is
/// never called.
#[test]
fn property_def_map_query_invariant_under_body_only_edits() {
    use glyim_db::{CrateConfig, Database, QueryContext};
    use glyim_def_map::build_def_map;
    use glyim_frontend::parse_to_syntax;
    use glyim_meta::Expander;
    use glyim_pipeline::body_queries::BodyQueries;
    use glyim_span::HygieneCtx;
    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};
    use std::path::Path;
    use std::sync::Arc as StdArc;

    const N: usize = 50;
    let mut rng = StdRng::seed_from_u64(0x6D64_0000);
    let db = Database::new(CrateConfig {
        name: "app".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 2,
    });
    let fns = ["foo", "bar", "baz", "qux", "quux"];
    // Build the initial source: five `fn <name>() -> i32 { 0 }` items.
    let mk_source = |bodies: &[u64]| -> StdArc<str> {
        use std::fmt::Write;
        let mut s = String::new();
        for (n, b) in fns.iter().zip(bodies.iter()) {
            writeln!(s, "fn {n}() -> i32 {{ {b} }}").expect("write into String is infallible");
        }
        StdArc::from(s)
    };
    let v1 = mk_source(&[0; 5]);
    let path = Path::new("src.g");
    let file = db
        .vfs()
        .add_file_content(path, v1.clone())
        .expect("fixture sources are far below the VFS length gate");

    // Build a real CrateDefMap (parse + expand + build_def_map). The compute
    // closure runs only on a cache miss; on a body-only-edit cache hit it is
    // never called (the panic closure passed in the loop guards that).
    let build_def_map_for = |src: &str| {
        let parse = parse_to_syntax(src, file);
        let mut hygiene = HygieneCtx::new();
        let mut expander = Expander::new(&mut hygiene);
        expander.set_source_file(file);
        expander.set_vfs(db.vfs());
        let (expanded, _) = expander.expand_crate(&parse.root);
        build_def_map(&expanded, db.krate(), db.interner().clone()).0
    };

    // Initial query: cache miss → compute → cache → Arc A.
    let arc1 = db.def_map_query(file, || build_def_map_for(&v1));
    let fp1 = db.item_tree_fingerprint(file);

    for _ in 0..N {
        // Pick a random function, edit its body only (signature unchanged).
        let idx = rng.random_range(0..fns.len());
        let new_body = rng.random_range(0..1_000_000);
        let mut bodies = [0u64; 5];
        bodies[idx] = new_body;
        let edited = mk_source(&bodies);
        db.vfs()
            .set_file_content(file, edited.clone())
            .expect("set content");

        // def_map_query MUST cache-hit: the body-only edit left
        // item_tree_fingerprint unchanged, so the compute closure (which
        // panics) is never called and the same Arc is returned.
        let arc_n = db.def_map_query(file, || panic!("body-only edit must cache-hit"));
        assert!(
            StdArc::ptr_eq(&arc1, &arc_n),
            "def_map invalidated by a body-only edit on {}",
            fns[idx]
        );
        let fp_n = db.item_tree_fingerprint(file);
        assert_eq!(
            fp1, fp_n,
            "item_tree_fingerprint moved by a body-only edit on {}",
            fns[idx]
        );
    }

    // Positive control: a signature change (rename `foo` → `renamed`) moves the
    // fingerprint → cache miss → compute runs → fresh Arc.
    let v_rename: StdArc<str> = {
        use std::fmt::Write;
        let mut s = String::new();
        for n in fns.iter() {
            if *n == "foo" {
                writeln!(s, "fn renamed() -> i32 {{ 0 }}")
                    .expect("write into String is infallible");
            } else {
                writeln!(s, "fn {n}() -> i32 {{ 0 }}").expect("write into String is infallible");
            }
        }
        StdArc::from(s)
    };
    db.vfs()
        .set_file_content(file, v_rename.clone())
        .expect("set content");
    let arc_r = db.def_map_query(file, || build_def_map_for(&v_rename));
    assert!(
        !StdArc::ptr_eq(&arc1, &arc_r),
        "a signature change must cache-miss (fresh Arc)"
    );
    let fp_r = db.item_tree_fingerprint(file);
    assert_ne!(
        fp1, fp_r,
        "a signature change must move item_tree_fingerprint"
    );
}
