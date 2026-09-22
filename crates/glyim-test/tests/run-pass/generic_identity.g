// test-mode: run-pass
// exit-code: 41

fn id<T>(x: T) -> T { x }

fn main() -> i32 {
    id(41)
}
