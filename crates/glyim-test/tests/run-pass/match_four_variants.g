// test-mode: run-pass
// exit-code: 3

enum Four { A, B, C, D }

fn pick(f: Four) -> i32 {
    match f {
        Four::A => 1,
        Four::B => 2,
        Four::C => 3,
        Four::D => 4,
    }
}

fn main() -> i32 { pick(Four::C) }
