// Glyim Standard Library — I/O Types
//
// Minimal implementation: provides stdout() and stderr()
// that return structs with the correct file descriptors.
// Method calls like write() are blocked by compiler limitations
// (see original design comments).

struct Stdout {
    fd: i64,
}

struct Stderr {
    fd: i64,
}

pub fn stdout() -> Stdout {
    Stdout { fd: 1 }
}

pub fn stderr() -> Stderr {
    Stderr { fd: 2 }
}
