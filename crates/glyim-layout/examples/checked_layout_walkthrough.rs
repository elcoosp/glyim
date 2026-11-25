//! Checked-layout walkthrough: the CT-3 layout vocabulary the ABI
//! decisions ride on.
//!
//! `glyim-layout` had no consumer walkthrough so far: its newtypes
//! (`Size`, `Align`, `Offset`) are the checked-arithmetic layer between
//! the type system and every ABI/layout decision the backends make, and
//! nothing shows a library consumer how the pieces compose. This
//! walkthrough builds the shapes a layout author actually makes: a
//! struct's field cursor (`Offset::checked_add` + `checked_align_to`),
//! the power-of-two `Align` lattice, an array's checked stride math
//! (`Size::checked_mul`), and the overflow refusals that replaced the
//! old silent wrap/saturate masks (plan M3.2 / CT-3; the property suite
//! pinning these laws lives in `src/tests/checked_layout_proptests.rs`,
//! plan M3.10). Run with:
//!
//! ```text
//! cargo run -p glyim-layout --example checked_layout_walkthrough
//! ```

use glyim_layout::{Align, Offset, Size};

fn main() {
    // A scalar field: i32, 4 bytes, 4-aligned. Alignments are minted
    // through `Align::from_bytes`, which refuses non-power-of-two values
    // with `None` — an illegal alignment is unrepresentable.
    let scalar = LayoutSample {
        size: Size::bytes(4),
        align: Align::from_bytes(4).expect("4 is a power of two"),
    };
    println!(
        "scalar field: size={} align={}",
        scalar.size.as_u64(),
        scalar.align.as_u64()
    );

    // The field cursor: field 0 at byte 0, field 1 advanced by the first
    // field's size, then padded up to its alignment. Every step is
    // `checked_*`: overflow is a `None`, never a wrap (the old mask
    // arithmetic silently wrapped past u64::MAX).
    let mut cursor = Offset::ZERO;
    let mut layout = Vec::new();
    for (name, field) in [("a", &scalar), ("b", &scalar)] {
        cursor = cursor
            .checked_align_to(field.align)
            .expect("cursor padding fits u64")
            .checked_add(field.size)
            .expect("field advance fits u64");
        layout.push((name, cursor.as_u64()));
    }
    println!("field cursors: {layout:?}");

    // An array: element count times element size, checked. The `MAX`
    // sample shows the refusal arm: the product past `u64::MAX` returns
    // `None` instead of a wrapped (wrong) total size.
    let count = 1_000_u64;
    let total = scalar.size.checked_mul(count).expect("array total fits");
    let absurd = Size::MAX.checked_mul(count);
    println!(
        "array[{count}]: total={} bytes; Size::MAX * {count} = {:?}",
        total.as_u64(),
        absurd.map(Size::as_u64)
    );

    // The `Align` lattice: the struct alignment is the strictest field
    // alignment — `Align::max`, commutative and idempotent (pinned by
    // proptests).
    let wider = Align::from_bytes(16).expect("16 is a power of two");
    let struct_align = scalar.align.max(wider);
    println!(
        "struct align: max({}, {}) = {}",
        scalar.align.as_u64(),
        wider.as_u64(),
        struct_align.as_u64()
    );

    // Total size: the cursor rounded up to the struct alignment — the
    // same reference formula the proptests differential-check against
    // plain u64 arithmetic.
    let total_size = cursor
        .to_size()
        .checked_align_to(struct_align)
        .expect("final padding fits");
    println!(
        "final: cursor={} -> aligned size={} bytes",
        cursor.as_u64(),
        total_size.as_u64()
    );
}

/// One field's layout facts (size + alignment), the input the cursor
/// walk consumes.
struct LayoutSample {
    /// The field's byte size.
    size: Size,
    /// The field's power-of-two alignment.
    align: Align,
}
