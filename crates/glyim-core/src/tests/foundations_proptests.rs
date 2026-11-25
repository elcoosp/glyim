//! Property tests for the glyim-core foundations (plan §6 M3.10).
//!
//! Plan acceptance line (M3.10): *"Property tests for foundations:
//! `IndexVec`, interner, spans, `Size/Align`, substitution, unification"*
//! with the `DoD` `≥ 200 properties; run in PR tier`. The span half of that
//! line is pinned in `glyim-span::tests::checked_span_tests` (M3.2); this
//! module pins the two glyim-core foundations — the typed-index arena
//! ([`IndexVec`]/[`Idx`]/`define_idx!`) and the string interner — with
//! properties that must hold for every generated input, not just the
//! hand-picked cases in `idx.rs`/`interning.rs`.
//!
//! Everything here runs in the default `cargo test` PR tier: no feature
//! flags, no nightly-only gates (AGENTS.md rule 9), no LLVM dependency —
//! glyim-core intentionally has no dev-dependency that reaches the LLVM
//! toolchain (see this crate's `Cargo.toml`).

use crate::arena::{Idx, IndexVec};
use crate::define_idx;
use crate::interner::Interner;
use proptest::prelude::*;

// A locally-defined `define_idx!` index type: the macro-generated
// accessors are part of the M3.10 foundation surface and are exercised
// exactly the way downstream crates consume them.
define_idx!(PropIdx);

proptest! {
    // ------------------------------------------------------------------
    // Idx: the typed handle is a faithful, order-preserving wrapper of
    // its raw value (the arena's addressing contract).
    // ------------------------------------------------------------------

    /// `Idx::from_raw` after `to_raw` is the identity, and `index()` is
    /// exactly the raw value widened to `usize` (no narrowing surprises
    /// inside the handle space).
    #[test]
    fn idx_raw_round_trip(raw in any::<u32>()) {
        let idx = Idx::<u8>::from_raw(raw);
        prop_assert_eq!(idx.to_raw(), raw);
        prop_assert_eq!(idx.index(), raw as usize);
        let macro_idx = PropIdx::from_raw(raw);
        prop_assert_eq!(macro_idx.to_raw(), raw);
        prop_assert_eq!(macro_idx.index(), raw as usize);
    }

    /// Handle equality and ordering are exactly raw-value equality and
    /// ordering — the arena can rely on `Idx` sorts being index sorts.
    #[test]
    fn idx_ordering_matches_raw(a in any::<u32>(), b in any::<u32>()) {
        let (a, b) = (Idx::<u8>::from_raw(a), Idx::<u8>::from_raw(b));
        prop_assert_eq!(a == b, a.to_raw() == b.to_raw());
        prop_assert_eq!(a.cmp(&b), a.to_raw().cmp(&b.to_raw()));
        let (ma, mb) = (PropIdx::from_raw(a.to_raw()), PropIdx::from_raw(b.to_raw()));
        prop_assert_eq!(ma == mb, ma.to_raw() == mb.to_raw());
        prop_assert_eq!(ma.cmp(&mb), ma.to_raw().cmp(&mb.to_raw()));
    }

    /// Equal handles hash equally (the `Hash`/`Eq` contract every
    /// `HashMap<Idx, _>` in the compiler relies on).
    #[test]
    fn idx_hash_follows_eq(a in any::<u32>(), b in any::<u32>()) {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let hash = |i: Idx<u8>| {
            let mut h = DefaultHasher::new();
            i.hash(&mut h);
            h.finish()
        };
        let (a, b) = (Idx::<u8>::from_raw(a), Idx::<u8>::from_raw(b));
        if a == b {
            prop_assert_eq!(hash(a), hash(b));
        }
    }

    /// Distinct `define_idx!` types with the same raw value are distinct
    /// handles (the phantom-type safety the macro exists for): comparing
    /// them only through their raw projections, never across types.
    #[test]
    fn idx_types_do_not_share_identity(raw in any::<u32>()) {
        let generic = Idx::<u8>::from_raw(raw);
        let macro_typed = PropIdx::from_raw(raw);
        // Same raw projection...
        prop_assert_eq!(generic.to_raw(), macro_typed.to_raw());
        // ...but each type's own equality is closed over itself.
        prop_assert_eq!(generic, Idx::<u8>::from_raw(raw));
        prop_assert_eq!(macro_typed, PropIdx::from_raw(raw));
    }

    // ------------------------------------------------------------------
    // IndexVec: push/get/iter are a faithful arena over insertion order.
    // ------------------------------------------------------------------

    /// Push returns the sequential indices `0..n`, `len` tracks the push
    /// count, and every pushed value reads back through the returned
    /// handle — the arena round-trip all compiler arenas rest on.
    #[test]
    fn push_returns_sequential_indices(vals in proptest::collection::vec(any::<u32>(), 0..64)) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::new();
        let mut handles = Vec::with_capacity(vals.len());
        for val in &vals {
            let before = v.len();
            let idx = v.push(*val);
            prop_assert_eq!(idx.index(), before);
            handles.push(idx);
        }
        prop_assert_eq!(v.len(), vals.len());
        for (idx, val) in handles.iter().zip(&vals) {
            prop_assert_eq!(v.get(*idx), Some(val));
        }
    }

    /// `iter` and `iter_enumerated` reproduce insertion order with
    /// sequential handles (deterministic iteration — constitution C6).
    #[test]
    fn iteration_is_insertion_order(vals in proptest::collection::vec(any::<u32>(), 0..64)) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::new();
        for val in &vals {
            v.push(*val);
        }
        let collected: Vec<u32> = v.iter().copied().collect();
        prop_assert_eq!(collected, vals.clone());
        for (pos, (idx, val)) in v.iter_enumerated().enumerate() {
            prop_assert_eq!(idx.index(), pos);
            prop_assert_eq!(*val, vals[pos]);
        }
    }

    /// `into_iter_enumerated` yields the same (index, value) sequence as
    /// the borrowed form; `into_raw` inverts `from_raw`.
    #[test]
    fn into_iter_and_into_raw_round_trip(vals in proptest::collection::vec(any::<u32>(), 0..64)) {
        let v: IndexVec<Idx<u8>, u32> = IndexVec::from_raw(vals.clone());
        for (pos, (idx, val)) in v.into_iter_enumerated().enumerate() {
            prop_assert_eq!(idx.index(), pos);
            prop_assert_eq!(val, vals[pos]);
        }
        let rebuilt = IndexVec::<Idx<u8>, u32>::from_raw(vals.clone());
        prop_assert_eq!(rebuilt.into_raw(), vals);
    }

    /// `get_mut` writes read back through `get`, and untouched slots keep
    /// their values (the mutation path the passes take).
    #[test]
    fn get_mut_overwrites_only_the_target(
        vals in proptest::collection::vec(any::<u32>(), 0..64),
        new_val in any::<u32>(),
    ) {
        prop_assume!(!vals.is_empty(), "need a slot to write");
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::from_raw(vals.clone());
        let target = Idx::<u8>::from_raw(0);
        *v.get_mut(target).expect("slot 0 exists") = new_val;
        prop_assert_eq!(v.get(target), Some(&new_val));
        for (pos, val) in vals.iter().enumerate().skip(1) {
            prop_assert_eq!(v.get(Idx::<u8>::from_raw(pos as u32)), Some(val));
        }
    }

    /// `last` is the most recently pushed value; `len`/`is_empty` agree;
    /// `reserve` never changes the logical contents.
    #[test]
    fn last_len_is_empty_and_reserve(vals in proptest::collection::vec(any::<u32>(), 0..64), extra in 0_usize..64) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::new();
        prop_assert!(v.is_empty());
        for val in &vals {
            v.push(*val);
        }
        prop_assert_eq!(v.is_empty(), vals.is_empty());
        prop_assert_eq!(v.last(), vals.last());
        v.reserve(extra);
        prop_assert_eq!(v.len(), vals.len());
        prop_assert_eq!(v.as_slice(), &vals[..]);
    }

    /// `with_capacity` starts logically empty and subsequent pushes behave
    /// exactly like the default-constructed arena.
    #[test]
    fn with_capacity_push_equivalence(vals in proptest::collection::vec(any::<u32>(), 0..64), cap in 0_usize..64) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::with_capacity(cap);
        prop_assert!(v.is_empty());
        let expected: IndexVec<Idx<u8>, u32> = IndexVec::from_raw(vals.clone());
        for (pos, val) in vals.iter().enumerate() {
            let idx = v.push(*val);
            prop_assert_eq!(idx.index(), pos);
        }
        prop_assert_eq!(v.as_slice(), expected.as_slice());
    }

    /// The `Default` impl is the empty arena (constructors agree).
    #[test]
    fn default_is_empty(cap in 0_usize..8) {
        let default: IndexVec<PropIdx, u32> = IndexVec::default();
        prop_assert!(default.is_empty());
        prop_assert_eq!(default.len(), 0);
        let _ = cap;
    }

    // ------------------------------------------------------------------
    // Interner: string interning is a faithful, deduplicating,
    // content-addressed map (the symbol identity contract).
    // ------------------------------------------------------------------

    /// `intern` then `resolve` returns the exact original string — for
    /// arbitrary text, including unicode, empties and control chars.
    #[test]
    fn intern_resolve_round_trip(s in ".*") {
        let interner = Interner::new();
        let name = interner.intern(&s);
        prop_assert_eq!(interner.resolve(name), s.as_str());
    }

    /// Interning the same string twice yields the same handle (the
    /// identity property every `Name`-keyed map relies on).
    #[test]
    fn intern_is_idempotent(a in ".*", b in ".*") {
        let interner = Interner::new();
        let first = interner.intern(&a);
        let second = interner.intern(&a);
        prop_assert_eq!(first, second);
        let other = interner.intern(&b);
        prop_assert_eq!(other == first, a == b);
    }

    /// `len` grows exactly once per *distinct* string: interning a
    /// duplicate never inflates the count (dedup contract, M6 cache
    /// sizing input).
    #[test]
    fn len_counts_distinct_strings(
        s in ".*",
        dup_count in 0_usize..5,
    ) {
        let interner = Interner::new();
        prop_assert!(interner.is_empty());
        let _first = interner.intern(&s);
        prop_assert_eq!(interner.len(), 1);
        for _ in 0..dup_count {
            let _dup = interner.intern(&s);
        }
        prop_assert_eq!(interner.len(), 1);
        let distinct = format!("{s}#suffix");
        let _distinct = interner.intern(&distinct);
        prop_assert_eq!(interner.len(), 2);
    }

    /// `lookup` is the read-only face of `intern`: it finds exactly the
    /// previously interned strings and nothing else (never mints).
    #[test]
    fn lookup_never_mints(known in "[a-z]{1,12}", unknown in "[A-Z]{13,24}") {
        let interner = Interner::new();
        let name = interner.intern(&known);
        prop_assert_eq!(interner.lookup(&known), Some(name));
        prop_assume!(known != unknown, "need a genuinely absent string");
        prop_assert_eq!(interner.lookup(&unknown), None);
        prop_assert_eq!(interner.len(), 1, "lookup must not intern");
    }

    /// A cloned interner shares the symbol space: handles minted on the
    /// original resolve through the clone and vice versa (the `Arc`
    /// sharing contract).
    #[test]
    fn clone_shares_symbol_space(s in ".*", t in ".*") {
        let interner = Interner::new();
        let a = interner.intern(&s);
        let clone = interner.clone();
        prop_assert_eq!(clone.resolve(a), s.as_str());
        let b = clone.intern(&t);
        prop_assert_eq!(interner.resolve(b), t.as_str());
        prop_assert_eq!(interner.len(), clone.len());
        prop_assert_eq!(a == b, s == t);
    }

    /// `Default` produces a fresh empty interner; `Debug` formatting is
    /// total for any interned content (the `finish_non_exhaustive` shape
    /// never resolves strings into logs — pinned as a totality property).
    #[test]
    fn default_and_debug_totality(s in ".*") {
        let interner = Interner::default();
        prop_assert!(interner.is_empty());
        let _name = interner.intern(&s);
        let rendered = format!("{interner:?}");
        prop_assert!(
            rendered.starts_with("Interner"),
            "debug is the non-exhaustive struct form, got: {rendered}"
        );
        prop_assert_eq!(interner.resolve(interner.intern(&s)), s.as_str());
    }
}

proptest! {
    /// `as_mut_slice` writes read back through `iter` in the same order —
    /// the escape hatch passes use to batch-edit arena contents.
    #[test]
    fn mut_slice_writes_read_back(
        vals in proptest::collection::vec(any::<u32>(), 1..32),
        new_val in any::<u32>(),
    ) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::from_raw(vals);
        for slot in v.as_mut_slice() {
            *slot = new_val;
        }
        for val in v.iter() {
            prop_assert_eq!(*val, new_val);
        }
        prop_assert_eq!(v.as_slice().first(), Some(&new_val));
    }

    /// Interning a stream of strings (duplicates included) then
    /// re-interning every one of them keeps `len` at the distinct count
    /// and every handle stable — first-wins interning at scale.
    #[test]
    fn first_wins_across_many_distincts(
        strs in proptest::collection::vec("[a-z]{1,8}", 1..12),
    ) {
        let interner = Interner::new();
        let mut handles = Vec::new();
        for s in &strs {
            handles.push(interner.intern(s));
        }
        let distinct: std::collections::HashSet<&str> =
            strs.iter().map(String::as_str).collect();
        prop_assert_eq!(interner.len(), distinct.len());
        // Re-interning in the same order reproduces the same handles.
        for (s, h) in strs.iter().zip(&handles) {
            prop_assert_eq!(interner.lookup(s), Some(*h));
            prop_assert_eq!(interner.resolve(*h), s.as_str());
            let again = interner.intern(s);
            prop_assert_eq!(again, *h, "re-intern is the identity");
        }
    }

    /// `Idx` handles key a `HashMap` faithfully: same raw -> same entry,
    /// different raw -> different entry (the `Eq`/`Hash` pair in action).
    #[test]
    fn idx_hashes_as_map_keys(a in any::<u32>(), b in any::<u32>()) {
        use std::collections::HashMap;
        let mut map: HashMap<Idx<u8>, u32> = HashMap::new();
        map.insert(Idx::<u8>::from_raw(a), 1);
        map.insert(Idx::<u8>::from_raw(b), 2);
        let expected_len = if a == b { 1 } else { 2 };
        prop_assert_eq!(map.len(), expected_len);
        prop_assert_eq!(map.get(&Idx::<u8>::from_raw(a)), Some(&if a == b { 2 } else { 1 }));
    }

    /// `iter_enumerated` handles stay valid after `get_mut` edits: the
    /// index space is dense `0..len` regardless of contents.
    #[test]
    fn enumerated_handles_survive_edits(
        vals in proptest::collection::vec(any::<u32>(), 0..16),
        new_val in any::<u32>(),
    ) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::from_raw(vals);
        let indices: Vec<Idx<u8>> = v.iter_enumerated().map(|(idx, _)| idx).collect();
        for idx in &indices {
            prop_assert!(idx.index() < v.len());
            if let Some(slot) = v.get_mut(*idx) {
                *slot = new_val;
            }
        }
        prop_assert!(v.iter().all(|val| *val == new_val));
    }
}

proptest! {
    /// Push handles are strictly increasing — the index order is the
    /// insertion order, so `Idx` comparisons double as insertion order.
    #[test]
    fn push_handles_strictly_increase(vals in proptest::collection::vec(any::<u32>(), 0..64)) {
        let mut v: IndexVec<Idx<u8>, u32> = IndexVec::new();
        let mut prev: Option<Idx<u8>> = None;
        for val in &vals {
            let idx = v.push(*val);
            if let Some(prev) = prev {
                prop_assert!(idx > prev, "handles must strictly increase");
            }
            prev = Some(idx);
        }
    }
}
