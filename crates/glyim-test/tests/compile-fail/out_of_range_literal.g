// §3.5.18: an unsuffixed integer literal that does not fit its inferred
// type must be a hard error, not a silent wrap to 0. Rust's default type
// for an unconstrained integer literal is `i32`; 4294967296 (2^32) does
// not fit i32, so Rust rejects this with `literal out of range`.
// test-mode: compile-fail
fn main() -> i32 {
    let x = 4294967296; //~ ERROR does not fit
    let _ = x;
    0
}
