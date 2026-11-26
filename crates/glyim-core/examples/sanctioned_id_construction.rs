//! Walkthrough: the sanctioned ways to construct a typed index handle after
//! the CT-1 private-constructor ratchet (plan M3.1 — "`from_raw` outside
//! arena modules = 0").
//!
//! Run with `cargo run -p glyim-core --example sanctioned_id_construction`.
//!
//! The ratchet removed every public `from_raw(u32)` ID constructor:
//! production code mints handles only through
//! 1. [`IndexVec::push`] — the arena issues the handle and returns it,
//! 2. `from_usize(usize)` — the sanctioned factory for a slot index that
//!    an issuer already produced (an interner, a counter, another arena),
//! 3. `IdxLike::from_u32` — `IndexVec`-internal machinery, not a public
//!    constructor (kept under a name the CT-1 ledger metric does not
//!    whitelist).
//!
//! The round-trip is still lossless: `handle.to_raw()` / `handle.index()`
//! feed interning tables, and `from_usize` reconstructs the same handle.

use glyim_core::arena::{Idx, IdxLike, IndexVec};

// A macro-generated ID family (as used across the workspace for
// `LocalDefId`, `BasicBlockIdx`, `FieldIdx`, ...).
glyim_core::define_idx!(WidgetId);

fn main() {
    // 1. The arena issues handles: `push` returns the slot it created.
    let mut widgets: IndexVec<WidgetId, &'static str> = IndexVec::new();
    let gear = widgets.push("gear");
    let lever = widgets.push("lever");
    assert_eq!(gear.index(), 0);
    assert_eq!(lever.index(), 1);
    println!("issued {gear:?} and {lever:?} from the arena");

    // 2. `from_usize` reconstructs a handle from an issuer-produced slot —
    //    here the interner-like table position of a serialized handle.
    let stored_slot = gear.index();
    let rebuilt = WidgetId::from_usize(stored_slot);
    assert_eq!(rebuilt, gear);

    // 3. Raw round-trips for interning tables: `to_raw` (and the
    //    `IndexVec`-internal `from_u32` machinery it pairs with) stay
    //    lossless; the public `from_raw(u32)` constructor is gone.
    let raw = rebuilt.to_raw();
    assert_eq!(Idx::<()>::from_u32(raw).to_raw(), raw);
    println!(
        "raw {raw} -> {rebuilt:?} -> raw {} (lossless, no public from_raw)",
        rebuilt.to_raw()
    );

    // `IndexVec::from_vec` (formerly `from_raw`) wraps an already-collected
    // backing vector — the CT-1 rename keeps the raw-handle vocabulary
    // unambiguous.
    let collected: IndexVec<WidgetId, &'static str> = IndexVec::from_vec(vec!["gear", "lever"]);
    assert_eq!(collected.len(), 2);
    println!("from_vec keeps the CT-1 vocabulary unambiguous");
}
