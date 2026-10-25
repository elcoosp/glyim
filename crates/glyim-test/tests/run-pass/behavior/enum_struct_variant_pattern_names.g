// Regression for KNOWN_GAPS §3.5.5 / P1-C6: struct-variant field names
// in patterns must bind the named field, not by position. Before this,
// `E::A { y, .. }` bound `y` to the *first* declared field (`x`), so
// this program exited 1 instead of 0.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
enum E {
    A { x: u8, y: u64 },
    B,
}

fn main() -> i32 {
    // Natural order (previously worked by coincidence).
    let e1 = E::A { x: 11, y: 42 };
    let r1 = match e1 {
        E::A { x, y } => (x as i32) == 11 && (y as i32) == 42,
        E::B => false,
    };

    // Reverse order (previously bound by position → both wrong).
    let e2 = E::A { x: 11, y: 42 };
    let r2 = match e2 {
        E::A { y, x } => (x as i32) == 11 && (y as i32) == 42,
        E::B => false,
    };

    // `..` rest pattern (previously bound `y` to `x`'s value).
    let e3 = E::A { x: 11, y: 42 };
    let r3 = match e3 {
        E::A { y, .. } => (y as i32) == 42,
        E::B => false,
    };

    if r1 && r2 && r3 { 0 } else { 1 }
}
