// Multi-file with `dir/mod.g` layout: `mod math;` resolves to
// `math/mod.g` when `math.g` is absent.
// test-mode: run-pass
// exit-code: 12
mod math;
fn main() -> i32 { math::triple(4) }
