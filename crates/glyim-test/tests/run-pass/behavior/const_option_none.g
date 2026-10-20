// `Option::None` as a const exercises the *niche* tag encoding: the
// discriminant is stored inside the payload rather than in a dedicated tag
// field, so the emitted constant must write the right bytes for it.
// test-mode: run-pass
// exit-code: 41
// compile-flags: --with-stdlib
const K: Option<i32> = Option::None;
fn main() -> i32 {
    match K {
        Option::Some(v) => v,
        Option::None => 41,
    }
}
