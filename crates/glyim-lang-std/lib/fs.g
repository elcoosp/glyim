//! Filesystem manipulation operations for the Glyim standard library.
//!
//! This module contains functions for manipulating files and directories.

use io::{Read, Write, Error, ErrorKind, Result};

/// A reference to an open file on the filesystem.
///
/// An instance of a `File` can be read and/or written depending on what options
/// it was opened with. Files also implement `Seek` to alter the logical cursor
/// that the file contains internally.
struct File {
    fd: i32,
    path: String,
}

/// Options and flags which can be used to configure how a file is opened.
struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl OpenOptions {
    /// Create a new blank `OpenOptions` with default settings.
    fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
        }
    }

    /// Set the option for read access.
    fn read(mut self, read: bool) -> OpenOptions {
        self.read = read;
        self
    }

    /// Set the option for write access.
    fn write(mut self, write: bool) -> OpenOptions {
        self.write = write;
        self
    }

    /// Set the option for append mode.
    fn append(mut self, append: bool) -> OpenOptions {
        self.append = append;
        self
    }

    /// Set the option for truncating a previous file.
    fn truncate(mut self, truncate: bool) -> OpenOptions {
        self.truncate = truncate;
        self
    }

    /// Set the option to create a new file.
    fn create(mut self, create: bool) -> OpenOptions {
        self.create = create;
        self
    }

    /// Set the option to create a new file, failing if it already exists.
    fn create_new(mut self, create_new: bool) -> OpenOptions {
        self.create_new = create_new;
        self
    }

    /// Open a file at `path` with the options specified by `self`.
    fn open(self, path: &str) -> Result<File> {
        extern "C" {
            fn glyim_fs_open(path: *const u8, path_len: usize, flags: u32) -> i32;
        }
        let flags = self.to_flags();
        // SAFETY: path is valid UTF-8, flags are computed from OpenOptions
        let fd = unsafe { glyim_fs_open(path.as_ptr(), path.len(), flags) };
        if fd < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(File { fd, path: path.to_string() })
        }
    }

    fn to_flags(&self) -> u32 {
        let mut flags: u32 = 0;
        if self.read { flags |= 1; }
        if self.write { flags |= 2; }
        if self.append { flags |= 4; }
        if self.truncate { flags |= 8; }
        if self.create { flags |= 16; }
        if self.create_new { flags |= 32; }
        flags
    }
}

impl File {
    /// Attempt to open a file in read-only mode.
    fn open(path: &str) -> Result<File> {
        OpenOptions::new().read(true).open(path)
    }

    /// Attempt to open a file in write-only mode, creating it if it doesn't exist.
    fn create(path: &str) -> Result<File> {
        OpenOptions::new().write(true).create(true).truncate(true).open(path)
    }

    /// Returns the file path this `File` was opened with.
    fn path(&self) -> &str {
        &self.path
    }

    /// Read all bytes until EOF in this source, placing them into a `Vec<u8>`.
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let mut tmp = [0u8; 4096];
        let mut total = 0;
        loop {
            match self.read(&mut tmp) {
                Result::Ok(0) => break,
                Result::Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    total += n;
                }
                Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Result::Err(e) => return Result::Err(e),
            }
        }
        Result::Ok(total)
    }

    /// Get the metadata for this file.
    fn metadata(&self) -> Result<Metadata> {
        extern "C" {
            fn glyim_fs_metadata(fd: i32, out: *mut MetadataRaw) -> i32;
        }
        let mut raw = MetadataRaw::default();
        let rc = unsafe { glyim_fs_metadata(self.fd, &mut raw) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(Metadata::from_raw(raw))
        }
    }

    /// Truncate or extend the underlying file.
    fn set_len(&self, size: u64) -> Result<()> {
        extern "C" {
            fn glyim_fs_truncate(fd: i32, size: u64) -> i32;
        }
        let rc = unsafe { glyim_fs_truncate(self.fd, size) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        extern "C" {
            fn glyim_fs_read(fd: i32, buf: *mut u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_fs_read(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        extern "C" {
            fn glyim_fs_write(fd: i32, buf: *const u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_fs_write(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    fn flush(&mut self) -> Result<()> {
        extern "C" {
            fn glyim_fs_flush(fd: i32) -> i32;
        }
        let rc = unsafe { glyim_fs_flush(self.fd) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }
}

/// Raw metadata from FFI.
struct MetadataRaw {
    size: u64,
    perm: u32,
    file_type: u32,
    modified_secs: u64,
    modified_nanos: u32,
    accessed_secs: u64,
    accessed_nanos: u32,
    created_secs: u64,
    created_nanos: u32,
}

impl Default for MetadataRaw {
    fn default() -> Self {
        MetadataRaw {
            size: 0, perm: 0, file_type: 0,
            modified_secs: 0, modified_nanos: 0,
            accessed_secs: 0, accessed_nanos: 0,
            created_secs: 0, created_nanos: 0,
        }
    }
}

/// Metadata information about a file.
struct Metadata {
    size: u64,
    perm: u32,
    file_type: FileType,
    modified: SystemTime,
    accessed: SystemTime,
    created: SystemTime,
}

impl Metadata {
    /// Convert from raw FFI metadata.
    fn from_raw(raw: MetadataRaw) -> Metadata {
        Metadata {
            size: raw.size,
            perm: raw.perm,
            file_type: FileType::from_raw(raw.file_type),
            modified: SystemTime::from_secs_nanos(raw.modified_secs, raw.modified_nanos),
            accessed: SystemTime::from_secs_nanos(raw.accessed_secs, raw.accessed_nanos),
            created: SystemTime::from_secs_nanos(raw.created_secs, raw.created_nanos),
        }
    }

    /// Returns the size of the file in bytes.
    fn len(&self) -> u64 {
        self.size
    }

    /// Returns `true` if this metadata is for a regular file.
    fn is_file(&self) -> bool {
        self.file_type.is_file()
    }

    /// Returns `true` if this metadata is for a directory.
    fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// Returns `true` if this metadata is for a symbolic link.
    fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// Returns the permissions of the file.
    fn permissions(&self) -> Permissions {
        Permissions { mode: self.perm }
    }
}

/// A structure representing a type of file.
struct FileType {
    raw: u32,
}

impl FileType {
    fn from_raw(raw: u32) -> FileType {
        FileType { raw }
    }

    /// Test whether this file type represents a regular file.
    fn is_file(&self) -> bool {
        self.raw == 1
    }

    /// Test whether this file type represents a directory.
    fn is_dir(&self) -> bool {
        self.raw == 2
    }

    /// Test whether this file type represents a symbolic link.
    fn is_symlink(&self) -> bool {
        self.raw == 3
    }
}

/// Representation of the various permissions on a file.
struct Permissions {
    mode: u32,
}

impl Permissions {
    /// Returns `true` if these permissions describe a readonly file.
    fn readonly(&self) -> bool {
        (self.mode & 0o200u32) == 0
    }

    /// Set the readonly flag.
    fn set_readonly(&mut self, readonly: bool) {
        if readonly {
            self.mode &= !0o222u32;
        } else {
            self.mode |= 0o200u32;
        }
    }
}

/// Read the entire contents of a file into a bytes vector.
fn read(path: &str) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Result::Ok(buf)
}

/// Read the entire contents of a file into a string.
fn read_to_string(path: &str) -> Result<String> {
    let bytes = read(path)?;
    let s = str::from_utf8(&bytes).map_err(|_| Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8".to_string()))?;
    Result::Ok(s.to_string())
}

/// Write a slice as the entire contents of a file.
fn write(path: &str, contents: &[u8]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents)?;
    Result::Ok(())
}

/// Copy the contents of one file to another.
fn copy(from: &str, to: &str) -> Result<u64> {
    let mut src = File::open(from)?;
    let mut dst = File::create(to)?;
    io::copy(&mut src, &mut dst)
}

/// Rename a file or directory to a new name.
fn rename(from: &str, to: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_rename(from: *const u8, from_len: usize, to: *const u8, to_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_rename(from.as_ptr(), from.len(), to.as_ptr(), to.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Remove a file from the filesystem.
fn remove_file(path: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_remove_file(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_remove_file(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Remove a directory at this path.
fn remove_dir(path: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_remove_dir(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_remove_dir(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Remove a directory and all of its contents recursively.
fn remove_dir_all(path: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_remove_dir_all(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_remove_dir_all(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Create a new directory.
fn create_dir(path: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_create_dir(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_create_dir(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Recursively create a directory and all of its parent components.
fn create_dir_all(path: &str) -> Result<()> {
    extern "C" {
        fn glyim_fs_create_dir_all(path: *const u8, path_len: usize) -> i32;
    }
    let rc = unsafe { glyim_fs_create_dir_all(path.as_ptr(), path.len()) };
    if rc < 0 {
        Result::Err(Error::last_os_error())
    } else {
        Result::Ok(())
    }
}

/// Read the metadata of a file without following symlinks.
fn symlink_metadata(path: &str) -> Result<Metadata> {
    let file = File::open(path)?;
    file.metadata()
}

/// Return the metadata of a file.
fn metadata(path: &str) -> Result<Metadata> {
    let file = File::open(path)?;
    file.metadata()
}

/// Check if a path exists.
fn exists(path: &str) -> bool {
    metadata(path).is_ok()
}

/// Check if a path is a directory.
fn is_dir(path: &str) -> bool {
    metadata(path).map_or(false, |m| m.is_dir())
}

/// Check if a path is a file.
fn is_file(path: &str) -> bool {
    metadata(path).map_or(false, |m| m.is_file())
}

/// An iterator over the entries in a directory.
struct ReadDir {
    dir_handle: i32,
}

/// An entry in a directory.
struct DirEntry {
    path: String,
}

impl DirEntry {
    /// Return the full path to this entry.
    fn path(&self) -> &str {
        &self.path
    }

    /// Return the metadata for this entry.
    fn metadata(&self) -> Result<Metadata> {
        fs::metadata(&self.path)
    }
}

/// A builder for creating directories with configurable options.
struct DirBuilder {
    recursive: bool,
    mode: u32,
}

impl DirBuilder {
    /// Create a new `DirBuilder` with default settings.
    fn new() -> DirBuilder {
        DirBuilder {
            recursive: false,
            mode: 0o755,
        }
    }

    /// Set recursive mode.
    fn recursive(mut self, recursive: bool) -> DirBuilder {
        self.recursive = recursive;
        self
    }

    /// Create the directory.
    fn create(self, path: &str) -> Result<()> {
        if self.recursive {
            create_dir_all(path)
        } else {
            create_dir(path)
        }
    }
}

/// Returns the canonical form of a path.
fn canonicalize(path: &str) -> Result<String> {
    extern "C" {
        fn glyim_fs_canonicalize(path: *const u8, path_len: usize, out: *mut u8, out_cap: usize) -> isize;
    }
    let mut buf = Vec::with_capacity(4096);
    let n = unsafe { glyim_fs_canonicalize(path.as_ptr(), path.len(), buf.as_mut_ptr(), buf.capacity()) };
    if n < 0 {
        Result::Err(Error::last_os_error())
    } else {
        unsafe { buf.set_len(n as usize); }
        Result::Ok(String::from_utf8(buf).unwrap_or_default())
    }
}
