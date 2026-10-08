// ignore
// test-mode: run-pass
// compile-flags: --with-stdlib
// check-stdout: abc
//
// KNOWN-UNIMPLEMENTED: `format!("{}", x)` interpolation.
//
// `println!("{}", s.as_str())` expands to `format!(concat!("{}","\n"), x)`,
// which needs `format!` to emit `String::new` / `push_str` / `to_string`.
// Lowering those to LLVM requires modelling `String` (ADT 1050) as a wrapper
// around `Vec<u8>` (3-field: ptr/cap/len) and bridging `&String` (thin
// pointer) to `&str` (fat pointer `{ptr,i64}`). That layout bridge is a
// workstream, not a patch — see docs/roadmaps/format-interp-gap.md.
//
// The other four corpus probes (`probe_println_function`, `probe_println_macro`,
// `probe_vec_push_len`, `probe_option_unwrap`) pass. This one is skipped.
fn main() {
    let mut s = String::new();
    s.push_str("abc");
    println!("{}", s.as_str());
}
