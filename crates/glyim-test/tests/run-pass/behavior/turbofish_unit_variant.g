// §3.5.24 Bug B: turbofish on a unit-variant path must carry the type
// argument through to the variant's substitution.
//
// `Option::<i32>::None` used to reach LLVM lowering with an empty
// substitution (`Adt(1010, <error>)`) because `variant_expr` in
// `unify.rs` unconditionally allocated fresh inference vars instead of
// honouring the explicit turbofish args. The fix threads
// `path.segments[0].generic_args` through to `variant_expr`, which now
// seeds the substitution slot from the resolved type when one is
// available.
//
// Exercises: unit variant + turbofish in a match scrutinee (the
// narrowest shape that used to ICE); the annotated `let` form is already
// covered by `if_let_some_none.g`.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-SYN-TUPLE-STRUCT, COMPILER-MONO-MONOMORPHIZE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    match Option::<i32>::None {
        Option::Some(_) => 1,
        Option::None => 7,
    }
}
