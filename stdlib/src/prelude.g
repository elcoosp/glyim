// === Types ===
pub enum Option<T> {
    Some(T),
    None,
}

pub enum Result<T, E> {
    Ok(T),
    Err(E),
}

// === Allocator ===
extern {
    fn glyim_alloc(size: i64) -> *mut u8;
    fn glyim_free(ptr: *mut u8);
}

// === Intrinsics ===
extern {
    fn __size_of<T>() -> i64;
    fn __ptr_offset(ptr: *mut u8, offset: i64) -> *mut u8;
    fn write(fd: i64, buf: *const u8, len: i64) -> i64;
    fn abort();
}
