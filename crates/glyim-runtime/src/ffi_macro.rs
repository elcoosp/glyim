// FFI macro for plan §3.3 — centralizes safety in a declarative macro.
//
// Usage for read-oriented functions:
//   ffi_fn!(read glyim_fs_read(buf: *mut u8, buf_len: usize) |slice| file.read(slice))
//
// Usage for write-oriented functions:
//   ffi_fn!(write glyim_fs_write(buf: *const u8, buf_len: usize) |slice| file.write(slice))

macro_rules! ffi_fn {
    // Read variant: mutable slice, returns isize with byte count or error
    (
        read $name:ident(
            $buf:ident: *mut u8,
            $buf_len:ident: usize
        ) |$slice:ident| {
            $($body:tt)*
        }
    ) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            fd: i32,
            $buf: *mut u8,
            $buf_len: usize,
        ) -> isize {
            if $buf.is_null() {
                return FS_EIO as isize;
            }
            let $slice = unsafe { crate::fs::slice_from_raw_parts_mut($buf, $buf_len) };
            let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
            let file = match table.get_mut(fd) {
                Some(f) => f,
                None => return FS_EBADF as isize,
            };
            match file.$($body)* {
                Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
                Err(e) => io_err_to_errno(&e) as isize,
            }
        }
    };
    // Write variant: immutable slice, returns isize with byte count or error
    (
        write $name:ident(
            $buf:ident: *const u8,
            $buf_len:ident: usize
        ) |$slice:ident| {
            $($body:tt)*
        }
    ) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            fd: i32,
            $buf: *const u8,
            $buf_len: usize,
        ) -> isize {
            if $buf.is_null() && $buf_len > 0 {
                return FS_EIO as isize;
            }
            let $slice = unsafe { crate::fs::slice_from_raw_parts($buf, $buf_len) };
            let mut table = fs_table().lock().unwrap_or_else(|e| e.into_inner());
            let file = match table.get_mut(fd) {
                Some(f) => f,
                None => return FS_EBADF as isize,
            };
            match file.$($body)* {
                Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
                Err(e) => io_err_to_errno(&e) as isize,
            }
        }
    };
}

#[cfg(test)]
mod test_ffi_fn_macro {
    // Test that ffi_fn! macro expands correctly.
    // These are tested in compile time (the macro expansion is checked by cargo check).
    // The runtime tests for glyim_fs_read/glyim_fs_write verify the generated code works.
    use super::*;

    #[test]
    fn ffi_fn_macro_validates() {
        // This test just ensures the module compiles; the real validation
        // is that glyim_fs_read/glyim_fs_write use the macro and pass their tests.
    }
}