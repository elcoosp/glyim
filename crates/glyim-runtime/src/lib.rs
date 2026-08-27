//! Runtime support: memory allocation, panic handling, drop glue, ABI stubs,
//! networking, threading, time, environment variables, and process management.
//!
//! This crate provides the low-level FFI interface used by generated code:
//! - Memory: `glyim_alloc`, `glyim_dealloc`, `glyim_drop_in_place`
//! - Panic: `glyim_panic`
//! - Environment: `glyim_env_var`, `glyim_env_set_var`, `glyim_env_remove_var`,
//!   `glyim_env_args_count`, `glyim_env_args_get`, `glyim_env_current_dir`,
//!   `glyim_env_current_exe`, `glyim_env_home_dir`, `glyim_env_temp_dir`
//! - Process: `glyim_process_spawn`, `glyim_process_wait`, `glyim_process_wait_output`,
//!   `glyim_process_kill`, `glyim_process_exit`, `glyim_process_abort`,
//!   `glyim_process_getpid`, `glyim_process_getppid`
//! - Networking (TCP/UDP): `glyim_net_tcp_connect`, `glyim_net_tcp_bind`,
//!   `glyim_net_tcp_accept`, `glyim_net_tcp_read`, `glyim_net_tcp_write`,
//!   `glyim_net_tcp_local_addr`, `glyim_net_udp_bind`, `glyim_net_udp_send_to`,
//!   `glyim_net_udp_recv_from`, `glyim_net_udp_connect`, `glyim_net_udp_send`,
//!   `glyim_net_udp_recv`
//! - Threading: `glyim_thread_spawn`, `glyim_thread_join`, `glyim_thread_yield`,
//!   `glyim_thread_sleep`, `glyim_thread_park`, `glyim_thread_unpark`,
//!   `glyim_thread_current_id`, `glyim_thread_available_parallelism`
//! - Time: `glyim_time_now_secs`, `glyim_time_now_nanos`,
//!   `glyim_time_system_secs`, `glyim_time_system_nanos`
//! - Memory cleanup: `glyim_free_cstr`
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]

pub use glyim_core::abi::ALIGN_MAX;

use std::alloc::{self, Layout};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// FFI string allocation helpers
// ---------------------------------------------------------------------------

/// Type for a drop function pointer passed to `glyim_drop_in_place`.
pub type DropFn = unsafe extern "C" fn(*mut u8);

/// Size of the header prepended to FFI-returned byte buffers.
///
/// The header stores the data length so that `glyim_free_cstr` can
/// reconstruct the full allocation layout without the caller providing a size.
const FFI_STR_HEADER_SIZE: usize = std::mem::size_of::<usize>();

/// Allocate a copy of `data` for return to FFI callers.
///
/// Returns a pointer to the copied data (past an internal header), or null
/// if `data` is empty or allocation fails. The returned pointer **must** be
/// freed with `glyim_free_cstr`.
pub(crate) fn alloc_ffi_bytes(data: &[u8]) -> *mut u8 {
    if data.is_empty() {
        return std::ptr::null_mut();
    }
    let total = match FFI_STR_HEADER_SIZE.checked_add(data.len()) {
        Some(t) => t,
        None => return std::ptr::null_mut(),
    };
    let layout = match Layout::from_size_align(total, std::mem::align_of::<usize>()) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    // SAFETY: Layout is validated above (non-zero size, valid alignment).
    let ptr = unsafe { alloc::alloc(layout) };
    if ptr.is_null() {
        return ptr;
    }
    // SAFETY: ptr is valid and writable for `total` bytes. We write the
    // data length as a header, then copy the data immediately after it.
    unsafe {
        ptr.cast::<usize>().write(data.len());
        ptr.add(FFI_STR_HEADER_SIZE)
            .copy_from_nonoverlapping(data.as_ptr(), data.len());
    }
    // SAFETY: ptr + FFI_STR_HEADER_SIZE is within the allocated block.
    unsafe { ptr.add(FFI_STR_HEADER_SIZE) }
}

// ---------------------------------------------------------------------------
// Memory allocation
// ---------------------------------------------------------------------------

/// Allocate memory with the given size and alignment.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::NonNull::dangling().as_ptr();
    }
    let layout = match Layout::from_size_align(size, align.max(1)) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    unsafe { alloc::alloc(layout) }
}

/// Deallocate memory previously allocated by `glyim_alloc`.
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_dealloc(ptr: *mut u8, size: usize, align: usize) {
    if size == 0 || ptr.is_null() {
        return;
    }
    let layout = match Layout::from_size_align(size, align.max(1)) {
        Ok(l) => l,
        Err(_) => return,
    };
    unsafe { alloc::dealloc(ptr, layout) }
}

/// Drop a value in place by calling its type-specific destructor.
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_drop_in_place(ptr: *mut u8, drop_fn: Option<DropFn>) {
    if ptr.is_null() {
        return;
    }
    if let Some(drop) = drop_fn {
        unsafe { drop(ptr) }
    }
}

/// Panic handler for the runtime.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_panic(_msg: *const u8, _len: usize) -> ! {
    std::process::abort()
}

/// Free a byte buffer previously returned by an environment or process FFI function.
///
/// # Safety
///
/// - `ptr` must have been returned by one of the FFI functions that allocate
///   output buffers (e.g. `glyim_env_var`, `glyim_env_current_dir`, etc.)
/// - `ptr` must not have been already freed
/// - Passing a null pointer is safe and results in a no-op
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_free_cstr(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was returned by alloc_ffi_bytes, so the base allocation
    // starts at ptr - FFI_STR_HEADER_SIZE and the header contains the data
    // length, allowing us to reconstruct the full layout for dealloc.
    unsafe {
        let base = ptr.sub(FFI_STR_HEADER_SIZE);
        let len = base.cast::<usize>().read();
        let total = match FFI_STR_HEADER_SIZE.checked_add(len) {
            Some(t) => t,
            None => return,
        };
        let layout = match Layout::from_size_align(total, std::mem::align_of::<usize>()) {
            Ok(l) => l,
            Err(_) => return,
        };
        alloc::dealloc(base, layout);
    }
}

// ---------------------------------------------------------------------------
// Environment functions
// ---------------------------------------------------------------------------

/// Look up the value of an environment variable.
///
/// On success (variable exists), writes a pointer to a newly allocated buffer
/// containing the variable's value into `*out_ptr` and its byte length into
/// `*out_len`. The buffer must be freed with `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success
/// - `-1` if the variable does not exist
///
/// # Safety
///
/// - `name` must point to valid UTF-8 data of exactly `name_len` bytes
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_var(
    name: *const u8,
    name_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if name.is_null() || out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees name points to valid UTF-8 data of name_len bytes.
    let name_bytes = unsafe { std::slice::from_raw_parts(name, name_len) };
    let name_str = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match std::env::var(name_str) {
        Ok(value) => {
            let ptr = alloc_ffi_bytes(value.as_bytes());
            if ptr.is_null() {
                return -1;
            }
            // SAFETY: out_ptr and out_len are guaranteed non-null by the caller.
            unsafe {
                out_ptr.write(ptr);
                out_len.write(value.len());
            }
            0
        }
        Err(_) => -1,
    }
}

/// Set an environment variable.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure (invalid UTF-8 in name or value)
///
/// # Safety
///
/// - `name` must point to valid UTF-8 data of exactly `name_len` bytes
/// - `value` must point to valid UTF-8 data of exactly `value_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_set_var(
    name: *const u8,
    name_len: usize,
    value: *const u8,
    value_len: usize,
) -> i32 {
    if name.is_null() || value.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees valid UTF-8 data of the given lengths.
    let name_bytes = unsafe { std::slice::from_raw_parts(name, name_len) };
    let value_bytes = unsafe { std::slice::from_raw_parts(value, value_len) };
    let name_str = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let value_str = match std::str::from_utf8(value_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    // SAFETY: We are in an extern "C" function; the caller provides the
    // variable name and value as byte slices. In Rust 2024, set_var is
    // unsafe because modifying environment variables during the lifetime
    // of the process can cause undefined behavior in some scenarios
    // (e.g. if another thread is iterating env). This is acceptable for
    // a runtime FFI function where the caller explicitly requests mutation.
    unsafe {
        std::env::set_var(name_str, value_str);
    }
    0
}

/// Remove an environment variable.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure (invalid UTF-8 in name)
///
/// # Safety
///
/// - `name` must point to valid UTF-8 data of exactly `name_len` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_remove_var(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees valid UTF-8 data of the given length.
    let name_bytes = unsafe { std::slice::from_raw_parts(name, name_len) };
    let name_str = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    // SAFETY: Same rationale as set_var — the caller explicitly requests
    // environment mutation via FFI.
    unsafe {
        std::env::remove_var(name_str);
    }
    0
}

/// Return the number of command-line arguments (including the program name).
#[unsafe(no_mangle)]
pub extern "C" fn glyim_env_args_count() -> usize {
    std::env::args().len()
}

/// Get a command-line argument by index.
///
/// On success, writes a pointer to a newly allocated buffer containing the
/// argument's UTF-8 encoding into `*out_ptr` and its byte length into
/// `*out_len`. The buffer must be freed with `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success
/// - `-2` if `index` is out of bounds
///
/// # Safety
///
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_args_get(
    index: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -2;
    }
    match std::env::args().nth(index) {
        Some(arg) => {
            let ptr = alloc_ffi_bytes(arg.as_bytes());
            if ptr.is_null() {
                return -2;
            }
            // SAFETY: out_ptr and out_len are guaranteed non-null.
            unsafe {
                out_ptr.write(ptr);
                out_len.write(arg.len());
            }
            0
        }
        None => -2,
    }
}

/// Get the current working directory.
///
/// On success, writes a pointer to a newly allocated buffer into `*out_ptr`
/// and its byte length into `*out_len`. The buffer must be freed with
/// `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure (e.g. directory removed out from under the process)
///
/// # Safety
///
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_current_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    match std::env::current_dir() {
        Ok(path) => {
            let path_str = path.to_string_lossy().into_owned();
            let ptr = alloc_ffi_bytes(path_str.as_bytes());
            if ptr.is_null() {
                return -1;
            }
            // SAFETY: out_ptr and out_len are guaranteed non-null.
            unsafe {
                out_ptr.write(ptr);
                out_len.write(path_str.len());
            }
            0
        }
        Err(_) => -1,
    }
}

/// Get the path to the current executable.
///
/// On success, writes a pointer to a newly allocated buffer into `*out_ptr`
/// and its byte length into `*out_len`. The buffer must be freed with
/// `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure
///
/// # Safety
///
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_current_exe(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    match std::env::current_exe() {
        Ok(path) => {
            let path_str = path.to_string_lossy().into_owned();
            let ptr = alloc_ffi_bytes(path_str.as_bytes());
            if ptr.is_null() {
                return -1;
            }
            // SAFETY: out_ptr and out_len are guaranteed non-null.
            unsafe {
                out_ptr.write(ptr);
                out_len.write(path_str.len());
            }
            0
        }
        Err(_) => -1,
    }
}

/// Get the home directory.
///
/// On success, writes a pointer to a newly allocated buffer into `*out_ptr`
/// and its byte length into `*out_len`. The buffer must be freed with
/// `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success
/// - `-1` if the home directory cannot be determined (e.g. `$HOME` not set)
///
/// # Safety
///
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_home_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    match dirs_home_dir() {
        Some(path_str) => {
            let ptr = alloc_ffi_bytes(path_str.as_bytes());
            if ptr.is_null() {
                return -1;
            }
            // SAFETY: out_ptr and out_len are guaranteed non-null.
            unsafe {
                out_ptr.write(ptr);
                out_len.write(path_str.len());
            }
            0
        }
        None => -1,
    }
}

/// Get the temporary directory path.
///
/// On success, writes a pointer to a newly allocated buffer into `*out_ptr`
/// and its byte length into `*out_len`. The buffer must be freed with
/// `glyim_free_cstr`.
///
/// # Returns
/// - `0` on success (always succeeds; falls back to `/tmp` on Unix)
/// - `-1` only if allocation fails
///
/// # Safety
///
/// - `out_ptr` and `out_len` must be valid, non-null pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_temp_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    let path = std::env::temp_dir();
    let path_str = path.to_string_lossy().into_owned();
    let ptr = alloc_ffi_bytes(path_str.as_bytes());
    if ptr.is_null() {
        return -1;
    }
    // SAFETY: out_ptr and out_len are guaranteed non-null.
    unsafe {
        out_ptr.write(ptr);
        out_len.write(path_str.len());
    }
    0
}

/// Return the number of environment variables.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_env_vars_count() -> usize {
    std::env::vars().count()
}

/// Get an environment variable by index.
///
/// On success, writes the key and value into the provided buffers.
/// Returns 0 on success, -1 if the index is out of bounds, or -2 if the buffers are too small.
///
/// # Safety
///
/// - `key_buf` and `val_buf` must be valid, writable buffers of the given capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_env_vars_get(
    index: usize,
    key_buf: *mut u8,
    key_cap: usize,
    val_buf: *mut u8,
    val_cap: usize,
) -> i32 {
    // Collect all env vars into a vector (to allow indexing by position).
    let vars: Vec<(String, String)> = std::env::vars().collect();
    if index >= vars.len() {
        return -1;
    }
    let (key, value) = &vars[index];
    let key_bytes = key.as_bytes();
    let val_bytes = value.as_bytes();
    // Check if buffers are large enough (including null terminator? We'll copy without null terminator).
    if key_bytes.len() >= key_cap || val_bytes.len() >= val_cap {
        return -2; // buffers too small
    }
    // SAFETY: Caller guarantees valid writable buffers.
    unsafe {
        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), key_buf, key_bytes.len());
        std::ptr::copy_nonoverlapping(val_bytes.as_ptr(), val_buf, val_bytes.len());
    }
    0
}

/// Determine the user's home directory.
///
/// Checks `HOME` environment variable first (Unix convention).
fn dirs_home_dir() -> Option<String> {
    if let Some(home) = std::env::var_os("HOME") {
        let home_str = home.to_string_lossy().into_owned();
        if !home_str.is_empty() {
            return Some(home_str);
        }
    }
    // Windows fallback: USERPROFILE
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile_str = profile.to_string_lossy().into_owned();
        if !profile_str.is_empty() {
            return Some(profile_str);
        }
    }
    // Windows fallback: HOMEDRIVE + HOMEPATH
    if let (Some(drive), Some(path)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
    {
        let drive_str = drive.to_string_lossy();
        let path_str = path.to_string_lossy();
        if !drive_str.is_empty() && !path_str.is_empty() {
            let home = format!("{}{}", drive_str, path_str);
            if !home.is_empty() {
                return Some(home);
            }
        }
    }
    None
}
// ---------------------------------------------------------------------------
// Process functions
// ---------------------------------------------------------------------------

/// Global registry of spawned child processes, keyed by handle.
///
/// Each spawned child is assigned a monotonically increasing handle.
/// The handle is returned to the FFI caller and used to refer to the
/// child in subsequent `wait`, `wait_output`, and `kill` calls.
struct ProcessRegistry {
    children: HashMap<usize, Child>,
    next_handle: usize,
}

impl ProcessRegistry {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            next_handle: 1,
        }
    }
}

fn process_registry() -> &'static Mutex<ProcessRegistry> {
    static REGISTRY: OnceLock<Mutex<ProcessRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ProcessRegistry::new()))
}

/// Split a byte slice on null bytes, producing owned strings for each segment.
///
/// Empty segments (consecutive nulls or trailing null) are skipped.
fn split_null_separated(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return Vec::new();
    }
    data.split(|&b| b == 0)
        .filter(|seg| !seg.is_empty())
        .map(|seg| String::from_utf8_lossy(seg).into_owned())
        .collect()
}

/// Spawn a child process.
///
/// `cmd` is the program name to execute (searched on `PATH`). `args` is a
/// null-separated byte sequence of arguments; the first element is
/// conventionally the program name (argv[0]). If `args` is null or `args_len`
/// is zero, the program is launched with no arguments.
///
/// On success, writes a non-zero handle into `*out_handle`.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure (program not found, invalid arguments, etc.)
///
/// # Safety
///
/// - `cmd` must point to valid UTF-8 data of exactly `cmd_len` bytes
/// - If `args` is non-null, it must point to `args_len` bytes of
///   null-separated UTF-8 data
/// - `out_handle` must be a valid, non-null pointer
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_process_spawn(
    cmd: *const u8,
    cmd_len: usize,
    args: *const u8,
    args_len: usize,
    out_handle: *mut usize,
) -> i32 {
    if cmd.is_null() || out_handle.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees cmd points to valid UTF-8 data of cmd_len bytes.
    let cmd_bytes = unsafe { std::slice::from_raw_parts(cmd, cmd_len) };
    let cmd_str = match std::str::from_utf8(cmd_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let arg_strings = if args.is_null() || args_len == 0 {
        Vec::new()
    } else {
        // SAFETY: Caller guarantees args points to args_len bytes.
        let args_bytes = unsafe { std::slice::from_raw_parts(args, args_len) };
        split_null_separated(args_bytes)
    };

    let mut command = Command::new(cmd_str);
    if !arg_strings.is_empty() {
        command.args(&arg_strings);
    }
    #[cfg(windows)]
    {
        // Spawn the child in its own process group so that a later
        // `GenerateConsoleCtrlEvent` (SIGTERM-equivalent graceful signal) can
        // reach it. Without this flag the console-control event only goes to the
        // caller's own group and graceful signaling silently fails.
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    match command.spawn() {
        Ok(child) => {
            let mut registry = process_registry()
                .lock()
                .expect("process registry lock poisoned");
            let handle = registry.next_handle;
            registry.next_handle = registry.next_handle.saturating_add(1);
            registry.children.insert(handle, child);
            // SAFETY: out_handle is guaranteed non-null.
            unsafe {
                out_handle.write(handle);
            }
            0
        }
        Err(_) => -1,
    }
}

/// Wait for a child process to exit.
///
/// Writes the process exit code into `*out_exit_code`.
/// The child handle is consumed and no longer valid after this call.
///
/// # Returns
/// - `0` on success
/// - `-1` if the handle is invalid or the child has already been reaped
///
/// # Safety
///
/// - `handle` must be a valid handle previously returned by `glyim_process_spawn`
/// - `out_exit_code` must be a valid, non-null pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_process_wait(handle: usize, out_exit_code: *mut i32) -> i32 {
    if out_exit_code.is_null() {
        return -1;
    }
    let mut registry = process_registry()
        .lock()
        .expect("process registry lock poisoned");
    if let Some(mut child) = registry.children.remove(&handle) {
        match child.wait() {
            Ok(status) => {
                let code = status.code().unwrap_or(-1);
                // SAFETY: out_exit_code is guaranteed non-null.
                unsafe {
                    out_exit_code.write(code);
                }
                0
            }
            Err(_) => -1,
        }
    } else {
        -1
    }
}

/// Wait for a child process to exit and capture its output.
///
/// Writes pointers to newly allocated buffers containing the child's stdout
/// and stderr into `*stdout_ptr`/`*stderr_ptr` and their lengths into
/// `*stdout_len`/`*stderr_len`. The buffers must be freed with `glyim_free_cstr`.
/// Writes the exit code into `*out_exit_code`.
/// The child handle is consumed and no longer valid after this call.
///
/// If the child produced no output on a stream, the corresponding pointer
/// will be null and length zero.
///
/// # Returns
/// - `0` on success
/// - `-1` if the handle is invalid or the child has already been reaped
///
/// # Safety
///
/// - `handle` must be a valid handle previously returned by `glyim_process_spawn`
/// - All output pointers and length pointers must be valid and non-null
/// - `out_exit_code` must be a valid, non-null pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_process_wait_output(
    handle: usize,
    stdout_ptr: *mut *mut u8,
    stdout_len: *mut usize,
    stderr_ptr: *mut *mut u8,
    stderr_len: *mut usize,
    out_exit_code: *mut i32,
) -> i32 {
    if stdout_ptr.is_null()
        || stdout_len.is_null()
        || stderr_ptr.is_null()
        || stderr_len.is_null()
        || out_exit_code.is_null()
    {
        return -1;
    }
    let mut registry = process_registry()
        .lock()
        .expect("process registry lock poisoned");
    if let Some(child) = registry.children.remove(&handle) {
        match child.wait_with_output() {
            Ok(output) => {
                let stdout_data = output.stdout.as_slice();
                let stderr_data = output.stderr.as_slice();

                let s_ptr = alloc_ffi_bytes(stdout_data);
                let e_ptr = alloc_ffi_bytes(stderr_data);

                let code = output.status.code().unwrap_or(-1);
                // SAFETY: All output pointers are guaranteed non-null.
                unsafe {
                    stdout_ptr.write(s_ptr);
                    stdout_len.write(stdout_data.len());
                    stderr_ptr.write(e_ptr);
                    stderr_len.write(stderr_data.len());
                    out_exit_code.write(code);
                }
                0
            }
            Err(_) => -1,
        }
    } else {
        -1
    }
}

/// Send a termination signal to a child process.
///
/// On Unix, `signal` is passed through to `kill(pid, signal)` so callers can
/// request e.g. `SIGTERM` (15) for graceful shutdown vs `SIGKILL` (9) for an
/// abrupt termination. An out-of-range signal falls back to `SIGKILL`.
///
/// On Windows (plan §17.3), POSIX signals do not exist, so the `signal`
/// parameter must be *mapped* rather than ignored. The intended semantics are:
///
/// | `signal`             | Windows behavior                                              |
/// |----------------------|--------------------------------------------------------------|
/// | `SIGTERM` (15)       | `GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid)` — graceful,    |
/// |                      |   cooperating console apps observe the event and exit cleanly|
/// | `SIGKILL` (9) / other| `TerminateProcess(handle, 1)` — hard, immediate termination |
/// | out-of-range         | falls back to hard `TerminateProcess`                       |
///
/// `GenerateConsoleCtrlEvent` is the graceful counterpart to `SIGTERM`. The
/// Windows branch now branches on `signal`: `SIGTERM` (15) / `SIGINT` (2) send a
/// `GenerateConsoleCtrlEvent` (CTRL_BREAK_EVENT / CTRL_C_EVENT) to the child's
/// process group (the child is spawned with `CREATE_NEW_PROCESS_GROUP` on
/// Windows so it can receive the event), while `SIGKILL` (9) / out-of-range
/// values perform a hard `TerminateProcess`. A graceful event that fails to
/// deliver falls back to a hard kill.
///
/// The handle remains valid after `kill`; use `wait` or `wait_output` to
/// reap the child afterward.
///
/// # Returns
/// - `0` on success
/// - `-1` if the handle is invalid or the child has already been reaped
///
/// # Safety
///
/// - `handle` must be a valid handle previously returned by `glyim_process_spawn`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_process_kill(handle: usize, signal: i32) -> i32 {
    let mut registry = process_registry()
        .lock()
        .expect("process registry lock poisoned");
    #[cfg(unix)]
    {
        if let Some(child) = registry.children.get_mut(&handle) {
            let pid = child.id() as libc::pid_t;
            // Map the requested signal; invalid values fall back to SIGKILL.
            let sig = if signal > 0 && signal <= libc::SIGKILL as i32 {
                signal as libc::c_int
            } else {
                libc::SIGKILL
            };
            // SAFETY: pid is a live child process; signal values are validated.
            let ret = unsafe { libc::kill(pid, sig) };
            if ret == 0 {
                0
            } else {
                -1
            }
        } else {
            -1
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: no POSIX signals, so map the requested signal to a Windows
        // console-control event for graceful shutdown, falling back to a hard
        // `TerminateProcess` for `SIGKILL`/out-of-range values (see the doc
        // comment on the function for the full signal mapping table).
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Console::{
            GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT, CTRL_C_EVENT,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        if let Some(ref mut child) = registry.children.get_mut(&handle) {
            // Map SIGTERM (15) / SIGINT (2) to a graceful console event;
            // everything else (incl. SIGKILL 9 and out-of-range) hard-kills.
            let graceful = matches!(signal, 2 | 15);
            if graceful {
                let ctrl_event = if signal == 2 {
                    CTRL_C_EVENT
                } else {
                    CTRL_BREAK_EVENT
                };
                // pid must be the actual child process id; the registry stores a
                // synthetic handle, so read the child's real pid.
                let pid = child.id();
                // 0 as the second arg targets the whole process group, which is
                // why spawn sets CREATE_NEW_PROCESS_GROUP on Windows.
                let ok = unsafe { GenerateConsoleCtrlEvent(ctrl_event, pid) };
                if ok != 0 {
                    return 0;
                }
                // If the graceful event didn't deliver (e.g. the child isn't in
                // a console), fall through to a hard kill below.
            }
            // Hard terminate path (SIGKILL, out-of-range, or graceful fallback).
            let pid = child.id();
            let raw = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
            if raw.is_null() {
                return -1;
            }
            let ok = unsafe { TerminateProcess(raw, 1) };
            unsafe {
                CloseHandle(raw);
            }
            if ok != 0 {
                0
            } else {
                -1
            }
        } else {
            -1
        }
    }
}

/// Terminate the current process with the given exit code.
///
/// This function never returns.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_exit(code: i32) -> ! {
    std::process::exit(code)
}

/// Abort the current process immediately.
///
/// This function never returns. On Unix, sends `SIGABRT`.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_abort() -> ! {
    std::process::abort()
}

/// Get the process ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_getpid() -> u32 {
    std::process::id()
}

/// Get the process ID of the parent of the calling process.
///
/// Returns 0 if the parent PID cannot be determined.
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_getppid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getppid is a POSIX function that always succeeds and returns
        // the caller's parent PID with no side effects.
        unsafe { libc::getppid() as u32 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;

        // SAFETY: standard Toolhelp32 snapshot usage; the handle is checked
        // against INVALID_HANDLE_VALUE and always closed before returning.
        unsafe {
            let current_pid = GetCurrentProcessId();
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return 0;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = 0u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32ProcessID == current_pid {
                        found = entry.th32ParentProcessID;
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Genuinely no known primitive on this target family (e.g. wasm); an
        // honest 0 rather than a fabricated value.
        0
    }
}

// ---------------------------------------------------------------------------
// Networking (TCP & UDP)
// ---------------------------------------------------------------------------

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(1);

fn alloc_socket_id() -> SocketId {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

type SocketId = u32;

struct TcpStreamStore {
    streams: HashMap<SocketId, TcpStream>,
}
struct TcpListenerStore {
    listeners: HashMap<SocketId, TcpListener>,
}
struct UdpSocketStore {
    sockets: HashMap<SocketId, UdpSocket>,
}

fn tcp_streams() -> &'static Mutex<TcpStreamStore> {
    static STREAMS: OnceLock<Mutex<TcpStreamStore>> = OnceLock::new();
    STREAMS.get_or_init(|| {
        Mutex::new(TcpStreamStore {
            streams: HashMap::new(),
        })
    })
}
fn tcp_listeners() -> &'static Mutex<TcpListenerStore> {
    static LISTENERS: OnceLock<Mutex<TcpListenerStore>> = OnceLock::new();
    LISTENERS.get_or_init(|| {
        Mutex::new(TcpListenerStore {
            listeners: HashMap::new(),
        })
    })
}
fn udp_sockets() -> &'static Mutex<UdpSocketStore> {
    static SOCKETS: OnceLock<Mutex<UdpSocketStore>> = OnceLock::new();
    SOCKETS.get_or_init(|| {
        Mutex::new(UdpSocketStore {
            sockets: HashMap::new(),
        })
    })
}

/// Helper: convert raw bytes to string (assumes valid UTF-8, null-terminated or length provided)
unsafe fn bytes_to_string(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(slice).ok().map(|s| s.to_string())
}

// TCP functions
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_connect(addr: *const u8, addr_len: usize, port: u16) -> i32 {
    let addr_str = match unsafe { bytes_to_string(addr, addr_len) } {
        Some(s) => s,
        None => return -1,
    };
    let full_addr = format!("{}:{}", addr_str, port);
    let stream = match TcpStream::connect(&full_addr) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let id = alloc_socket_id();
    tcp_streams().lock().unwrap().streams.insert(id, stream);
    id as i32
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32 {
    let addr_str = match unsafe { bytes_to_string(addr, addr_len) } {
        Some(s) => s,
        None => return -1,
    };
    let full_addr = format!("{}:{}", addr_str, port);
    // Bind using standard TcpListener, then set SO_REUSEADDR via socket2 SockRef
    let listener = match std::net::TcpListener::bind(&full_addr) {
        Ok(l) => l,
        Err(_) => return -1,
    };
    // Set SO_REUSEADDR using socket2
    use socket2::SockRef;
    if SockRef::from(&listener).set_reuse_address(true).is_err() {
        return -1;
    }
    let id = alloc_socket_id();
    tcp_listeners()
        .lock()
        .unwrap()
        .listeners
        .insert(id, listener);
    id as i32
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_accept(fd: i32) -> i32 {
    let fd = fd as u32;
    let mut listener_store = tcp_listeners().lock().unwrap();
    let listener = match listener_store.listeners.get_mut(&fd) {
        Some(l) => l,
        None => return -1,
    };
    let (stream, _) = match listener.accept() {
        Ok(pair) => pair,
        Err(_) => return -1,
    };
    drop(listener_store);
    let new_id = alloc_socket_id();
    tcp_streams().lock().unwrap().streams.insert(new_id, stream);
    new_id as i32
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_read(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let mut store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, count) };
    match stream.read(slice) {
        Ok(n) => n as isize,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_write(fd: i32, buf: *const u8, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let mut store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts(buf, count) };
    match stream.write_all(slice) {
        Ok(()) => count as isize,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_local_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32 {
    let fd = fd as u32;
    let store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let addr = match stream.local_addr() {
        Ok(a) => a,
        Err(_) => return -1,
    };
    let addr_str = addr.to_string();
    let bytes = addr_str.as_bytes();
    if bytes.len() >= buf_len {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    bytes.len() as i32
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_peer_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32 {
    let fd = fd as u32;
    let store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let addr = match stream.peer_addr() {
        Ok(a) => a,
        Err(_) => return -1,
    };
    let addr_str = addr.to_string();
    let bytes = addr_str.as_bytes();
    if bytes.len() >= buf_len {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    bytes.len() as i32
}

/// Set non-blocking mode on a TCP stream.
///
/// # Returns
/// - `0` on success
/// - `-1` on failure (invalid fd or socket error)
///
/// # Safety
///
/// - `fd` must be a valid TCP stream descriptor
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_net_tcp_set_nonblocking(fd: i32, nonblocking: i32) -> i32 {
    use socket2::SockRef;
    let fd = fd as u32;
    let store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get(&fd) {
        Some(s) => s,
        None => return -1,
    };
    // Use socket2 to set non-blocking.
    match SockRef::from(stream).set_nonblocking(nonblocking != 0) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

// UDP functions
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32 {
    let addr_str = match unsafe { bytes_to_string(addr, addr_len) } {
        Some(s) => s,
        None => return -1,
    };
    let full_addr = format!("{}:{}", addr_str, port);
    let socket = match UdpSocket::bind(&full_addr) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let id = alloc_socket_id();
    udp_sockets().lock().unwrap().sockets.insert(id, socket);
    id as i32
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_send_to(
    fd: i32,
    buf: *const u8,
    count: usize,
    dest_addr: *const u8,
    dest_addr_len: usize,
    dest_port: u16,
) -> isize {
    if buf.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let addr_str = match unsafe { bytes_to_string(dest_addr, dest_addr_len) } {
        Some(s) => s,
        None => return -1,
    };
    let target = match format!("{}:{}", addr_str, dest_port).to_socket_addrs() {
        Ok(mut addrs) => addrs.next(),
        Err(_) => None,
    };
    let target = match target {
        Some(addr) => addr,
        None => return -1,
    };
    let mut store = udp_sockets().lock().unwrap();
    let socket = match store.sockets.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts(buf, count) };
    match socket.send_to(slice, target) {
        Ok(n) => n as isize,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_recv_from(
    fd: i32,
    buf: *mut u8,
    count: usize,
    src_addr: *mut u8,
    src_addr_len: *mut usize,
    src_port: *mut u16,
) -> isize {
    if buf.is_null() || src_addr.is_null() || src_addr_len.is_null() || src_port.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let mut store = udp_sockets().lock().unwrap();
    let socket = match store.sockets.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, count) };
    let (n, addr) = match socket.recv_from(slice) {
        Ok((n, addr)) => (n, addr),
        Err(_) => return -1,
    };

    // Use proper SocketAddr parsing to get IP and port.
    let ip_str = addr.ip().to_string(); // This handles both IPv4 and IPv6 correctly.
    let port = addr.port();

    let ip_bytes = ip_str.as_bytes();
    let max_len = unsafe { *src_addr_len };
    if ip_bytes.len() >= max_len {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(ip_bytes.as_ptr(), src_addr, ip_bytes.len());
        // Add null terminator (optional, but consistent with previous behavior)
        *src_addr.add(ip_bytes.len()) = 0;
        *src_addr_len = ip_bytes.len() + 1; // including null
        *src_port = port;
    }
    n as isize
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_connect(
    fd: i32,
    addr: *const u8,
    addr_len: usize,
    port: u16,
) -> i32 {
    let fd = fd as u32;
    let addr_str = match unsafe { bytes_to_string(addr, addr_len) } {
        Some(s) => s,
        None => return -1,
    };
    let target = match format!("{}:{}", addr_str, port).to_socket_addrs() {
        Ok(mut addrs) => addrs.next(),
        Err(_) => None,
    };
    let target = match target {
        Some(addr) => addr,
        None => return -1,
    };
    let mut store = udp_sockets().lock().unwrap();
    let socket = match store.sockets.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    match socket.connect(target) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_send(fd: i32, buf: *const u8, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let mut store = udp_sockets().lock().unwrap();
    let socket = match store.sockets.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts(buf, count) };
    match socket.send(slice) {
        Ok(n) => n as isize,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_udp_recv(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    let fd = fd as u32;
    let mut store = udp_sockets().lock().unwrap();
    let socket = match store.sockets.get_mut(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, count) };
    match socket.recv(slice) {
        Ok(n) => n as isize,
        Err(_) => -1,
    }
}

// ---------------------------------------------------------------------------
// Threading
// ---------------------------------------------------------------------------

type ThreadId = usize;

struct ThreadInfo {
    handle: JoinHandle<()>,
    thread: Arc<std::thread::Thread>,
}
struct ThreadStore {
    next_id: ThreadId,
    infos: HashMap<ThreadId, ThreadInfo>,
}

fn threads() -> &'static Mutex<ThreadStore> {
    static THREADS: OnceLock<Mutex<ThreadStore>> = OnceLock::new();
    THREADS.get_or_init(|| {
        Mutex::new(ThreadStore {
            next_id: 1,
            infos: HashMap::new(),
        })
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_spawn(f: extern "C" fn(*mut u8), arg: *mut u8) -> usize {
    let arg_usize = arg as usize;
    let handle = thread::spawn(move || {
        let arg_ptr = arg_usize as *mut u8;
        f(arg_ptr);
    });
    let thread = handle.thread().clone();
    let info = ThreadInfo {
        handle,
        thread: Arc::new(thread),
    };
    let mut store = threads().lock().unwrap();
    let id = store.next_id;
    store.next_id += 1;
    store.infos.insert(id, info);
    id
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_join(handle: usize) -> i32 {
    let handle_id = handle;
    let mut store = threads().lock().unwrap();
    if let Some(info) = store.infos.remove(&handle_id) {
        drop(store);
        match info.handle.join() {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_yield() {
    thread::yield_now();
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_sleep(secs: u64, nanos: u32) {
    let duration = Duration::new(secs, nanos);
    thread::sleep(duration);
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_park() {
    thread::park();
}

/// Park the current thread with a timeout.
///
/// Blocks the current thread until either the token is made available or
/// the specified duration has elapsed.
///
/// # Safety
///
/// This is a safe FFI function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_thread_park_timeout(secs: u64, nanos: u32) {
    let duration = std::time::Duration::new(secs, nanos);
    std::thread::park_timeout(duration);
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_unpark(handle: usize) {
    let handle_id = handle;
    let store = threads().lock().unwrap();
    if let Some(info) = store.infos.get(&handle_id) {
        info.thread.unpark();
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_current_id() -> usize {
    // Use libc::pthread_self() for a numeric thread ID (Unix).
    #[cfg(unix)]
    {
        // libc::pthread_self() returns pthread_t (u64 on Linux x86_64, u64 on
        // macOS arm64). Cast to usize; on 64-bit targets this is a no-op that
        // satisfies the type checker across both platforms.
        unsafe { libc::pthread_self() as usize }
    }
    #[cfg(not(unix))]
    {
        use std::hash::{Hash, Hasher};
        let id = thread::current().id();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut hasher);
        hasher.finish() as usize
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_thread_available_parallelism() -> usize {
    match thread::available_parallelism() {
        Ok(n) => n.get(),
        Err(_) => 1,
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

static START: OnceLock<Instant> = OnceLock::new();

fn monotonic_base() -> &'static Instant {
    START.get_or_init(Instant::now)
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_time_now_secs() -> u64 {
    monotonic_base().elapsed().as_secs()
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_time_now_nanos() -> u64 {
    monotonic_base().elapsed().subsec_nanos() as u64
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_time_system_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_time_system_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

pub mod fs;

// ---------------------------------------------------------------------------
// Async executor (Phase 5 MVP)
// ---------------------------------------------------------------------------

pub mod async_runtime;

#[cfg(test)]
mod tests;