// KNOWN_GAPS §3.9 (this session): inherent slice methods returning references
// into the receiver (`first` / `last` / `split_first`) were broken by a
// fat-vs-thin ABI mismatch in the method-call autoref pass-through: the
// generic `&[T]` formal was compared by handle against the concrete `&[i32]`
// receiver, the receiver got re-wrapped into `&&[i32]`, and the callee read
// the fat pointer's data word as the length — garbage or SIGSEGV.
// test-mode: run-pass
// exit-code: 5
// Interp note: the MIR interpreter cannot pass a `&[T]` aggregate by value
// into a `&self` method frame (pre-existing interp limitation, KNOWN_GAPS
// §3.9 interp half); the LLVM backend is the gate for this fixture.
// needs-llvm
// compile-flags: --with-stdlib
fn main() -> i32 {
    let arr = [1, 2, 3];
    let s: &[i32] = &arr;
    let first = *s.first().unwrap();
    let last = *s.last().unwrap();
    let (a, _b) = s.split_first().unwrap();
    first + last + *a
}
