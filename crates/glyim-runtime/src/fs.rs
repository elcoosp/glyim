//! File system FFI functions for the Glyim runtime.
//!
//! Provides low-level file operations used by generated code:
//! - File open, close, read, write, flush
//! - Directory creation and removal (single and recursive)
//! - File metadata, rename, truncate, canonicalize
//!
//! # File Descriptor Table
//!
//! Opened files are tracked in a process-global table mapping `i32` file
//! descriptors to `std::fs::File` handles. This allows generated code to
//! reference files by simple integer IDs rather than raw pointers.
//!
//! # Error Codes
//!
//! All functions return negative values on error. See the `FS_E*` constants.
//! Success is indicated by `FS_OK` (0) or a non-negative fd / byte count.

use libc;
use std::collections::HashMap;

/// # Safety — debug-only bounds sanity for FFI `(ptr, len)` slices (plan §3.2.2)
///
/// Every `extern "C"` FFI entry point that reconstructs a Rust slice from a raw
/// `(ptr, len)` pair is a potential OOB read/write if the codegen backend ever
/// emits a wrong length. These helpers centralize the slice reconstruction and
/// gate it behind `debug_assert!` checks that fire in debug builds (and are
/// compiled away in release). They reject:
/// - a non-null `ptr` with `len > isize::MAX` (would wrap/overflow `from_raw_parts`),
/// - the pathological `len == 0` with a dangling/non-null `ptr` is *allowed*
///   (zero-length slices are always safe), but a null `ptr` with non-zero `len`
///   is rejected up front by the callers already.
///
/// This is purely a defense-in-depth guard; the caller's `# Safety` contract
/// remains the source of truth. Reconstructing through these helpers means the
/// length mistake can only happen once (here), not in every hand-written FFI fn.
pub(crate) unsafe fn slice_from_raw_parts<'a>(
    ptr: *const u8,
    len: usize,
) -> &'a [u8] {
    debug_assert!(
        len <= isize::MAX as usize,
        "glyim runtime FFI: slice length {len} exceeds isize::MAX; \
         refusing to reconstruct a potentially-overflowing slice"
    );
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes of readable
    // memory. The debug_assert above guards the length against overflow.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

pub(crate) unsafe fn slice_from_raw_parts_mut<'a>(
    ptr: *mut u8,
    len: usize,
) -> &'a mut [u8] {
    debug_assert!(
        len <= isize::MAX as usize,
        "glyim runtime FFI: slice length {len} exceeds isize::MAX; \
         refusing to reconstruct a potentially-overflowing slice"
    );
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes of writable
    // memory. The debug_assert above guards the length against overflow.
    unsafe { std::slice::from_raw_parts_mut(ptr, len) }
}
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

/// Success.
pub const FS_OK: i32 = 0;
/// Generic I/O error.
pub const FS_EIO: i32 = -1;
/// File or path not found.
pub const FS_ENOENT: i32 = -2;
/// Permission denied.
pub const FS_EACCES: i32 = -3;
/// Invalid file descriptor.
pub const FS_EBADF: i32 = -4;
/// Path is not a directory.
pub const FS_ENOTDIR: i32 = -5;
/// Directory is not empty.
pub const FS_ENOTEMPTY: i32 = -6;
/// File already exists.
pub const FS_EEXIST: i32 = -7;
/// Buffer too small.
pub const FS_EOVERFLOW: i32 = -8;

// ---------------------------------------------------------------------------
// Open flags
// ---------------------------------------------------------------------------

/// Open for reading only (default if no access-mode bits are set).
pub const FS_O_RDONLY: u32 = 0;
/// Open for writing only.
pub const FS_O_WRONLY: u32 = 1;
/// Open for reading and writing.
pub const FS_O_RDWR: u32 = 2;
/// Append on each write.
pub const FS_O_APPEND: u32 = 4;
/// Create the file if it does not exist.
pub const FS_O_CREAT: u32 = 8;
/// Truncate the file to zero length if it exists.
pub const FS_O_TRUNC: u32 = 16;
/// Fail if the file already exists (must be used with FS_O_CREAT).
pub const FS_O_EXCL: u32 = 32;

// ---------------------------------------------------------------------------
// File descriptor table
// ---------------------------------------------------------------------------

struct FsTable {
    next_fd: i32,
    files: HashMap<i32, File>,
}

impl FsTable {
    fn new() -> Self {
        Self {
            next_fd: 0,
            files: HashMap::new(),
        }
    }

    /// Insert a file, returning its assigned descriptor.
    fn insert(&mut self, file: File) -> i32 {
        let fd = self.next_fd;
        // INVARIANT: next_fd wraps on overflow; a single process opening
        // 2^31 files is infeasible, so wrapping will not cause collisions
        // with live descriptors in practice.
        self.next_fd = self.next_fd.wrapping_add(1);
        self.files.insert(fd, file);
        fd
    }

    fn get_mut(&mut self, fd: i32) -> Option<&mut File> {
        self.files.get_mut(&fd)
    }

    fn remove(&mut self, fd: i32) -> Option<File> {
        self.files.remove(&fd)
    }
}

static FS_TABLE: OnceLock<Mutex<FsTable>> = OnceLock::new();

fn fs_table() -> &'static Mutex<FsTable> {
    FS_TABLE.get_or_init(|| Mutex::new(FsTable::new()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a raw byte pointer + length to an owned `PathBuf`.
///
/// Returns `None` if the pointer is null or the bytes are not valid UTF-8
/// (after the platform-specific fallback conversions). The result is owned so
/// callers do not need to keep any local borrow alive across the call.
///
/// # Safety
///
/// Caller must ensure `[ptr, ptr + len)` is a valid, readable memory region.
unsafe fn path_from_raw(ptr: *const u8, len: usize) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees [ptr, ptr + len) is valid readable memory.
    // The debug_assert in the helper rejects an overflowing length.
    let bytes = unsafe { slice_from_raw_parts(ptr, len) };

    #[cfg(unix)]
    {
        // On Unix, paths are typically UTF-8, but we use OsString to be safe.
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let os_str = OsStr::from_bytes(bytes);
        Some(PathBuf::from(os_str))
    }

    #[cfg(windows)]
    {
        // Glyim strings are UTF-8 at the language level (there is no UTF-16
        // string type in Glyim), so the bytes passed here are the caller's
        // `String`/`&str` bytes verbatim. Converting them via `OsString` is
        // exact and lossless for both Unicode and any surrogateadjacent
        // sequences that a prior lossy round-trip may have produced. There is
        // no separate "maybe this is actually UTF-16" branch: Glyim never
        // produces UTF-16 path bytes, and guessing an unrelated encoding for
        // malformed input silently manufactures a wrong path rather than
        // reporting the real problem. Truly invalid UTF-8 surfaces as `None`
        // so the caller (which already handles `None` by returning an I/O
        // error) is notified instead of receiving a silently wrong path.
        match std::str::from_utf8(bytes) {
            Ok(s) => Some(PathBuf::from(s)),
            Err(_) => None,
        }
    }
}

/// Map an `std::io::Error` to a Glyim errno code.
///
/// Handles both high-level `ErrorKind` variants and raw OS error codes
/// (on Unix) for cases not yet represented in `ErrorKind`.
fn io_err_to_errno(err: &std::io::Error) -> i32 {
    match err.kind() {
        std::io::ErrorKind::NotFound => FS_ENOENT,
        std::io::ErrorKind::PermissionDenied => FS_EACCES,
        std::io::ErrorKind::AlreadyExists => FS_EEXIST,
        std::io::ErrorKind::DirectoryNotEmpty => FS_ENOTEMPTY,
        std::io::ErrorKind::NotADirectory => FS_ENOTDIR,
        _ => {
            // Fallback: check raw OS error for cases not mapped to ErrorKind.
            // This handles platforms or error conditions where the Rust std
            // library does not yet provide a typed ErrorKind variant.
            if let Some(raw) = err.raw_os_error() {
                match raw {
                    #[cfg(unix)]
                    libc::ENOTDIR => FS_ENOTDIR,
                    #[cfg(unix)]
                    libc::ENOTEMPTY => FS_ENOTEMPTY,
                    _ => FS_EIO,
                }
            } else {
                FS_EIO
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FFI functions
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// FFI function macro (plan §3.3)
// ---------------------------------------------------------------------------
//
// Centralizes the repetitive boilerplate for FFI functions that reconstruct
// a Rust slice from a raw `(buf, buf_len)` pair, acquire the file-table lock,
// look up the file descriptor, and map `std::io::Result` to the Glyim
// `isize` error convention.
//
// Each arm takes the function name + signature and an I/O method identifier
// (`read` or `write`), and generates the full extern "C" function including
// null-pointer checks, debug_assertions on slice length, and the file-table
// lookup / error mapping. The method is invoked as `file.$ident(slice)` so
// macro hygiene never needs to reach into the caller's scope.

macro_rules! glyim_ffi_fn {
    // Read path: *mut u8 -> &mut [u8] via slice_from_raw_parts_mut
    (
        fn $name:ident(fd: i32, buf: *mut u8, buf_len: usize) -> isize
        $op:ident
     ) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(fd: i32, buf: *mut u8, buf_len: usize) -> isize {
            if buf.is_null() {
                return FS_EIO as isize;
            }
            // SAFETY: Caller guarantees buf points to writable memory of buf_len bytes.
            // The debug_assert inside the helper rejects an overflowing length.
            let slice = unsafe { slice_from_raw_parts_mut(buf, buf_len) };
            let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
            let file = match table.get_mut(fd) {
                Some(f) => f,
                None => return FS_EBADF as isize,
            };
            match file.$op(slice) {
                Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
                Err(e) => io_err_to_errno(&e) as isize,
            }
        }
    };

    // Write path: *const u8 -> &[u8] via slice_from_raw_parts
    (
        fn $name:ident(fd: i32, buf: *const u8, buf_len: usize) -> isize
        $op:ident
     ) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(fd: i32, buf: *const u8, buf_len: usize) -> isize {
            if buf.is_null() && buf_len > 0 {
                return FS_EIO as isize;
            }
            // SAFETY: Caller guarantees buf points to readable memory of buf_len bytes.
            // The debug_assert inside the helper rejects an overflowing length.
            let slice = unsafe { slice_from_raw_parts(buf as *const u8, buf_len) };
            let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
            let file = match table.get_mut(fd) {
                Some(f) => f,
                None => return FS_EBADF as isize,
            };
            match file.$op(slice) {
                Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
                Err(e) => io_err_to_errno(&e) as isize,
            }
        }
    };
}

// Cleanup function for process exit.
extern "C" fn cleanup_fs_table() {
    if let Some(table) = FS_TABLE.get()
        && let Ok(mut guard) = table.lock()
    {
        // Collect all fds to close.
        let fds: Vec<i32> = guard.files.keys().copied().collect();
        for fd in fds {
            // Close the file descriptor.
            // This drops the File, which closes the underlying OS handle.
            guard.files.remove(&fd);
        }
    }
}

// Register the cleanup handler once.
static REGISTERED: std::sync::Once = std::sync::Once::new();
fn register_cleanup() {
    REGISTERED.call_once(|| {
        // SAFETY: atexit is safe to call and the cleanup function is safe to call from any thread.
        unsafe {
            libc::atexit(cleanup_fs_table);
        }
    });
}

/// Open a file and return a file descriptor.
///
/// The `flags` parameter controls the open mode (see `FS_O_*` constants).
/// The lowest two bits encode the access mode: `FS_O_RDONLY` (0), `FS_O_WRONLY`
/// (1), or `FS_O_RDWR` (2). Additional bits for append, create, and truncate
/// may be OR'd in.
///
/// Returns a non-negative file descriptor on success, or a negative error
/// code on failure. The caller must eventually close the fd with
/// `glyim_fs_close` to release the resource.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_open(path: *const u8, path_len: usize, flags: u32) -> i32 {
    register_cleanup();

    // Validate flag combinations.
    let access = flags & 0x3;
    // Must specify exactly one of RDONLY, WRONLY, RDWR.
    if access != FS_O_RDONLY && access != FS_O_WRONLY && access != FS_O_RDWR {
        return FS_EIO;
    }
    // O_EXCL requires O_CREAT.
    if (flags & FS_O_EXCL) != 0 && (flags & FS_O_CREAT) == 0 {
        return FS_EIO;
    }
    // O_TRUNC requires write access (WRONLY or RDWR).
    if (flags & FS_O_TRUNC) != 0 && (access == FS_O_RDONLY) {
        return FS_EIO;
    }
    // O_APPEND requires write access.
    if (flags & FS_O_APPEND) != 0 && (access == FS_O_RDONLY) {
        return FS_EIO;
    }

    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };

    let access = flags & 0x3;
    let do_read = access != FS_O_WRONLY;
    let append = (flags & FS_O_APPEND) != 0;
    let create = (flags & FS_O_CREAT) != 0;
    let truncate = (flags & FS_O_TRUNC) != 0;
    let do_write = access != FS_O_RDONLY || append || create || truncate;

    let mut opts = OpenOptions::new();
    opts.read(do_read)
        .write(do_write)
        .append(append)
        .create(create)
        .truncate(truncate);

    match opts.open(p) {
        Ok(file) => {
            let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
            table.insert(file)
        }
        Err(e) => io_err_to_errno(&e),
    }
}

/// Close a file descriptor, releasing the underlying OS handle.
///
/// Returns `FS_OK` on success or `FS_EBADF` if the fd was not open.
///
/// # Safety
///
/// - `fd` must be a value previously returned by `glyim_fs_open` that has
///   not already been closed (double-close returns `FS_EBADF`)
#[unsafe(no_mangle)]
pub extern "C" fn glyim_fs_close(fd: i32) -> i32 {
    let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
    if table.remove(fd).is_some() {
        FS_OK
    } else {
        FS_EBADF
    }
}

/// Read up to `buf_len` bytes from the file into `buf`.
///
/// Returns the number of bytes read (0 indicates EOF), or a negative error
/// code on failure.
///
/// # Safety
///
/// - `fd` must be a valid file descriptor opened for reading
/// - `buf` must point to a writable buffer of at least `buf_len` bytes
glyim_ffi_fn! {
    fn glyim_fs_read(fd: i32, buf: *mut u8, buf_len: usize) -> isize
    read
}

/// Write `buf_len` bytes from `buf` to the file.
///
/// Returns the number of bytes written, or a negative error code on failure.
///
/// # Safety
///
/// - `fd` must be a valid file descriptor opened for writing
/// - `buf` must point to readable data of at least `buf_len` bytes
glyim_ffi_fn! {
    fn glyim_fs_write(fd: i32, buf: *const u8, buf_len: usize) -> isize
    write
}

/// Flush pending writes to the file.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `fd` must be a valid file descriptor opened for writing
#[unsafe(no_mangle)]
pub extern "C" fn glyim_fs_flush(fd: i32) -> i32 {
    let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
    let file = match table.get_mut(fd) {
        Some(f) => f,
        None => return FS_EBADF,
    };
    match file.flush() {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Get file metadata (size).
///
/// On success, writes the file size in bytes to `*out_size` and returns
/// `FS_OK`. On failure, returns a negative error code and does not modify
/// `*out_size`.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
/// - `out_size` must point to a valid, aligned `u64` writable location
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_metadata(
    path: *const u8,
    path_len: usize,
    out_size: *mut u64,
) -> i32 {
    if out_size.is_null() {
        return FS_EIO;
    }
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::metadata(p) {
        Ok(meta) => {
            // SAFETY: Caller guarantees out_size points to a valid u64.
            unsafe {
                *out_size = meta.len();
            }
            FS_OK
        }
        Err(e) => io_err_to_errno(&e),
    }
}

/// Truncate the file at `path` to the given `size` in bytes.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_truncate(path: *const u8, path_len: usize, size: u64) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::File::options().write(true).open(p) {
        Ok(file) => match file.set_len(size) {
            Ok(()) => FS_OK,
            Err(e) => io_err_to_errno(&e),
        },
        Err(e) => io_err_to_errno(&e),
    }
}

/// Rename a file or directory from `old_path` to `new_path`.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - Both path pointers must point to valid UTF-8 data of the given lengths
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_rename(
    old_path: *const u8,
    old_len: usize,
    new_path: *const u8,
    new_len: usize,
) -> i32 {
    let old = match unsafe { path_from_raw(old_path, old_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    let new = match unsafe { path_from_raw(new_path, new_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::rename(old, new) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Remove a file at the given path.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_remove_file(path: *const u8, path_len: usize) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::remove_file(p) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Remove an empty directory at the given path.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_remove_dir(path: *const u8, path_len: usize) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::remove_dir(p) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Remove a directory and all of its contents recursively.
///
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_remove_dir_all(path: *const u8, path_len: usize) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::remove_dir_all(p) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Create a directory at the given path.
///
/// Only creates the final component; the parent directory must already exist.
/// Returns `FS_OK` on success or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_create_dir(path: *const u8, path_len: usize) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::create_dir(p) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Create a directory and all parent components as needed.
///
/// Returns `FS_OK` on success (including if the directory already exists)
/// or a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_create_dir_all(path: *const u8, path_len: usize) -> i32 {
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO,
    };
    match fs::create_dir_all(p) {
        Ok(()) => FS_OK,
        Err(e) => io_err_to_errno(&e),
    }
}

/// Canonicalize a path, resolving symlinks and `.`/`..` components.
///
/// On success, writes the canonical UTF-8 path into `out_buf` (up to
/// `out_buf_len` bytes, **not** including a null terminator) and returns
/// the number of bytes written. On failure, returns a negative error code.
///
/// # Safety
///
/// - `path` must point to valid UTF-8 data of exactly `path_len` bytes
/// - `out_buf` must point to a writable buffer of at least `out_buf_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_fs_canonicalize(
    path: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> isize {
    if out_buf.is_null() && out_buf_len > 0 {
        return FS_EIO as isize;
    }
    let p = match unsafe { path_from_raw(path, path_len) } {
        Some(p) => p,
        None => return FS_EIO as isize,
    };
    match fs::canonicalize(p) {
        Ok(canonical) => {
            let bytes = match canonical.to_str() {
                Some(s) => s.as_bytes(),
                None => return FS_EIO as isize,
            };
            if bytes.len() > out_buf_len {
                return FS_EOVERFLOW as isize;
            }
            if !bytes.is_empty() {
                // SAFETY: Caller guarantees out_buf is writable and we
                // verified bytes.len() <= out_buf_len above.
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
                }
            }
            isize::try_from(bytes.len()).unwrap_or(isize::MAX)
        }
        Err(e) => io_err_to_errno(&e) as isize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan §3.2.2: the `(ptr, len)` slice helpers must reject a length that
    /// would overflow `isize` rather than handing it to `from_raw_parts`,
    /// which would otherwise produce a fatally mis-sized slice. This is a
    /// defense-in-depth guard against a codegen backend emitting a wrong
    /// length over the FFI boundary.
    #[test]
    #[should_panic(expected = "exceeds isize::MAX")]
    fn slice_from_raw_parts_rejects_overflow_len() {
        let overflow_len = (isize::MAX as usize).wrapping_add(1);
        // null ptr + overflow len: the debug_assert must fire before any
        // `from_raw_parts` reconstruction.
        let _ = unsafe { slice_from_raw_parts(std::ptr::null(), overflow_len) };
    }

    #[test]
    #[should_panic(expected = "exceeds isize::MAX")]
    fn slice_from_raw_parts_mut_rejects_overflow_len() {
        let overflow_len = (isize::MAX as usize).wrapping_add(1);
        let _ = unsafe { slice_from_raw_parts_mut(std::ptr::null_mut(), overflow_len) };
    }

    /// A zero-length slice from a *valid* (non-null, aligned) pointer is
    /// always safe and must NOT panic — the guard only rejects overflowing
    /// lengths. (A null pointer is never valid for `from_raw_parts`, even at
    /// len 0; that case is handled by the FFI callers' `is_null()` checks.)
    #[test]
    fn zero_len_slice_from_valid_ptr_is_safe() {
        let mut sentinel = 0u8;
        let s = unsafe { slice_from_raw_parts(&sentinel as *const u8, 0) };
        assert!(s.is_empty());
        let m = unsafe { slice_from_raw_parts_mut(&mut sentinel as *mut u8, 0) };
        assert!(m.is_empty());
    }
}
