// Dynamic (variable) index write into a static mut array.
// test-mode: run-pass
// exit-code: 33
// compile-flags: --with-stdlib
static mut A: [i32; 3] = [1, 2, 3];
fn idx() -> usize { 1 }
fn main() -> i32 {
    unsafe {
        A[idx()] = 29;
        A[0] + A[1] + A[2]
    }
}
