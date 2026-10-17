// ANTI-MISCOMPILATION: one program that combines four independent
// computations, each of which would return a *different wrong value*
// under a specific miscompilation regression:
//
//   a = "11".parse::<i32>()          -- turbofish substitution correctness
//   b = match "13".parse::<i32>()    -- match payload ABI
//   c = Option::Some(Option::Some(17)).unwrap().unwrap()
//                                     -- nested generic instantiation
//   d = match "fail".parse::<i32>()  -- Err-arm discriminator
//
// The sum is 60. Each individual failure mode produces a distinct exit
// code (e.g. wrong turbofish → 49; wrong match ABI → 47; wrong nested
// unwrap → 43; wrong Err arm → 41) so a test failure immediately
// narrows the bug class even before running the individual fixtures
// above.
//
// A cross-type case (`parse::<u8>` combined with an i32 value in the
// same sum) is deliberately not included: `parse::<u8>().unwrap() as i32`
// currently trips a *separate* unrelated cast-validation gap (the
// `<T as FromStr>::Err` projection on the `unwrap` return type makes the
// cast check see a non-castable type). That is documented in the
// `generic_method_turbofish_all_primitives.g` fixture, which exercises
// every primitive `FromStr` impl at the parse site without casting the
// parsed value across types.
// test-mode: run-pass
// exit-code: 60
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = "11".parse::<i32>().unwrap();                                       // 11
    let b = match "13".parse::<i32>() {                                         // 13
        Result::Ok(v) => v,
        Result::Err(_) => -1,
    };
    let c: i32 = Option::Some(Option::Some(17)).unwrap().unwrap();              // 17
    let d = match "fail".parse::<i32>() {                                       // 19
        Result::Ok(_) => 0,
        Result::Err(_) => 19,
    };
    // 11 + 13 + 17 + 19 = 60
    a + b + c + d
}
