//! Tests for memory allocation FFI functions.
use crate::{glyim_alloc, glyim_dealloc, glyim_drop_in_place};

unsafe extern "C" fn noop_drop_for_test(_ptr: *mut u8) {}
#[test]
fn t01_alloc_returns_non_null_aligned() {
    let size = 64usize;
    let align = 8usize;
    let ptr = glyim_alloc(size, align);
    assert!(!ptr.is_null(), "glyim_alloc should return non-null pointer");
    let addr = ptr as usize;
    assert_eq!(
        addr % align,
        0,
        "Pointer should be aligned to {align} bytes"
    );
    unsafe { glyim_dealloc(ptr, size, align) };
}
#[test]
fn t01_alloc_zero_size_returns_dangling() {
    let ptr = glyim_alloc(0, 8);
    assert!(
        !ptr.is_null(),
        "Zero-size alloc should return dangling, not null"
    );
}
#[test]
fn t01_alloc_large_alignment() {
    let size = 32usize;
    let align = 64usize;
    let ptr = glyim_alloc(size, align);
    assert!(!ptr.is_null());
    let addr = ptr as usize;
    assert_eq!(addr % align, 0);
    unsafe { glyim_dealloc(ptr, size, align) };
}
#[test]
fn t01_drop_in_place_null_safe() {
    unsafe { glyim_drop_in_place(std::ptr::null_mut(), None) };
    unsafe { glyim_drop_in_place(std::ptr::null_mut(), Some(noop_drop_for_test)) };
}
