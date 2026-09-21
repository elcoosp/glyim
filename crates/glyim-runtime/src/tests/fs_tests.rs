//! Tests for the file system FFI functions in the glyim-runtime crate.
//!
//! Tests cover all four TDD requirements (S19-T01 through S19-T04) plus
//! comprehensive coverage for every public FFI function, including error
//! paths and edge cases.

use std::fs;
use std::path::PathBuf;

use crate::fs::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a unique temporary directory for a test.
///
/// The directory is cleaned up by the caller (via `cleanup`) at the end of
/// each test. Using a per-test subdirectory avoids collisions when tests run
/// in parallel.
fn temp_dir(name: &str) -> PathBuf {
    // Per-process + per-test unique dir. `std::process::id()` prevents a
    // second test process (parallel CI shard, `cargo-mutants` worker) from
    // racing on the same directory: previously `remove_dir_all` in one
    // process could delete the directory another just created, causing
    // spurious `canonicalize` failures.
    let dir = std::env::temp_dir()
        .join(format!("glyim-s19-test-{}", std::process::id()))
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// Remove a temp directory tree (call at the end of each test).
fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

/// Convert a Rust string to `(pointer, length)` for FFI calls.
fn str_ptr(s: &str) -> (*const u8, usize) {
    (s.as_ptr(), s.len())
}

/// Safe wrapper around `glyim_fs_open`.
fn open_file(path: &str, flags: u32) -> i32 {
    let (ptr, len) = str_ptr(path);
    // SAFETY: ptr points to valid UTF-8 data of length `len`.
    unsafe { glyim_fs_open(ptr, len, flags) }
}

/// Safe wrapper around `glyim_fs_close`.
fn close_file(fd: i32) -> i32 {
    glyim_fs_close(fd)
}

/// Safe wrapper around `glyim_fs_read`.
fn read_file(fd: i32, buf: &mut [u8]) -> isize {
    // SAFETY: buf is a valid mutable slice.
    unsafe { glyim_fs_read(fd, buf.as_mut_ptr(), buf.len()) }
}

/// Safe wrapper around `glyim_fs_write`.
fn write_file(fd: i32, data: &[u8]) -> isize {
    // SAFETY: data is a valid slice.
    unsafe { glyim_fs_write(fd, data.as_ptr(), data.len()) }
}

/// Safe wrapper around `glyim_fs_flush`.
fn flush_file(fd: i32) -> i32 {
    glyim_fs_flush(fd)
}

/// Safe wrapper around `glyim_fs_metadata`.
fn get_metadata(path: &str) -> (i32, u64) {
    let (ptr, len) = str_ptr(path);
    let mut size: u64 = 0;
    // SAFETY: ptr is valid UTF-8, &mut size is a valid u64 reference.
    let rc = unsafe { glyim_fs_metadata(ptr, len, &mut size) };
    (rc, size)
}

/// Safe wrapper around `glyim_fs_truncate`.
fn truncate_file(path: &str, size: u64) -> i32 {
    let (ptr, len) = str_ptr(path);
    // SAFETY: ptr is valid UTF-8 of length len.
    unsafe { glyim_fs_truncate(ptr, len, size) }
}

/// Safe wrapper around `glyim_fs_rename`.
fn rename_path(old: &str, new: &str) -> i32 {
    let (old_ptr, old_len) = str_ptr(old);
    let (new_ptr, new_len) = str_ptr(new);
    // SAFETY: both pointers are valid UTF-8.
    unsafe { glyim_fs_rename(old_ptr, old_len, new_ptr, new_len) }
}

/// Safe wrapper around `glyim_fs_remove_file`.
fn remove_the_file(path: &str) -> i32 {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_remove_file(ptr, len) }
}

/// Safe wrapper around `glyim_fs_remove_dir`.
fn remove_the_dir(path: &str) -> i32 {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_remove_dir(ptr, len) }
}

/// Safe wrapper around `glyim_fs_remove_dir_all`.
fn remove_the_dir_all(path: &str) -> i32 {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_remove_dir_all(ptr, len) }
}

/// Safe wrapper around `glyim_fs_create_dir`.
fn create_the_dir(path: &str) -> i32 {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_create_dir(ptr, len) }
}

/// Safe wrapper around `glyim_fs_create_dir_all`.
fn create_the_dir_all(path: &str) -> i32 {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_create_dir_all(ptr, len) }
}

/// Safe wrapper around `glyim_fs_canonicalize`.
fn canonicalize_path(path: &str, buf: &mut [u8]) -> isize {
    let (ptr, len) = str_ptr(path);
    unsafe { glyim_fs_canonicalize(ptr, len, buf.as_mut_ptr(), buf.len()) }
}

// ===================================================================
// S19-T01: glyim_fs_open returns valid fd for existing file
// ===================================================================

#[test]
fn s19_t01_open_existing_file() {
    let dir = temp_dir("open_existing");
    let file_path = dir.join("test.txt");
    fs::write(&file_path, b"hello").expect("failed to write test file");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0, "expected valid fd, got {}", fd);

    let rc = close_file(fd);
    assert_eq!(rc, FS_OK, "close should succeed");

    cleanup(&dir);
}

#[test]
fn s19_t01_open_nonexistent_file_returns_error() {
    let dir = temp_dir("open_nonexistent");
    let path = dir.join("does_not_exist.txt");
    let path_str = path.to_str().expect("non-utf8 path");

    let fd = open_file(path_str, FS_O_RDONLY);
    assert_eq!(
        fd, FS_ENOENT,
        "expected ENOENT for missing file, got {}",
        fd
    );

    cleanup(&dir);
}

#[test]
fn s19_t01_open_create_new_file() {
    let dir = temp_dir("open_create");
    let path = dir.join("new_file.txt");
    let path_str = path.to_str().expect("non-utf8 path");

    let fd = open_file(path_str, FS_O_WRONLY | FS_O_CREAT);
    assert!(fd >= 0, "expected valid fd, got {}", fd);

    close_file(fd);
    assert!(path.exists(), "file should have been created");

    cleanup(&dir);
}

#[test]
fn s19_t01_close_invalid_fd() {
    let rc = close_file(-1);
    assert_eq!(rc, FS_EBADF, "expected EBADF for invalid fd");
}

// ===================================================================
// S19-T02: glyim_fs_read reads bytes into buffer
// ===================================================================

#[test]
fn s19_t02_read_existing_content() {
    let dir = temp_dir("read_content");
    let file_path = dir.join("data.bin");
    let content = b"Hello, Glyim!";
    fs::write(&file_path, content).expect("failed to write test file");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0);

    let mut buf = vec![0u8; 256];
    let n = read_file(fd, &mut buf);
    assert_eq!(
        n,
        content.len() as isize,
        "should read {} bytes",
        content.len()
    );
    assert_eq!(&buf[..content.len()], content, "content should match");

    close_file(fd);
    cleanup(&dir);
}

#[test]
fn s19_t02_read_invalid_fd() {
    let mut buf = [0u8; 16];
    let n = read_file(-42, &mut buf);
    assert_eq!(n, FS_EBADF as isize, "expected EBADF for invalid fd");
}

#[test]
fn s19_t02_read_returns_zero_at_eof() {
    let dir = temp_dir("read_eof");
    let file_path = dir.join("tiny.txt");
    fs::write(&file_path, b"X").expect("write failed");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0);

    let mut buf = [0u8; 1];
    let n1 = read_file(fd, &mut buf);
    assert_eq!(n1, 1, "first read should return 1 byte");

    let n2 = read_file(fd, &mut buf);
    assert_eq!(n2, 0, "second read should return 0 (EOF)");

    close_file(fd);
    cleanup(&dir);
}

// ===================================================================
// glyim_fs_write
// ===================================================================

#[test]
fn test_fs_write_then_read() {
    let dir = temp_dir("write_read");
    let file_path = dir.join("output.txt");
    let path_str = file_path.to_str().expect("non-utf8 path");

    // Write
    let fd = open_file(path_str, FS_O_WRONLY | FS_O_CREAT | FS_O_TRUNC);
    assert!(fd >= 0, "expected valid fd for write, got {}", fd);

    let data = b"written data";
    let n = write_file(fd, data);
    assert_eq!(n, data.len() as isize, "should write all bytes");
    close_file(fd);

    // Read back
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0);
    let mut buf = vec![0u8; 256];
    let n = read_file(fd, &mut buf);
    assert_eq!(n, data.len() as isize);
    assert_eq!(&buf[..data.len()], data);
    close_file(fd);

    cleanup(&dir);
}

#[test]
fn test_fs_write_invalid_fd() {
    let data = b"test";
    let n = write_file(-99, data);
    assert_eq!(n, FS_EBADF as isize, "expected EBADF for invalid fd");
}

// ===================================================================
// glyim_fs_flush
// ===================================================================

#[test]
fn test_fs_flush_after_write() {
    let dir = temp_dir("flush");
    let file_path = dir.join("flush.txt");
    let path_str = file_path.to_str().expect("non-utf8 path");

    let fd = open_file(path_str, FS_O_WRONLY | FS_O_CREAT);
    assert!(fd >= 0);
    write_file(fd, b"flush test");
    let rc = flush_file(fd);
    assert_eq!(rc, FS_OK, "flush should succeed");
    close_file(fd);

    cleanup(&dir);
}

#[test]
fn test_fs_flush_invalid_fd() {
    let rc = flush_file(-1);
    assert_eq!(rc, FS_EBADF, "expected EBADF for invalid fd");
}

// ===================================================================
// S19-T03: glyim_fs_create_dir creates directory
// ===================================================================

#[test]
fn s19_t03_create_dir() {
    let dir = temp_dir("create_dir");
    let new_dir = dir.join("subdir");
    let path_str = new_dir.to_str().expect("non-utf8 path");

    let rc = create_the_dir(path_str);
    assert_eq!(rc, FS_OK, "create_dir should succeed");
    assert!(new_dir.is_dir(), "directory should exist");

    cleanup(&dir);
}

#[test]
fn s19_t03_create_dir_already_exists() {
    let dir = temp_dir("create_dir_exists");
    let new_dir = dir.join("subdir");
    fs::create_dir(&new_dir).expect("pre-create failed");
    let path_str = new_dir.to_str().expect("non-utf8 path");

    let rc = create_the_dir(path_str);
    assert_eq!(rc, FS_EEXIST, "expected EEXIST for existing dir");

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_create_dir_all
// ===================================================================

#[test]
fn test_fs_create_dir_all_nested() {
    let dir = temp_dir("create_dir_all");
    let deep = dir.join("a").join("b").join("c");
    let path_str = deep.to_str().expect("non-utf8 path");

    let rc = create_the_dir_all(path_str);
    assert_eq!(rc, FS_OK, "create_dir_all should succeed");
    assert!(deep.is_dir(), "deep directory should exist");

    // Idempotent: calling again should succeed
    let rc2 = create_the_dir_all(path_str);
    assert_eq!(rc2, FS_OK, "create_dir_all should be idempotent");

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_metadata
// ===================================================================

#[test]
fn test_fs_metadata_size() {
    let dir = temp_dir("metadata");
    let file_path = dir.join("sized.bin");
    let content = b"1234567890"; // 10 bytes
    fs::write(&file_path, content).expect("failed to write");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let (rc, size) = get_metadata(path_str);
    assert_eq!(rc, FS_OK, "metadata should succeed");
    assert_eq!(size, 10, "file should be 10 bytes");

    cleanup(&dir);
}

#[test]
fn test_fs_metadata_nonexistent() {
    let dir = temp_dir("metadata_missing");
    let path = dir.join("ghost.bin");
    let path_str = path.to_str().expect("non-utf8 path");

    let (rc, _) = get_metadata(path_str);
    assert_eq!(rc, FS_ENOENT, "expected ENOENT");

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_truncate
// ===================================================================

#[test]
fn test_fs_truncate_shrink() {
    let dir = temp_dir("truncate");
    let file_path = dir.join("trunc.bin");
    fs::write(&file_path, b"12345678901234567890").expect("write failed");
    // 20 bytes written
    let path_str = file_path.to_str().expect("non-utf8 path");

    let rc = truncate_file(path_str, 5);
    assert_eq!(rc, FS_OK, "truncate should succeed");

    let (rc, size) = get_metadata(path_str);
    assert_eq!(rc, FS_OK);
    assert_eq!(size, 5, "file should be truncated to 5 bytes");

    cleanup(&dir);
}

#[test]
fn test_fs_truncate_nonexistent() {
    let dir = temp_dir("truncate_missing");
    let path = dir.join("phantom.bin");
    let path_str = path.to_str().expect("non-utf8 path");

    let rc = truncate_file(path_str, 0);
    assert_eq!(rc, FS_ENOENT, "expected ENOENT for nonexistent file");

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_rename
// ===================================================================

#[test]
fn test_fs_rename_file() {
    let dir = temp_dir("rename");
    let old_path = dir.join("old.txt");
    let new_path = dir.join("new.txt");
    fs::write(&old_path, b"renamed content").expect("write failed");

    let old_str = old_path.to_str().expect("non-utf8");
    let new_str = new_path.to_str().expect("non-utf8");

    let rc = rename_path(old_str, new_str);
    assert_eq!(rc, FS_OK, "rename should succeed");
    assert!(!old_path.exists(), "old file should not exist");
    assert!(new_path.exists(), "new file should exist");
    assert_eq!(
        fs::read(&new_path).expect("read failed"),
        b"renamed content"
    );

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_remove_file
// ===================================================================

#[test]
fn test_fs_remove_file_success() {
    let dir = temp_dir("remove_file");
    let file_path = dir.join("to_remove.txt");
    fs::write(&file_path, b"bye").expect("write failed");
    assert!(file_path.exists());

    let path_str = file_path.to_str().expect("non-utf8");
    let rc = remove_the_file(path_str);
    assert_eq!(rc, FS_OK, "remove_file should succeed");
    assert!(!file_path.exists(), "file should be gone");

    cleanup(&dir);
}

#[test]
fn test_fs_remove_file_nonexistent() {
    let dir = temp_dir("remove_file_missing");
    let path = dir.join("phantom.txt");
    let path_str = path.to_str().expect("non-utf8");

    let rc = remove_the_file(path_str);
    assert_eq!(rc, FS_ENOENT, "expected ENOENT");

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_remove_dir
// ===================================================================

#[test]
fn test_fs_remove_dir_empty() {
    let dir = temp_dir("remove_dir");
    let subdir = dir.join("empty_dir");
    fs::create_dir(&subdir).expect("create failed");
    assert!(subdir.is_dir());

    let path_str = subdir.to_str().expect("non-utf8");
    let rc = remove_the_dir(path_str);
    assert_eq!(rc, FS_OK, "remove_dir should succeed for empty dir");
    assert!(!subdir.exists(), "dir should be gone");

    cleanup(&dir);
}

#[test]
fn test_fs_remove_dir_not_empty_fails() {
    let dir = temp_dir("remove_dir_notempty");
    let subdir = dir.join("full_dir");
    fs::create_dir(&subdir).expect("create failed");
    fs::write(subdir.join("file.txt"), b"content").expect("write failed");

    let path_str = subdir.to_str().expect("non-utf8");
    let rc = remove_the_dir(path_str);
    assert_ne!(
        rc, FS_OK,
        "remove_dir should fail for non-empty dir, got {}",
        rc
    );

    cleanup(&dir);
}

// ===================================================================
// glyim_fs_remove_dir_all
// ===================================================================

#[test]
fn test_fs_remove_dir_all_recursive() {
    let dir = temp_dir("remove_dir_all");
    let subdir = dir.join("nested");
    fs::create_dir_all(subdir.join("a").join("b")).expect("create failed");
    fs::write(subdir.join("a").join("file.txt"), b"content").expect("write failed");

    let path_str = subdir.to_str().expect("non-utf8");
    let rc = remove_the_dir_all(path_str);
    assert_eq!(rc, FS_OK, "remove_dir_all should succeed");
    assert!(!subdir.exists(), "dir tree should be gone");

    cleanup(&dir);
}

// ===================================================================
// S19-T04: glyim_fs_canonicalize resolves paths
// ===================================================================

#[test]
fn s19_t04_canonicalize_resolves_dot() {
    let dir = temp_dir("canonicalize");
    let file_path = dir.join("real.txt");
    fs::write(&file_path, b"canonical").expect("write failed");

    // Use a path with "." to verify canonicalization resolves it
    let dot_path = dir.join(".").join("real.txt");
    let path_str = dot_path.to_str().expect("non-utf8");

    let mut buf = vec![0u8; 1024];
    let n = canonicalize_path(path_str, &mut buf);
    assert!(n >= 0, "canonicalize should succeed, got {}", n);

    let resolved = std::str::from_utf8(&buf[..n as usize]).expect("utf8");
    assert!(
        std::path::Path::new(resolved).is_absolute(),
        "canonicalized path should be absolute: {}",
        resolved
    );
    assert!(
        resolved.ends_with("real.txt"),
        "resolved should end with real.txt: {}",
        resolved
    );

    cleanup(&dir);
}

#[test]
fn s19_t04_canonicalize_nonexistent_returns_error() {
    let dir = temp_dir("canonicalize_missing");
    let path = dir.join("ghost.txt");
    let path_str = path.to_str().expect("non-utf8");

    let mut buf = vec![0u8; 1024];
    let n = canonicalize_path(path_str, &mut buf);
    assert!(n < 0, "expected error for nonexistent path, got {}", n);

    cleanup(&dir);
}

#[test]
fn s19_t04_canonicalize_buffer_too_small() {
    let dir = temp_dir("canonicalize_small");
    let file_path = dir.join("file.txt");
    fs::write(&file_path, b"x").expect("write failed");

    let path_str = file_path.to_str().expect("non-utf8");
    let mut buf = vec![0u8; 1]; // intentionally too small
    let n = canonicalize_path(path_str, &mut buf);
    assert_eq!(
        n, FS_EOVERFLOW as isize,
        "expected EOVERFLOW for tiny buffer"
    );

    cleanup(&dir);
}

// ===================================================================
// Edge cases: null pointer handling
// ===================================================================

#[test]
fn test_fs_open_null_path_returns_error() {
    let fd = unsafe { glyim_fs_open(std::ptr::null(), 0, FS_O_RDONLY) };
    assert!(fd < 0, "null path should return error, got {}", fd);
}

#[test]
fn test_fs_read_null_buffer_returns_error() {
    let n = unsafe { glyim_fs_read(0, std::ptr::null_mut(), 100) };
    assert!(n < 0, "null buffer should return error, got {}", n);
}

#[test]
fn test_fs_write_null_buffer_with_positive_len_returns_error() {
    let n = unsafe { glyim_fs_write(0, std::ptr::null(), 10) };
    assert!(n < 0, "null buf with len>0 should return error, got {}", n);
}

// ===================================================================
// Multiple fds work independently
// ===================================================================

#[test]
fn test_fs_multiple_fds_independent() {
    let dir = temp_dir("multi_fd");
    let path_a = dir.join("a.txt");
    let path_b = dir.join("b.txt");
    fs::write(&path_a, b"AAA").expect("write a");
    fs::write(&path_b, b"BBB").expect("write b");

    let fd_a = open_file(path_a.to_str().unwrap(), FS_O_RDONLY);
    let fd_b = open_file(path_b.to_str().unwrap(), FS_O_RDONLY);
    assert!(fd_a >= 0, "fd_a should be valid");
    assert!(fd_b >= 0, "fd_b should be valid");
    assert_ne!(fd_a, fd_b, "fds should be distinct");

    let mut buf = [0u8; 16];

    let n_a = read_file(fd_a, &mut buf);
    assert_eq!(n_a, 3);
    assert_eq!(&buf[..3], b"AAA");

    let n_b = read_file(fd_b, &mut buf);
    assert_eq!(n_b, 3);
    assert_eq!(&buf[..3], b"BBB");

    close_file(fd_a);
    close_file(fd_b);

    cleanup(&dir);
}

// ===================================================================
// Append mode
// ===================================================================

#[test]
fn test_fs_append_mode() {
    let dir = temp_dir("append");
    let file_path = dir.join("append.txt");
    fs::write(&file_path, b"first").expect("write failed");
    let path_str = file_path.to_str().expect("non-utf8 path");

    let fd = open_file(path_str, FS_O_WRONLY | FS_O_APPEND);
    assert!(fd >= 0, "expected valid fd for append");
    let n = write_file(fd, b"second");
    assert_eq!(n, 6, "should write 6 bytes");
    close_file(fd);

    let content = fs::read(&file_path).expect("read failed");
    assert_eq!(
        content, b"firstsecond",
        "append should add after existing data"
    );

    cleanup(&dir);
}

// ===================================================================
// W5-C04-T04: glyim_fs_canonicalize resolves symlinks
// ===================================================================

#[cfg(unix)]
#[test]
fn w5_c04_t04_canonicalize_resolves_symlink() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("canonicalize_symlink");
    let real_file = dir.join("real.txt");
    let link_file = dir.join("link.txt");
    fs::write(&real_file, b"target").expect("write failed");
    symlink(&real_file, &link_file).expect("symlink creation failed");

    let path_str = link_file.to_str().expect("non-utf8");
    let mut buf = vec![0u8; 1024];
    let n = canonicalize_path(path_str, &mut buf);
    assert!(n >= 0, "canonicalize should succeed on symlink, got {}", n);

    let resolved = std::str::from_utf8(&buf[..n as usize]).expect("utf8");
    assert!(
        resolved.ends_with("real.txt"),
        "canonicalize should resolve symlink to real.txt, got: {}",
        resolved
    );

    cleanup(&dir);
}

// ===================================================================
// Error code mapping: non-empty directory returns FS_ENOTEMPTY
// ===================================================================

#[cfg(unix)]
#[test]
fn test_fs_remove_dir_not_empty_returns_enotempty() {
    let dir = temp_dir("remove_dir_enotempty");
    let subdir = dir.join("full_dir");
    fs::create_dir(&subdir).expect("create failed");
    fs::write(subdir.join("file.txt"), b"content").expect("write failed");

    let path_str = subdir.to_str().expect("non-utf8");
    let rc = remove_the_dir(path_str);
    assert_eq!(
        rc, FS_ENOTEMPTY,
        "expected FS_ENOTEMPTY for non-empty dir, got {}",
        rc
    );

    cleanup(&dir);
}

// ===================================================================
// Error code mapping: opening path through non-directory returns FS_ENOTDIR
// ===================================================================

#[cfg(unix)]
#[test]
fn test_fs_open_path_through_file_returns_enotdir() {
    let dir = temp_dir("open_enotdir");
    let file_path = dir.join("regular");
    fs::write(&file_path, b"not a dir").expect("write failed");

    // Attempt to open a path where the parent is a regular file, not a directory.
    let nested = file_path.join("nested");
    let path_str = nested.to_str().expect("non-utf8");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert_eq!(fd, FS_ENOTDIR, "expected FS_ENOTDIR, got {}", fd);

    cleanup(&dir);
}

// ===================================================================
// W5-C04-T01: glyim_fs_open returns valid fd for existing file
// ===================================================================

#[test]
fn w5_c04_t01_open_returns_valid_fd() {
    let dir = temp_dir("w5c04_open_fd");
    let file_path = dir.join("exists.txt");
    fs::write(&file_path, b"content").expect("failed to write test file");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0, "expected valid fd for existing file, got {}", fd);

    // The fd should be usable for reading.
    let mut buf = vec![0u8; 64];
    let n = read_file(fd, &mut buf);
    assert_eq!(n, 7, "should read 7 bytes");

    let rc = close_file(fd);
    assert_eq!(rc, FS_OK, "close should succeed");

    cleanup(&dir);
}

// ===================================================================
// W5-C04-T02: glyim_fs_read reads bytes into buffer
// ===================================================================

#[test]
fn w5_c04_t02_read_reads_bytes_into_buffer() {
    let dir = temp_dir("w5c04_read_buf");
    let file_path = dir.join("data.bin");
    let content = b"Hello Glyim FS!";
    fs::write(&file_path, content).expect("failed to write test file");

    let path_str = file_path.to_str().expect("non-utf8 path");
    let fd = open_file(path_str, FS_O_RDONLY);
    assert!(fd >= 0, "open should succeed");

    let mut buf = vec![0u8; 256];
    let n = read_file(fd, &mut buf);
    assert_eq!(
        n,
        content.len() as isize,
        "should read {} bytes",
        content.len()
    );
    assert_eq!(
        &buf[..content.len()],
        content,
        "buffer content should match"
    );

    close_file(fd);
    cleanup(&dir);
}

// ===================================================================
// W5-C04-T03: glyim_fs_create_dir creates directory
// ===================================================================

#[test]
fn w5_c04_t03_create_dir_creates_directory() {
    let dir = temp_dir("w5c04_createdir");
    let new_dir = dir.join("new_subdir");
    let path_str = new_dir.to_str().expect("non-utf8 path");

    let rc = create_the_dir(path_str);
    assert_eq!(rc, FS_OK, "create_dir should succeed");
    assert!(new_dir.is_dir(), "directory should exist on disk");

    // Writing a file inside the new directory proves it is usable.
    let inner = new_dir.join("inner.txt");
    fs::write(&inner, b"works").expect("should be able to write inside new dir");
    assert!(inner.exists());

    cleanup(&dir);
}
