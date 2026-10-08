// test-mode: run-fail
// exit-code: 101
fn main() -> i32 {
    let o = if 1 > 2 { Option::Some(1) } else { Option::None };
    o.unwrap()
}
