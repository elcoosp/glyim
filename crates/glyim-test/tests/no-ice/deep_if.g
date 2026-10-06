// test-mode: no-ice
// A deeply nested `else if` chain. The T078 recursion guard must produce a
// diagnostic instead of overflowing the stack (FE-111).
fn main() -> i32 {
    if false { 0 }
    else if false { 1 }
    else if false { 2 }
    else if false { 3 }
    else if false { 4 }
    else if false { 5 }
    else if false { 6 }
    else if false { 7 }
    else if false { 8 }
    else if false { 9 }
    else if false { 10 }
    else if false { 11 }
    else if false { 12 }
    else if false { 13 }
    else if false { 14 }
    else if false { 15 }
    else if false { 16 }
    else if false { 17 }
    else if false { 18 }
    else if false { 19 }
    else if false { 20 }
    else if false { 21 }
    else if false { 22 }
    else if false { 23 }
    else if false { 24 }
    else if false { 25 }
    else if false { 26 }
    else if false { 27 }
    else if false { 28 }
    else if false { 29 }
    else if false { 30 }
    else { 31 }
}
