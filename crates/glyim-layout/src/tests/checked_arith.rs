//! CT-3 checked-arithmetic tests (plan §6 M3.2 / §5.2 CT-3).
//!
//! The plan line these tests pin:
//!
//! > `Size`, `Align`, `Offset` newtypes with checked arithmetic
//!
//! Layout arithmetic saturates or wraps silently today (`Size + Size`
//! saturates, `align_to` wraps, `Align::from_bytes` only `debug_assert`s
//! its power-of-two invariant); the checked newtypes make every overflow a
//! visible `None` that the layout computer turns into
//! `LayoutError::SizeOverflow` instead of a silent wrong ABI.

use crate::{Align, Offset, Size};

#[test]
fn size_checked_add_overflow_is_none() {
    let big = Size::bytes(u64::MAX - 3);
    let bump = Size::bytes(8);
    assert_eq!(big.checked_add(bump), None, "overflow must be visible");
    assert_eq!(bump.checked_add(big), None);
    let ok = Size::bytes(4).checked_add(Size::bytes(8));
    assert_eq!(ok, Some(Size::bytes(12)));
}

#[test]
fn size_align_to_overflow_is_none_and_exact_boundaries_round_trip() {
    let align = Align::EIGHT;
    // Exact boundary: 8 aligned to 8 is 8.
    assert_eq!(Size::bytes(8).checked_align_to(align), Some(Size::bytes(8)));
    // Round up: 9 -> 16.
    assert_eq!(
        Size::bytes(9).checked_align_to(align),
        Some(Size::bytes(16))
    );
    // Near-max: 0xFFFF...F9 rounds past u64::MAX -> None, never a wrapped
    // small size (the silent-wrap class).
    let near_max = Size::bytes(u64::MAX - 2);
    assert_eq!(
        near_max.checked_align_to(align),
        None,
        "align overflow must be refused, not wrapped"
    );
    // Zero alignment is not an alignment: refused, not a mask panic.
    let zero = Align::from_bytes(0);
    assert_eq!(zero, None, "0 is not a power of two");
}

#[test]
fn size_checked_mul_and_bits_refuse_overflow() {
    let big = Size::bytes(u64::MAX / 4 + 1);
    assert_eq!(big.checked_mul(4), None, "array-size overflow is visible");
    assert_eq!(Size::bytes(5).checked_mul(3), Some(Size::bytes(15)));
    // bits(): 8 * bytes near max cannot fit u64.
    let huge = Size::bytes(u64::MAX);
    assert_eq!(huge.checked_bits(), None);
    assert_eq!(Size::bytes(3).checked_bits(), Some(24));
}

#[test]
fn align_from_bytes_rejects_non_power_of_two_without_debug_asserts() {
    // The old debug_assert vanished in release; the checked constructor
    // refuses outright.
    assert_eq!(Align::from_bytes(0), None);
    assert_eq!(Align::from_bytes(3), None);
    assert_eq!(Align::from_bytes(12), None);
    assert_eq!(Align::from_bytes(1), Some(Align::ONE));
    assert_eq!(Align::from_bytes(8), Some(Align::EIGHT));
    assert_eq!(Align::from_bytes(64).map(Align::as_u64), Some(64));
}

#[test]
fn offset_checked_arithmetic_and_size_conversion() {
    let base = Offset::ZERO;
    let next = base.checked_add(Size::bytes(12)).expect("fits");
    assert_eq!(next.as_u64(), 12);
    let aligned = next
        .checked_align_to(Align::EIGHT)
        .expect("12 aligned to 8 is 16");
    assert_eq!(aligned.as_u64(), 16);
    assert!(
        Offset::at_byte(u64::MAX)
            .checked_add(Size::bytes(1))
            .is_none(),
        "offset overflow is visible"
    );
    assert_eq!(Offset::at_byte(24).to_size(), Size::bytes(24));
}

#[test]
fn max_is_total_and_accessors_round_trip() {
    assert_eq!(Size::MAX.as_u64(), u64::MAX);
    assert_eq!(Size::ZERO.as_u64(), 0);
    assert_eq!(Size::bytes(7).as_u64(), 7);
}
