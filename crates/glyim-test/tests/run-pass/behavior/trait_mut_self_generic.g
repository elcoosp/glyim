// §3.10 second variant: trait with `&mut self` on a generic-lifetime ADT.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-SELF, LANG-SYN-MUT, LANG-TRAIT-IMPL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
trait Read {
    fn get(&mut self) -> i32;
}

struct W<'a> { r: &'a [i32] }

impl<'a> Read for W<'a> {
    fn get(&mut self) -> i32 { self.r[0] }
}

fn main() -> i32 {
    let s: &[i32] = &[42, 20, 30];
    let mut w = W { r: s };
    w.get()
}
