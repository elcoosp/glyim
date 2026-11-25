//! Property tests for the checked layout newtypes (plan §6 M3.10).
//!
//! Plan acceptance line (M3.10): *"Property tests for foundations:
//! `IndexVec`, interner, spans, `Size/Align`, substitution, unification"*
//! with the `DoD` `≥ 200 properties; run in PR tier`. The spans half is
//! pinned in `glyim-span` (M3.2); this module pins the CT-3 layout
//! vocabulary — [`Size`], [`Align`], [`Offset`] — against plain `u64`
//! reference arithmetic (the CO-10 differential-oracle taste: every
//! checked operation must agree with the naive computation wherever the
//! naive one is defined, and refuse — never wrap or saturate — where it
//! is not, the FIXES §4 class the newtypes exist to close).
//!
//! Runs in the default `cargo test` PR tier; no LLVM dependency.

use crate::{Align, Offset, Size};
use proptest::prelude::*;

/// The reference ceil-to-alignment every `checked_align_to` property
/// compares against: `(x + a - 1) / a * a` when it fits, `None` when the
/// rounding addition overflows.
fn reference_align_to(x: u64, a: u64) -> Option<u64> {
    x.checked_add(a - 1).map(|ceil| (ceil / a) * a)
}

/// Arbitrary power-of-two alignment (exactly the `Align::from_bytes`
/// domain), biased toward small values layouts actually use.
fn align_bytes() -> impl Strategy<Value = u64> {
    prop_oneof![
        4 => prop::num::u64::ANY.prop_map(|raw| 1u64 << (raw % 64)),
        1 => prop_oneof![Just(1u64), Just(2), Just(4), Just(8), Just(16), Just(64)],
    ]
}

proptest! {
    // ------------------------------------------------------------------
    // Size: checked arithmetic agrees with the u64 reference or refuses.
    // ------------------------------------------------------------------

    /// `Size::bytes` after `as_u64` is the identity (total constructor).
    #[test]
    fn size_bytes_round_trip(b in any::<u64>()) {
        prop_assert_eq!(Size::bytes(b).as_u64(), b);
    }

    /// `checked_add` is exactly `u64::checked_add` (agreement inside the
    /// domain, refusal at the boundary — no saturation, no wrap).
    #[test]
    fn size_checked_add_matches_reference(a in any::<u64>(), b in any::<u64>()) {
        let size = Size::bytes(a).checked_add(Size::bytes(b));
        prop_assert_eq!(size.as_ref().map(|sz| sz.as_u64()), a.checked_add(b));
        if let Some(sum) = size {
            prop_assert!(sum.as_u64() >= a && sum.as_u64() >= b, "add is monotone");
        }
    }

    /// `checked_mul` is exactly `u64::checked_mul` (element counts into
    /// array sizes never wrap).
    #[test]
    fn size_checked_mul_matches_reference(a in any::<u64>(), n in any::<u64>()) {
        let size = Size::bytes(a).checked_mul(n);
        prop_assert_eq!(size.as_ref().map(|sz| sz.as_u64()), a.checked_mul(n));
    }

    /// `checked_bits` is exactly `bytes * 8` in checked arithmetic; the
    /// `MAX` size must refuse (the one product guaranteed to overflow).
    #[test]
    fn size_checked_bits_matches_reference(b in any::<u64>()) {
        prop_assert_eq!(Size::bytes(b).checked_bits(), b.checked_mul(8));
        prop_assert_eq!(Size::MAX.checked_bits(), None);
        prop_assert_eq!(Size::MAX.as_u64(), u64::MAX);
    }

    /// `checked_align_to` equals the reference ceil-division formula for
    /// every representable pair; the result is aligned and never smaller.
    #[test]
    fn size_align_to_matches_reference(
        x in any::<u64>(),
        a in align_bytes(),
    ) {
        let aligned = Size::bytes(x).checked_align_to(Align::from_bytes(a).expect("pow2"));
        prop_assert_eq!(aligned.as_ref().map(|sz| sz.as_u64()), reference_align_to(x, a));
        if let Some(aligned) = aligned {
            prop_assert!(aligned.as_u64() >= x, "rounding never truncates");
            prop_assert_eq!(aligned.as_u64() % a, 0, "result is aligned");
        }
    }

    /// Aligning to 1 is the identity and `ZERO` absorbs addition (the
    /// neutral elements the layout computer leans on).
    #[test]
    fn size_neutral_elements(x in any::<u64>()) {
        prop_assert_eq!(
            Size::bytes(x).checked_align_to(Align::ONE),
            Some(Size::bytes(x)),
            "align 1 is the identity"
        );
        prop_assert_eq!(
            Size::ZERO.checked_add(Size::bytes(x)),
            Some(Size::bytes(x)),
            "zero size absorbs"
        );
        prop_assert!(Size::ZERO.checked_align_to(Align::EIGHT).is_some());
    }

    /// The overflow boundary is exact: the sum that lands exactly on
    /// `u64::MAX` is accepted, one byte past it is refused (sampled along
    /// the boundary, where the old saturating `Add` lied).
    #[test]
    fn size_add_boundary_is_exact(back in 0_u64..1_000) {
        let near_max = u64::MAX - back;
        let exact = Size::bytes(near_max).checked_add(Size::bytes(back));
        prop_assert_eq!(exact, Some(Size::bytes(u64::MAX)), "the boundary sum fits");
        let past = Size::bytes(near_max).checked_add(Size::bytes(back + 1));
        prop_assert_eq!(past, None, "one byte past the boundary is refused");
    }

    // ------------------------------------------------------------------
    // Align: the power-of-two domain is enforced at the mint.
    // ------------------------------------------------------------------

    /// `Align::from_bytes` accepts exactly the non-zero power-of-two
    /// values (differential against the popcount criterion), and the
    /// round-trip through `as_u64` is the identity.
    #[test]
    fn align_mint_is_exactly_pow2(b in any::<u64>()) {
        let is_pow2 = b.is_power_of_two();
        let minted = Align::from_bytes(b);
        prop_assert_eq!(minted.is_some(), is_pow2);
        if let Some(align) = minted {
            prop_assert_eq!(align.as_u64(), b);
        }
    }

    /// `Align::max` is the stricter alignment: commutative, idempotent,
    /// agrees with the numeric max, and always at least each operand.
    #[test]
    fn align_max_is_stricter(a in align_bytes(), b in align_bytes()) {
        let (a, b) = (
            Align::from_bytes(a).expect("pow2"),
            Align::from_bytes(b).expect("pow2"),
        );
        let stricter = a.max(b);
        prop_assert_eq!(stricter.as_u64(), a.as_u64().max(b.as_u64()));
        prop_assert_eq!(a.max(b), b.max(a), "commutative");
        prop_assert_eq!(a.max(a), a, "idempotent");
        prop_assert!(stricter.as_u64() >= a.as_u64() && stricter.as_u64() >= b.as_u64());
    }

    /// `max` is associative: the layout computer may group in any order.
    #[test]
    fn align_max_is_associative(a in align_bytes(), b in align_bytes(), c in align_bytes()) {
        let (a, b, c) = (
            Align::from_bytes(a).expect("pow2"),
            Align::from_bytes(b).expect("pow2"),
            Align::from_bytes(c).expect("pow2"),
        );
        prop_assert_eq!(a.max(b).max(c), a.max(b.max(c)));
    }

    // ------------------------------------------------------------------
    // Offset: field-accumulator arithmetic agrees with the reference.
    // ------------------------------------------------------------------

    /// `Offset::at_byte` after `as_u64` is the identity.
    #[test]
    fn offset_at_byte_round_trip(b in any::<u64>()) {
        prop_assert_eq!(Offset::at_byte(b).as_u64(), b);
    }

    /// `checked_add(size)` is exactly `u64::checked_add` (the field
    /// accumulator never wraps past the end of the address space).
    #[test]
    fn offset_checked_add_matches_reference(o in any::<u64>(), s in any::<u64>()) {
        let advanced = Offset::at_byte(o).checked_add(Size::bytes(s));
        prop_assert_eq!(advanced.as_ref().map(|off| off.as_u64()), o.checked_add(s));
    }

    /// `checked_align_to` equals the reference formula; the padded offset
    /// is aligned and never smaller than the unpadded one.
    #[test]
    fn offset_align_to_matches_reference(
        x in any::<u64>(),
        a in align_bytes(),
    ) {
        let aligned = Offset::at_byte(x).checked_align_to(Align::from_bytes(a).expect("pow2"));
        prop_assert_eq!(aligned.as_ref().map(|off| off.as_u64()), reference_align_to(x, a));
        if let Some(aligned) = aligned {
            prop_assert!(aligned.as_u64() >= x);
            prop_assert_eq!(aligned.as_u64() % a, 0);
        }
    }

    /// `to_size` views the offset as the prefix size it names: the byte
    /// counts agree, and `Offset::ZERO` advanced by a size lands on it.
    #[test]
    fn offset_to_size_and_zero_identity(s in any::<u64>()) {
        let offset = Offset::at_byte(s);
        prop_assert_eq!(offset.to_size().as_u64(), s);
        prop_assert_eq!(
            Offset::ZERO.checked_add(Size::bytes(s)),
            Some(Offset::at_byte(s)),
            "the first field starts at byte 0"
        );
    }

    /// Size and Offset are distinct vocabularies that agree on the byte
    /// count they carry: `offset.to_size()` re-enters the Size checked
    /// arithmetic with the same value (the vocabulary bridge is lossless).
    #[test]
    fn offset_size_bridge_is_lossless(o in any::<u64>(), delta in 0_u64..1_000) {
        let bridged = Offset::at_byte(o).to_size();
        let advanced = bridged.checked_add(Size::bytes(delta));
        prop_assert_eq!(advanced.as_ref().map(|sz| sz.as_u64()), o.checked_add(delta));
    }
}

/// The constants are themselves valid alignments (they bypass the checked
/// mint, so the invariant is pinned independently of the generator).
#[test]
fn align_constants_are_valid() {
    assert_eq!(Align::ONE.as_u64(), 1);
    assert_eq!(Align::EIGHT.as_u64(), 8);
    // The constants bypass `from_bytes`, so their domain membership is
    // pinned against the mint: 0 and non-powers-of-two are refused there.
    assert_eq!(Align::from_bytes(0), None, "zero is not an alignment");
    assert_eq!(Align::from_bytes(3), None, "3 is not a power of two");
    assert!(Align::from_bytes(Align::ONE.as_u64()).is_some());
    assert!(Align::from_bytes(Align::EIGHT.as_u64()).is_some());
}

proptest! {
    /// `Size` ordering is exactly `u64` ordering (the `Ord` derive must
    /// not disagree with the byte counts it wraps).
    #[test]
    fn size_ordering_matches_bytes(a in any::<u64>(), b in any::<u64>()) {
        let (sa, sb) = (Size::bytes(a), Size::bytes(b));
        prop_assert_eq!(sa.cmp(&sb), a.cmp(&b));
        prop_assert_eq!(sa == sb, a == b);
        prop_assert_eq!(sa < sb, a < b);
    }

    /// `checked_align_to` is idempotent: padding an already-aligned size
    /// is a no-op (the layout accumulator relies on this fixpoint).
    #[test]
    fn size_align_to_is_idempotent(x in any::<u64>(), a in align_bytes()) {
        let align = Align::from_bytes(a).expect("pow2");
        let once = Size::bytes(x).checked_align_to(align);
        if let Some(aligned) = once {
            prop_assert_eq!(
                aligned.checked_align_to(align),
                Some(aligned),
                "aligning an aligned size is the identity"
            );
        }
    }

    /// Adding a multiple of the alignment commutes with aligning: for
    /// aligned strides, `align(x + k*a) == align(x) + k*a` (the field-
    /// stride formula the array layout uses).
    #[test]
    fn size_aligned_adds_commute_with_align(
        x in any::<u64>(),
        a in align_bytes(),
        k in 0_u64..1_000,
    ) {
        let align = Align::from_bytes(a).expect("pow2");
        let stride = k.checked_mul(a);
        if let (Some(base), Some(stride)) = (
            Size::bytes(x).checked_align_to(align),
            stride.map(Size::bytes),
        ) {
            let shifted = base.checked_add(stride);
            let direct = Size::bytes(x)
                .checked_add(stride)
                .and_then(|sum| sum.checked_align_to(align));
            prop_assert_eq!(direct, shifted);
        }
    }

    /// Checked addition is associative wherever all partial sums fit:
    /// `(a + b) + c == a + (b + c)`, and refusal agrees on both sides.
    #[test]
    fn size_add_is_associative(
        a in any::<u64>(),
        b in any::<u64>(),
        c in any::<u64>(),
    ) {
        let (sa, sb, sc) = (Size::bytes(a), Size::bytes(b), Size::bytes(c));
        let left = sa.checked_add(sb).and_then(|sum| sum.checked_add(sc));
        let right = sb.checked_add(sc).and_then(|sum| sa.checked_add(sum));
        prop_assert_eq!(left, right);
    }

    /// `Offset::checked_align_to` is idempotent too (the field cursor is
    /// aligned at most once per field).
    #[test]
    fn offset_align_to_is_idempotent(x in any::<u64>(), a in align_bytes()) {
        let align = Align::from_bytes(a).expect("pow2");
        let once = Offset::at_byte(x).checked_align_to(align);
        if let Some(aligned) = once {
            prop_assert_eq!(aligned.checked_align_to(align), Some(aligned));
        }
    }

    /// The vocabulary bridge is arithmetic-preserving: advancing an
    /// offset by a size equals advancing the offset's size-view by the
    /// same amount (offsets and sizes are the same bytes, differently
    /// named).
    #[test]
    fn offset_and_size_views_agree(o in any::<u64>(), s in any::<u64>()) {
        let via_offset = Offset::at_byte(o).checked_add(Size::bytes(s));
        let via_size = Offset::at_byte(o).to_size().checked_add(Size::bytes(s));
        prop_assert_eq!(via_offset.map(Offset::as_u64), via_size.map(Size::as_u64));
    }

    /// `Align::max` dominates `ONE` and `EIGHT` per the numeric max (the
    /// lattice top the struct-layout walk relies on).
    #[test]
    fn align_max_dominates_constants(a in align_bytes()) {
        let align = Align::from_bytes(a).expect("pow2");
        prop_assert_eq!(Align::ONE.max(align), align);
        prop_assert_eq!(align.max(Align::ONE), align);
        let dominated = align.max(Align::EIGHT);
        prop_assert!(dominated.as_u64() >= 8);
        prop_assert!(dominated.as_u64() >= align.as_u64());
    }
}

proptest! {
    /// The align refusal boundary is exact for `Size`: padding a size
    /// that sits exactly on the last representable multiple succeeds,
    /// and the checked form refuses precisely when the reference
    /// rounding-addition would overflow.
    #[test]
    fn size_align_refusal_boundary_matches_reference(
        x in any::<u64>(),
        a in align_bytes(),
    ) {
        let align = Align::from_bytes(a).expect("pow2");
        let direct = Size::bytes(x).checked_align_to(align);
        prop_assert_eq!(direct.as_ref().map(|sz| sz.as_u64()), reference_align_to(x, a));
        // Alignment is a fixpoint of the reference formula too.
        if let Some(aligned) = direct {
            prop_assert_eq!(
                reference_align_to(aligned.as_u64(), a),
                Some(aligned.as_u64())
            );
        }
    }

    /// The align refusal boundary is exact for `Offset` (the field
    /// cursor can sit arbitrarily close to the address-space top).
    #[test]
    fn offset_align_refusal_boundary_matches_reference(
        x in any::<u64>(),
        a in align_bytes(),
    ) {
        let align = Align::from_bytes(a).expect("pow2");
        let direct = Offset::at_byte(x).checked_align_to(align);
        prop_assert_eq!(direct.as_ref().map(|off| off.as_u64()), reference_align_to(x, a));
    }
}
