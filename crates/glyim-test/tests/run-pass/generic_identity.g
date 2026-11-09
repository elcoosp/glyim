//@ mode: run-pass
//@ expect-exit: 41
//@ feature: COMPILER-MONO-MONOMORPHIZE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn id<T>(x: T) -> T { x }

fn main() -> i32 {
    id(41)
}
