//! Native runtime shims for the JIT.
//! These #[no_mangle] extern "C" functions are linked into the test binary
//! and automatically available to the JIT engine without any symbol mapping.

use std::io::Write;

#[no_mangle]
pub extern "C" fn glyim_println_int(val: i64) {
    println!("{}", val);
}

#[no_mangle]
pub extern "C" fn glyim_println_str(ptr: *const u8, len: i64) {
    if len > 0 && !ptr.is_null() {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        if let Ok(s) = std::str::from_utf8(bytes) {
            println!("{}", s);
        }
    }
}

#[no_mangle]
pub extern "C" fn glyim_assert_fail(msg: *const u8, len: i64) {
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(b"assertion failed: ");
    if len > 0 && !msg.is_null() {
        let bytes = unsafe { std::slice::from_raw_parts(msg, len as usize) };
        let _ = handle.write_all(bytes);
    }
    let _ = handle.write_all(b"\n");
    std::process::abort();
}
