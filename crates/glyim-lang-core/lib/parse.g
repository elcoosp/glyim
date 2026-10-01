//! Integer `FromStr` implementations.
//!
//! The `FromStr` trait is declared in `str.g`; this file provides the
//! primitive-type impls. They are emitted FLAT (see the stdlib assembler in
//! `glyim-lang-std/src/lib.rs`) so `impl FromStr for i32` binds to the
//! primitive `i32` rather than to a module named `i32`.
//!
//! Every `from_str` body is fully inlined (no shared helper) so each impl is
//! self-contained. The magnitude accumulates in `u64` with the exact
//! pre-multiply overflow check `value > (LIMIT - digit) / 10`, since the
//! language has no `i128`/`u128` accumulator.

/// The error type for integer parsing. A dedicated zero-sized type (rather
/// than `()`) so callers can pattern-match / format it like Rust's
/// `ParseIntError` without colliding with the unit type used elsewhere.
struct ParseIntError;

impl FromStr for u8 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<u8, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        let mut i: usize = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (255 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        Result::Ok(value as u8)
    }
}

impl FromStr for u16 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<u16, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        let mut i: usize = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (65535 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        Result::Ok(value as u16)
    }
}

impl FromStr for u32 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<u32, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        let mut i: usize = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (4294967295 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        Result::Ok(value as u32)
    }
}

impl FromStr for u64 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<u64, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        let mut i: usize = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (18446744073709551615 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        Result::Ok(value)
    }
}

impl FromStr for usize {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<usize, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        let mut i: usize = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (18446744073709551615 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        Result::Ok(value as usize)
    }
}

impl FromStr for i8 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<i8, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' { neg = true; i = 1; }
        if i >= len { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (128 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        if !neg && value > 127 { return Result::Err(ParseIntError); }
        if neg { Result::Ok(-(value as i8)) } else { Result::Ok(value as i8) }
    }
}

impl FromStr for i16 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<i16, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' { neg = true; i = 1; }
        if i >= len { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (32768 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        if !neg && value > 32767 { return Result::Err(ParseIntError); }
        if neg { Result::Ok(-(value as i16)) } else { Result::Ok(value as i16) }
    }
}

impl FromStr for i32 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<i32, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' { neg = true; i = 1; }
        if i >= len { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (2147483648 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        if !neg && value > 2147483647 { return Result::Err(ParseIntError); }
        if neg {
            Result::Ok(-(value as i32))
        } else {
            Result::Ok(value as i32)
        }
    }
}

impl FromStr for i64 {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<i64, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' { neg = true; i = 1; }
        if i >= len { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (9223372036854775808 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        if !neg && value > 9223372036854775807 { return Result::Err(ParseIntError); }
        if neg {
            if value == 9223372036854775808 {
                Result::Ok(-9223372036854775807 - 1)
            } else {
                Result::Ok(-(value as i64))
            }
        } else {
            Result::Ok(value as i64)
        }
    }
}

impl FromStr for isize {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<isize, ParseIntError> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len == 0 { return Result::Err(ParseIntError); }
        let mut i: usize = 0;
        let mut neg = false;
        if bytes[0] == b'-' { neg = true; i = 1; }
        if i >= len { return Result::Err(ParseIntError); }
        let mut value: u64 = 0;
        while i < len {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }
            let digit: u64 = (ch as u64) - (b'0' as u64);
            if value > (9223372036854775808 - digit) / 10 { return Result::Err(ParseIntError); }
            value = value * 10 + digit;
            i += 1;
        }
        if !neg && value > 9223372036854775807 { return Result::Err(ParseIntError); }
        if neg {
            if value == 9223372036854775808 {
                Result::Ok(-9223372036854775807 - 1)
            } else {
                Result::Ok(-(value as isize))
            }
        } else {
            Result::Ok(value as isize)
        }
    }
}
