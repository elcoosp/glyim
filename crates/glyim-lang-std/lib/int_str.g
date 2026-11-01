//! Integer-to-string conversions (§3.5.22).
//!
//! `format!("{}", x)` and `x.to_string()` on integer primitives route
//! through these free functions. `String` has no inherent `push(char)`;
//! digits are appended with `push_str` on a one-char `&str`.

fn i32_to_string(n: i32) -> String {
    let mut s = String::new();
    if n == 0 { s.push_str("0"); return s; }
    if n == -2147483648 { s.push_str("-2147483648"); return s; }
    let mut neg = false;
    let mut v = n;
    if v < 0 { neg = true; v = -v; }
    let mut digits: Vec<u8> = Vec::new();
    while v > 0 {
        digits.push((v % 10) as u8);
        v = v / 10;
    }
    if neg { s.push_str("-"); }
    let mut i = digits.len();
    while i > 0 {
        i = i - 1;
        let d = digits[i];
        match d {
            0 => s.push_str("0"), 1 => s.push_str("1"),
            2 => s.push_str("2"), 3 => s.push_str("3"),
            4 => s.push_str("4"), 5 => s.push_str("5"),
            6 => s.push_str("6"), 7 => s.push_str("7"),
            8 => s.push_str("8"), _ => s.push_str("9"),
        }
    }
    s
}

fn u32_to_string(n: u32) -> String {
    let mut s = String::new();
    if n == 0 { s.push_str("0"); return s; }
    let mut v = n;
    let mut digits: Vec<u8> = Vec::new();
    while v > 0 {
        digits.push((v % 10) as u8);
        v = v / 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i = i - 1;
        let d = digits[i];
        match d {
            0 => s.push_str("0"), 1 => s.push_str("1"),
            2 => s.push_str("2"), 3 => s.push_str("3"),
            4 => s.push_str("4"), 5 => s.push_str("5"),
            6 => s.push_str("6"), 7 => s.push_str("7"),
            8 => s.push_str("8"), _ => s.push_str("9"),
        }
    }
    s
}

fn i64_to_string(n: i64) -> String {
    let mut s = String::new();
    if n == 0 { s.push_str("0"); return s; }
    if n == -9223372036854775808 { s.push_str("-9223372036854775808"); return s; }
    let mut neg = false;
    let mut v = n;
    if v < 0 { neg = true; v = -v; }
    let mut digits: Vec<u8> = Vec::new();
    while v > 0 {
        digits.push((v % 10) as u8);
        v = v / 10;
    }
    if neg { s.push_str("-"); }
    let mut i = digits.len();
    while i > 0 {
        i = i - 1;
        let d = digits[i];
        match d {
            0 => s.push_str("0"), 1 => s.push_str("1"),
            2 => s.push_str("2"), 3 => s.push_str("3"),
            4 => s.push_str("4"), 5 => s.push_str("5"),
            6 => s.push_str("6"), 7 => s.push_str("7"),
            8 => s.push_str("8"), _ => s.push_str("9"),
        }
    }
    s
}

fn u64_to_string(n: u64) -> String {
    let mut s = String::new();
    if n == 0 { s.push_str("0"); return s; }
    let mut v = n;
    let mut digits: Vec<u8> = Vec::new();
    while v > 0 {
        digits.push((v % 10) as u8);
        v = v / 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i = i - 1;
        let d = digits[i];
        match d {
            0 => s.push_str("0"), 1 => s.push_str("1"),
            2 => s.push_str("2"), 3 => s.push_str("3"),
            4 => s.push_str("4"), 5 => s.push_str("5"),
            6 => s.push_str("6"), 7 => s.push_str("7"),
            8 => s.push_str("8"), _ => s.push_str("9"),
        }
    }
    s
}
