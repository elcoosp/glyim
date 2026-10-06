//! Inspection and manipulation of the process's environment for the Glyim standard library.
//!
//! T010-PATCHED [STD-4]: every string-returning runtime FFI takes
//! `(out_ptr: *mut *mut u8, out_len: *mut usize)` and *allocates* the
//! buffer which the caller must free with `glyim_free_cstr`. The old
//! declarations used `(buf: *mut u8, cap: usize) -> isize`, so the
//! runtime wrote an 8-byte heap pointer into `buf[0..8]` and its length
//! into the `cap` value interpreted as an address — a write to near-NULL
//! that segfaulted every `env::var` / `current_dir` / `args` /
//! `current_exe` / `home_dir` / `temp_dir` call.

/// Returns the filesystem path that the current process was started from.
fn current_dir() -> Result<String, String> {
    extern "C" {
        fn glyim_env_current_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let mut p: *mut u8 = ptr::null_mut();
    let mut n: usize = 0;
    let rc = unsafe { glyim_env_current_dir(&mut p, &mut n) };
    if rc != 0 {
        return Result::Err("failed to get current directory".to_string());
    }
    let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p); }
    Result::Ok(s)
}

/// Changes the current working directory to the specified path.
fn set_current_dir(path: &str) -> Result<(), String> {
    extern "C" {
        fn glyim_env_set_current_dir(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_env_set_current_dir(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(format!("failed to set current directory to '{}'", path))
    } else {
        Result::Ok(())
    }
}

/// Fetches the environment variable `key` from the current process.
fn var(key: &str) -> Result<String, String> {
    extern "C" {
        fn glyim_env_var(
            name: *const u8,
            name_len: usize,
            out_ptr: *mut *mut u8,
            out_len: *mut usize,
        ) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let mut p: *mut u8 = ptr::null_mut();
    let mut n: usize = 0;
    let rc = unsafe { glyim_env_var(key.as_ptr(), key.len(), &mut p, &mut n) };
    if rc != 0 {
        return Result::Err(format!("environment variable '{}' not found", key));
    }
    let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p); }
    Result::Ok(s)
}

/// Sets the environment variable `key` to the value `value` for the currently running process.
fn set_var(key: &str, value: &str) {
    extern "C" {
        fn glyim_env_set_var(key: *const u8, key_len: usize, value: *const u8, value_len: usize) -> i32;
    }
    let _ = unsafe { glyim_env_set_var(key.as_ptr(), key.len(), value.as_ptr(), value.len()) };
}

/// Removes an environment variable from the environment of the currently running process.
fn remove_var(key: &str) {
    extern "C" {
        fn glyim_env_remove_var(key: *const u8, key_len: usize) -> i32;
    }
    let _ = unsafe { glyim_env_remove_var(key.as_ptr(), key.len()) };
}

/// Returns an iterator of (variable, value) pairs of strings, for all the
/// environment variables of the currently running process.
///
/// T141 (STD-16) tracks the follow-up: the runtime's `glyim_env_vars_get`
/// copies key/value bytes into the caller's buffers but does not expose
/// the number of bytes written, so this routine currently reads back
/// NUL-padded buffers.
fn vars() -> Vec<(String, String)> {
    extern "C" {
        fn glyim_env_vars_count() -> usize;
        fn glyim_env_vars_get(index: usize, key_buf: *mut u8, key_cap: usize, val_buf: *mut u8, val_cap: usize) -> i32;
    }
    let count = unsafe { glyim_env_vars_count() };
    let mut result = Vec::new();
    let mut i = 0;
    while i < count {
        let mut key_buf = [0u8; 256];
        let mut val_buf = [0u8; 4096];
        let rc = unsafe {
            glyim_env_vars_get(
                i,
                key_buf.as_mut_ptr(),
                key_buf.len(),
                val_buf.as_mut_ptr(),
                val_buf.len(),
            )
        };
        if rc >= 0 {
            let key = String::from_utf8_lossy(&key_buf).to_string();
            let val = String::from_utf8_lossy(&val_buf).to_string();
            result.push((key, val));
        }
        i += 1;
    }
    result
}

/// Returns the arguments which this program was started with.
fn args() -> Vec<String> {
    extern "C" {
        fn glyim_env_args_count() -> usize;
        fn glyim_env_args_get(index: usize, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let count = unsafe { glyim_env_args_count() };
    let mut result = Vec::new();
    let mut i = 0;
    while i < count {
        let mut p: *mut u8 = ptr::null_mut();
        let mut n: usize = 0;
        let rc = unsafe { glyim_env_args_get(i, &mut p, &mut n) };
        if rc >= 0 {
            let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
            let s = String::from_utf8_lossy(bytes).to_string();
            unsafe { glyim_free_cstr(p); }
            result.push(s);
        }
        i += 1;
    }
    result
}

/// Returns the first argument (the program name), or a default.
fn current_exe() -> Result<String, String> {
    extern "C" {
        fn glyim_env_current_exe(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let mut p: *mut u8 = ptr::null_mut();
    let mut n: usize = 0;
    let rc = unsafe { glyim_env_current_exe(&mut p, &mut n) };
    if rc != 0 {
        return Result::Err("failed to get current executable path".to_string());
    }
    let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p); }
    Result::Ok(s)
}

/// Possible errors from the `home_dir` function.
enum HomeDirError {
    /// The home directory could not be determined.
    Unknown,
    /// The home directory path was not valid UTF-8.
    InvalidUtf8,
}

/// Returns the path to the user's home directory.
fn home_dir() -> Result<String, HomeDirError> {
    extern "C" {
        fn glyim_env_home_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let mut p: *mut u8 = ptr::null_mut();
    let mut n: usize = 0;
    let rc = unsafe { glyim_env_home_dir(&mut p, &mut n) };
    if rc != 0 {
        return Result::Err(HomeDirError::Unknown);
    }
    let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p); }
    Result::Ok(s)
}

/// Returns the path to a temporary directory.
fn temp_dir() -> String {
    extern "C" {
        fn glyim_env_temp_dir(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32;
        fn glyim_free_cstr(ptr: *mut u8);
    }
    let mut p: *mut u8 = ptr::null_mut();
    let mut n: usize = 0;
    let rc = unsafe { glyim_env_temp_dir(&mut p, &mut n) };
    if rc != 0 {
        return "/tmp".to_string();
    }
    let bytes = unsafe { slice::from_raw_parts(p as *const u8, n) };
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p); }
    s
}

/// Returns the OS separator character.
const OS_SEPARATOR: &str = "/";

/// Returns `true` if the OS is a Unix-like system.
fn is_unix() -> bool {
    true
}

/// Returns `true` if the OS is a Windows system.
fn is_windows() -> bool {
    false
}

/// Constants associated with the current target.
const CONSTS: OsConsts = OsConsts {
    family: "unix",
    os: "linux",
    arch: "x86_64",
};

/// Constants for the operating system.
struct OsConsts {
    family: &'static str,
    os: &'static str,
    arch: &'static str,
}
