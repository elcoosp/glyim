// `Option::None` as a const exercises the *niche* tag encoding: the
// discriminant is stored inside the payload rather than in a dedicated tag
// field, so the emitted constant must write the right bytes for it.
//@ mode: run-pass
//@ expect-exit: 41
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
const K: Option<i32> = Option::None;
fn main() -> i32 {
    match K {
        Option::Some(v) => v,
        Option::None => 41,
    }
}
