// Glyim Standard Library — I/O Types

extern {
    fn write(fd: i32, buf: *const u8, len: i64) -> i64;
    fn read(fd: i32, buf: *mut u8, len: i64) -> i64;
    fn open(path: *const u8, flags: i32) -> i64;
    fn close(fd: i32) -> i64;
}

pub struct Stdin {
    fd: i64,
}

pub struct Stdout {
    fd: i64,
}

pub struct Stderr {
    fd: i64,
}

pub struct File {
    fd: i64,
}

pub fn stdin() -> Stdin {
    Stdin { fd: 0 }
}

pub fn stdout() -> Stdout {
    Stdout { fd: 1 }
}

pub fn stderr() -> Stderr {
    Stderr { fd: 2 }
}

impl Stdin {
    pub fn read(self: Stdin, buf: *mut u8, len: i64) -> i64 {
        read(self.fd as i32, buf, len)
    }
}

impl Stdout {
    pub fn write(self: Stdout, buf: *const u8, len: i64) -> i64 {
        write(self.fd as i32, buf, len)
    }
}

impl Stderr {
    pub fn write(self: Stderr, buf: *const u8, len: i64) -> i64 {
        write(self.fd as i32, buf, len)
    }
}

impl File {
    pub fn read(self: File, buf: *mut u8, len: i64) -> i64 {
        read(self.fd as i32, buf, len)
    }
    pub fn write(self: File, buf: *const u8, len: i64) -> i64 {
        write(self.fd as i32, buf, len)
    }
    pub fn close(self: File) -> i64 {
        close(self.fd as i32)
    }
}

pub fn file_open(path: *const u8, flags: i32) -> File {
    let fd = open(path, flags);
    File { fd }
}

// ── Print Utilities ──────────────────────────────────────────────

pub fn print_str(ptr: *const u8, len: i64) -> i64 {
    write(1, ptr, len)
}

pub fn println_str(ptr: *const u8, len: i64) -> i64 {
    let n = write(1, ptr, len);
    let newline = "\n" as *const u8;
    let _ = write(1, newline, 1);
    n
}


// For now, we skip int-to-string; users can use `println(val)` built‑in.
