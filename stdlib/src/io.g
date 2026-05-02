// Glyim Standard Library — I/O Types

extern {
    fn write(fd: i32, buf: *const u8, len: i64) -> i64;
}

pub struct Stdout {
    fd: i64,
}

pub struct Stderr {
    fd: i64,
}

pub fn stdout() -> Stdout {
    Stdout { fd: 1 }
}

pub fn stderr() -> Stderr {
    Stderr { fd: 2 }
}
