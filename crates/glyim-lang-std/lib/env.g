//! Inspection and manipulation of the process's environment for the Glyim standard library.
//!
//! This module contains functions to inspect various aspects such as environment
//! variables, process arguments, the current directory, and the home directory.

/// Returns the filesystem path that the current process was started from.
fn current_dir() -> Result<String, String> {
    extern "C" {
        fn glyim_env_current_dir(buf: *mut u8, cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_env_current_dir(buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        Result::Err("failed to get current directory".to_string())
    } else {
        unsafe { buf.set_len(n as usize); }
        Result::Ok(String::from_utf8(buf).unwrap_or_default())
    }
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
        fn glyim_env_var(key: *const u8, key_len: usize, buf: *mut u8, cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_env_var(key.as_ptr(), key.len(), buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        Result::Err(format!("environment variable '{}' not found", key))
    } else {
        unsafe { buf.set_len(n as usize); }
        Result::Ok(String::from_utf8(buf).unwrap_or_default())
    }
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
        let rc = unsafe { glyim_env_vars_get(i, key_buf.as_mut_ptr(), key_buf.len(), val_buf.as_mut_ptr(), val_buf.len()) };
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
        fn glyim_env_args_get(index: usize, buf: *mut u8, cap: usize) -> i32;
    }
    let count = unsafe { glyim_env_args_count() };
    let mut result = Vec::new();
    let mut i = 0;
    while i < count {
        let mut buf = [0u8; 4096];
        let rc = unsafe { glyim_env_args_get(i, buf.as_mut_ptr(), buf.len()) };
        if rc >= 0 {
            result.push(String::from_utf8_lossy(&buf).to_string());
        }
        i += 1;
    }
    result
}

/// Returns the first argument (the program name), or a default.
fn current_exe() -> Result<String, String> {
    extern "C" {
        fn glyim_env_current_exe(buf: *mut u8, cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_env_current_exe(buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        Result::Err("failed to get current executable path".to_string())
    } else {
        unsafe { buf.set_len(n as usize); }
        Result::Ok(String::from_utf8(buf).unwrap_or_default())
    }
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
        fn glyim_env_home_dir(buf: *mut u8, cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_env_home_dir(buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        Result::Err(HomeDirError::Unknown)
    } else {
        unsafe { buf.set_len(n as usize); }
        match String::from_utf8(buf) {
            Result::Ok(s) => Result::Ok(s),
            Result::Err(_) => Result::Err(HomeDirError::InvalidUtf8),
        }
    }
}

/// Returns the path to a temporary directory.
fn temp_dir() -> String {
    extern "C" {
        fn glyim_env_temp_dir(buf: *mut u8, cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_env_temp_dir(buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        "/tmp".to_string()
    } else {
        unsafe { buf.set_len(n as usize); }
        String::from_utf8(buf).unwrap_or_else(|_| "/tmp".to_string())
    }
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
