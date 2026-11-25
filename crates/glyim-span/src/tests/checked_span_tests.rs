//! CT-3 checked-span tests (plan §6 M3.2 / §5.2 CT-3).
//!
//! The plan line these tests pin:
//!
//! > `Span { file, lo, len: u32 }` — `lo > hi` is unrepresentable; offsets
//! > > `u32::MAX` produce a diagnostic, not wraparound (FIXES §4 class).
//!
//! and the plan-acceptance half of `M3.2`: "Span invariants proptested; old `debug_assert`
//! class gone". The first test below (`span_stores_lo_and_len...`) is the
//! acceptance criterion: the *stored representation* is `(lo, len)`, so a
//! span whose end precedes its start cannot be written, and the
//! `(lo, len)` constructor rejects a `lo + len` past `u32::MAX` with an
//! error instead of wrapping.

use crate::*;

#[test]
fn span_stores_lo_and_len_backwards_is_unrepresentable() {
    // Plan CT-3: the span's stored shape is `(lo, len)`; every `(lo, len)`
    // pair is a valid span and no stored span can have `end < start`.
    let file = FileId::from_usize(1);
    let lo = ByteIdx::from_usize(5);
    let len = 7_u32;
    let span = Span::new(file, lo, len, SyntaxContext::ROOT).expect("lo + len fits u32::MAX");
    assert_eq!(span.file(), file);
    assert_eq!(span.lo(), lo);
    assert_eq!(span.len(), 7);
    assert_eq!(span.hi(), ByteIdx::from_usize(12));
    assert_eq!(span.byte_range(), 5..12);
    assert_eq!(span.ctx(), SyntaxContext::ROOT);
}

#[test]
fn range_rejects_a_backwards_pair_loudly() {
    // The `(lo, hi)` bridge keeps the invariant loud: `lo > hi` is a
    // compiler-internal bug and fails hard (the old `debug_assert` class —
    // now an unconditional assert, so it cannot vanish in release builds).
    let file = FileId::from_usize(0);
    let lo = ByteIdx::from_usize(10);
    let hi = ByteIdx::from_usize(3);
    let result = std::panic::catch_unwind(|| {
        let _ = Span::range(file, lo, hi, SyntaxContext::ROOT);
    });
    assert!(
        result.is_err(),
        "Span::range(lo > hi) must fail loudly, not produce a span"
    );
}

#[test]
fn checked_len_overflow_is_an_error_not_wraparound() {
    // Plan CT-3: offsets past `u32::MAX` produce an error, never a
    // wrapped span (FIXES §4 class).
    let file = FileId::from_usize(0);
    let lo = ByteIdx::try_from_usize(u32::MAX as usize - 1).expect("u32::MAX - 1 fits");
    let overflow = Span::new(file, lo, 5, SyntaxContext::ROOT);
    assert_eq!(
        overflow,
        Err(SpanOverflow { lo, len: 5 }),
        "lo + len past u32::MAX must be refused, not wrapped"
    );
    // The exact-boundary case is fine: lo + len == u32::MAX.
    let ok = Span::new(file, lo, 1, SyntaxContext::ROOT);
    assert!(ok.is_ok(), "lo + len == u32::MAX is representable");
}

#[test]
fn byteidx_try_from_usize_rejects_past_u32_max() {
    // The checked mint: a `usize` cursor past `u32::MAX` is refused
    // instead of silently narrowing (`from_usize` wraparound class).
    let ok = ByteIdx::try_from_usize(u32::MAX as usize);
    assert_eq!(ok, Ok(ByteIdx::from_usize(u32::MAX as usize)));
    let err = ByteIdx::try_from_usize(u32::MAX as usize + 1);
    assert!(
        err.is_err(),
        "usize past u32::MAX must not wrap into ByteIdx"
    );
}

#[test]
fn at_mints_an_infallible_point_span() {
    let file = FileId::from_usize(2);
    let lo = ByteIdx::from_usize(9);
    let span = Span::at(file, lo, SyntaxContext::ROOT);
    assert_eq!(span.len(), 0);
    assert!(span.is_empty());
    assert_eq!(span.hi(), lo);
}

#[test]
fn merge_and_sans_ctx_preserve_the_representation() {
    let file = FileId::from_usize(1);
    let a = Span::range(
        file,
        ByteIdx::from_usize(2),
        ByteIdx::from_usize(5),
        SyntaxContext::ROOT,
    );
    let b = Span::range(
        file,
        ByteIdx::from_usize(4),
        ByteIdx::from_usize(7),
        SyntaxContext::ROOT,
    );
    let merged = a.to(b);
    assert_eq!(merged.lo(), ByteIdx::from_usize(2));
    assert_eq!(merged.hi(), ByteIdx::from_usize(7));
    let sans = merged.sans_ctx();
    assert_eq!(sans.ctx(), SyntaxContext::ROOT);
    assert_eq!(sans.lo(), merged.lo());
    assert_eq!(sans.hi(), merged.hi());
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Plan M3.2 DoD "Span invariants proptested": for every
        /// representable `(lo, len)`, the constructed span round-trips
        /// (`hi == lo + len`, `byte_range()` covers exactly the length) and
        /// never reads as backwards.
        #[test]
        fn new_range_round_trip(lo_raw in 0_u32..=u32::MAX, len in 0_u32..=1_000) {
            let lo_raw = lo_raw.saturating_sub(len);
            let lo = ByteIdx::try_from_usize(lo_raw as usize)
                .expect("lo_raw <= u32::MAX");
            let span = Span::new(FileId::from_usize(0), lo, len, SyntaxContext::ROOT)
                .expect("lo + len <= u32::MAX by construction");
            prop_assert_eq!(span.len(), len);
            prop_assert_eq!(span.hi().to_usize(), lo_raw as usize + len as usize);
            prop_assert_eq!(span.byte_range().end - span.byte_range().start, len as usize);
        }

        /// The `(lo, hi)` bridge: for every ordered pair, `range` builds a
        /// span whose `len` is exactly `hi - lo` (no saturating, no wrap).
        #[test]
        fn range_len_equals_hi_minus_lo(a in 0_u32..=u32::MAX, b in 0_u32..=u32::MAX) {
            let (lo_raw, hi_raw) = if a <= b { (a, b) } else { (b, a) };
            let span = Span::range(
                FileId::from_usize(0),
                ByteIdx::try_from_usize(lo_raw as usize).expect("fits"),
                ByteIdx::try_from_usize(hi_raw as usize).expect("fits"),
                SyntaxContext::ROOT,
            );
            prop_assert_eq!(span.len(), hi_raw - lo_raw);
            prop_assert_eq!(span.hi().to_raw(), hi_raw);
        }

        /// Merging two spans yields the covering span: the merge's `hi` is
        /// the max of the two ends and its `lo` the min of the two starts.
        #[test]
        fn merge_covers_both_spans(
            a_lo in 0_u32..4_096,
            a_len in 0_u32..256,
            b_lo in 0_u32..4_096,
            b_len in 0_u32..256,
        ) {
            let file = FileId::from_usize(3);
            let a = Span::new(
                file,
                ByteIdx::try_from_usize(a_lo as usize).expect("fits"),
                a_len,
                SyntaxContext::ROOT,
            )
            .expect("fits");
            let b = Span::new(
                file,
                ByteIdx::try_from_usize(b_lo as usize).expect("fits"),
                b_len,
                SyntaxContext::ROOT,
            )
            .expect("fits");
            let merged = a.to(b);
            prop_assert!(merged.lo().to_usize() <= a.lo().to_usize());
            prop_assert!(merged.lo().to_usize() <= b.lo().to_usize());
            prop_assert!(merged.hi().to_usize() >= a.hi().to_usize());
            prop_assert!(merged.hi().to_usize() >= b.hi().to_usize());
        }

        /// `ByteIdx::try_from_usize` is the total, checked mint: every
        /// `usize` past `u32::MAX` is refused (never narrowed/wrapped).
        #[test]
        fn byteidx_mint_never_wraps(huge in (u32::MAX as usize + 1)..=(u32::MAX as usize + 1_000)) {
            prop_assert!(ByteIdx::try_from_usize(huge).is_err());
        }
    }
}
