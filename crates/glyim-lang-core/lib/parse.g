//! Integer `FromStr` implementations.
//!
//! The `FromStr` trait is declared in `str.g`; this file provides the
//! primitive-type impls. They are emitted FLAT (see the stdlib assembler in
//! `glyim-lang-std/src/lib.rs`) so `impl FromStr for i32` binds to the
//! primitive `i32` rather than to a module named `i32`.

/// The error type for integer parsing. A dedicated zero-sized type (rather
/// than `()`) so callers can pattern-match / format it like Rust's
/// `ParseIntError` without colliding with the unit type used elsewhere.
struct ParseIntError;

impl FromStr for i32 {
    type Err = ParseIntError;

    /// Parse a decimal `i32` from `s`, matching Rust's `i32::from_str`
    /// semantics: an optional leading `-`, then one or more ASCII digits,
    /// no whitespace, no `+` sign (yet).
    fn from_str(s: &str) -> Result<i32, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 {
            return Result::Err(ParseIntError);
        }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' {
            neg = true;
            i = 1;
        }
        // A lone `-` (or `+` if we later accept it) is not a number.
        if i >= len {
            return Result::Err(ParseIntError);
        }
        // Accumulate in i64 so an overflowing magnitude is detected without
        // wrapping (then range-checked against i32's bounds at the end).
        let mut value: i64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' {
                return Result::Err(ParseIntError);
            }
            let digit: i64 = (ch as i64) - (b'0' as i64);
            value = value * 10 + digit;
            if value > 2147483648 {
                return Result::Err(ParseIntError);
            }
            i += 1;
        }
        // i32::MIN = -2147483648 (one more than i32::MAX in magnitude).
        let limit: i64 = if neg { 2147483648 } else { 2147483647 };
        if value > limit {
            return Result::Err(ParseIntError);
        }
        let signed: i64 = if neg { -value } else { value };
        Result::Ok(signed as i32)
    }
}
