// Glyim Standard Library — I/O Types
//
// STATUS: Blocked by compiler limitations
// BLOCKERS:
//   §1. User extern declarations conflict with compiler-emitted shims
//   §2. No i64→i32 cast support for primitive conversions
//   §3. Method calls with extern-backed types fail LLVM verification
//
// Working design (ready when compiler supports above):
//   struct File { fd: i64 }
//   struct Stdout { fd: i64 }
//   struct Stderr { fd: i64 }
//   fn stdout() -> Stdout { Stdout { fd: 1 } }
//   fn stderr() -> Stderr { Stderr { fd: 2 } }
//   impl Stdout { fn write(self, ptr: *const u8, len: i64) -> i64 { write(1, ptr, len) } }
