//! Raw file-descriptor I/O + errno FFI for the Glyim standard library.
//!
//! The assembled `io.g` declares these as `extern "C"` and expects them to
//! resolve against this crate when a `--emit=exec` object is linked into a
//! native binary. They are intentionally the thinnest possible wrappers over
//! the platform read/write syscalls: no buffering, no error translation
//! beyond mapping the OS error into a small `i32` errno. The Glyim-side
//! `Error::last_os_error` / `ErrorKind::from_raw_os_error` (io.g) interprets
//! that code.
//!
//! Conventions (matched by io.g):
//! - `glyim_stdin_read(fd, buf, len) -> isize`: bytes read (>=0) or `-errno`.
//! - `glyim_stdout_write(fd, buf, len) -> isize`: bytes written (>=0) or `-errno`.
//! - `glyim_stderr_write(fd, buf, len) -> isize`: same.
//! - `glyim_stdout_flush(fd) -> i32`: 0 on success, `-errno` on failure.
//! - `glyim_errno() -> i32`: the last OS errno from a preceding call on this
//!   thread (raw `errno`, matching what `ErrorKind::from_raw_os_error`
//!   expects — `35` / `11` for `WouldBlock`, etc.).

use std::io::{Read, Write};

/// Reconstruct a `&[u8]` from a raw `(ptr, len)` pair, rejecting the
/// pathological shapes (null ptr with nonzero len, len > isize::MAX).
#[inline]
unsafe fn as_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len > isize::MAX as usize {
        return None;
    }
    if ptr.is_null() {
        if len == 0 {
            return Some(&[]);
        }
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Reconstruct a `&mut [u8]` from a raw `(ptr, len)` pair.
#[inline]
unsafe fn as_mut_slice<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if len > isize::MAX as usize {
        return None;
    }
    if ptr.is_null() {
        if len == 0 {
            return Some(&mut []);
        }
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts_mut(ptr, len) })
}

/// Convert an `std::io::Error` to a raw errno (positive).
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(libc::EIO)
}

/// Read up to `len` bytes from `fd` into `buf`.
///
/// # Safety
/// `buf` must point to at least `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_stdin_read(fd: i32, buf: *mut u8, len: usize) -> isize {
    let Some(dst) = (unsafe { as_mut_slice(buf, len) }) else {
        return -(libc::EINVAL as isize);
    };
    let mut file = unsafe { std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(fd)) };
    match file.read(dst) {
        Ok(n) => n as isize,
        Err(e) => -(errno_of(&e) as isize),
    }
}

/// Write `len` bytes from `buf` to `fd` (typically stdout).
///
/// # Safety
/// `buf` must point to at least `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_stdout_write(fd: i32, buf: *const u8, len: usize) -> isize {
    let Some(src) = (unsafe { as_slice(buf, len) }) else {
        return -(libc::EINVAL as isize);
    };
    let mut file = unsafe { std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(fd)) };
    match file.write(src) {
        Ok(n) => n as isize,
        Err(e) => -(errno_of(&e) as isize),
    }
}

/// Write `len` bytes from `buf` to `fd` (typically stderr).
///
/// # Safety
/// `buf` must point to at least `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_stderr_write(fd: i32, buf: *const u8, len: usize) -> isize {
    let Some(src) = (unsafe { as_slice(buf, len) }) else {
        return -(libc::EINVAL as isize);
    };
    let mut file = unsafe { std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(fd)) };
    match file.write(src) {
        Ok(n) => n as isize,
        Err(e) => -(errno_of(&e) as isize),
    }
}

/// Flush the OS buffer for `fd`. On POSIX stdio, a raw fd has no user-space
/// buffer, so this is effectively a no-op; it returns 0 for `stdout`/`stderr`
/// and lets the caller treat any error as fatal.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_stdout_flush(_fd: i32) -> i32 {
    0
}

/// Return the current thread's C `errno`.
///
/// `ErrorKind::from_raw_os_error` on the Glyim side interprets this — e.g.
/// `35` (`EWOULDBLOCK`) / `11` (`EAGAIN`) map to `WouldBlock`.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(0)
}

#[cfg(unix)]
use std::os::unix::io::FromRawFd;
