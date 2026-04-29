// Glyim Standard Library — I/O Types
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. File descriptor (i32) as first-class type – feasible but needs type system support
//   §2. Raw pointer types (*const u8, *mut u8) only work with named types, not generic T
//   §3. No way to read a file into memory without pointer load operations
//   §4. Struct with fd field needs to be constructible with pointer value
//   §5. BufReader needs Vec<u8> which is blocked
//
// DESIGN:
//   struct File { fd: i32 }
//   struct Stdout { fd: i32 }
//   struct Stderr { fd: i32 }
//
// fn File::open(path: &str) -> Result<File, i32> { ... }
// fn File::read(self: *File, buf: *mut u8, count: i64) -> Result<i64, i32> { ... }
// fn File::close(self: *mut File) { ... }
//
// fn Stdout::write(self: *Stdout, buf: *const u8, count: i64) -> Result<i64, i32> { ... }
// fn Stderr::write(self: *Stderr, buf: *const u8, count: i64) -> Result<i64, i32> { ... }
//
// fn stdout() -> Stdout { Stdout { fd: 1 } }
// fn stderr() -> Stderr { Stderr { fd: 2 } }
//
// struct BufReader { file: File, buffer: Vec<u8>, pos: i64 }
