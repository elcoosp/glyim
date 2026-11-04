// §3.10 second variant: trait with `&mut self` on a generic-lifetime ADT.
// test-mode: run-pass
// exit-code: 42
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
