//! Tests for `glyim_drop_in_place` FFI function.
//!
//! Tests that `drop_in_place` correctly calls the provided drop function
//! and handles null/None cases safely.

use crate::{DropFn, glyim_alloc, glyim_dealloc, glyim_drop_in_place};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn test_drop_in_place_is_callable() {
    let ptr = glyim_alloc(8, 8);
    assert!(!ptr.is_null());
    unsafe {
        let typed = ptr as *mut u64;
        typed.write(12345u64);
    }
    // glyim_drop_in_place with None (trivially destructible) should not crash
    unsafe {
        glyim_drop_in_place(ptr, None);
    }
    // After drop_in_place with no drop_fn, memory is still valid
    // (the type is trivially destructible)
    unsafe {
        let typed = ptr as *mut u64;
        assert_eq!(
            *typed, 12345u64,
            "trivially destructible types should not be modified"
        );
    }
    unsafe {
        glyim_dealloc(ptr, 8, 8);
    }
}

#[test]
fn test_drop_in_place_null_pointer() {
    // Should not crash even with null
    unsafe {
        glyim_drop_in_place(std::ptr::null_mut(), None);
    }
}

#[test]
fn test_alloc_and_drop_in_place_sequence() {
    // Simulates the pattern: allocate, write, drop_in_place, dealloc
    for i in 0..10 {
        let ptr = glyim_alloc(16, 8);
        assert!(!ptr.is_null(), "allocation {i} failed");
        unsafe {
            std::ptr::write_bytes(ptr, i as u8, 16);
        }
        unsafe {
            glyim_drop_in_place(ptr, None);
        }
        unsafe {
            glyim_dealloc(ptr, 16, 8);
        }
    }
}

#[test]
fn test_drop_in_place_multiple_calls() {
    // Call drop_in_place multiple times on different allocations
    for _ in 0..50 {
        let ptr = glyim_alloc(32, 8);
        assert!(!ptr.is_null());
        unsafe {
            std::ptr::write_bytes(ptr, 0xFF, 32);
        }
        unsafe {
            glyim_drop_in_place(ptr, None);
        }
        unsafe {
            glyim_dealloc(ptr, 32, 8);
        }
    }
}

#[test]
fn test_drop_in_place_calls_drop_fn() {
    // Verify that glyim_drop_in_place actually calls the provided drop function
    static DROP_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_ptr: *mut u8) {
        DROP_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    let drop_fn: Option<DropFn> = Some(counting_drop);
    DROP_CALL_COUNT.store(0, Ordering::SeqCst);

    let ptr = glyim_alloc(8, 8);
    assert!(!ptr.is_null());
    unsafe {
        glyim_drop_in_place(ptr, drop_fn);
    }
    assert_eq!(
        DROP_CALL_COUNT.load(Ordering::SeqCst),
        1,
        "drop_fn should have been called exactly once"
    );
    unsafe {
        glyim_dealloc(ptr, 8, 8);
    }
}

#[test]
fn test_drop_in_place_none_does_not_call_anything() {
    // Verify that None means no destructor is called
    static UNEXPECTED_DROP: AtomicUsize = AtomicUsize::new(0);

    // This should never be called
    unsafe extern "C" fn bad_drop(_ptr: *mut u8) {
        UNEXPECTED_DROP.fetch_add(1, Ordering::SeqCst);
    }

    let ptr = glyim_alloc(8, 8);
    assert!(!ptr.is_null());
    unsafe {
        glyim_drop_in_place(ptr, None);
    }
    assert_eq!(
        UNEXPECTED_DROP.load(Ordering::SeqCst),
        0,
        "no drop function should have been called with None"
    );
    unsafe {
        glyim_dealloc(ptr, 8, 8);
    }
    // Just to verify the unused variable doesn't warn
    let _ = bad_drop as DropFn;
}

#[test]
fn test_drop_in_place_multiple_drops_with_fn() {
    static TOTAL_DROPS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_ptr: *mut u8) {
        TOTAL_DROPS.fetch_add(1, Ordering::SeqCst);
    }

    let drop_fn: Option<DropFn> = Some(counting_drop);
    TOTAL_DROPS.store(0, Ordering::SeqCst);

    for _ in 0..20 {
        let ptr = glyim_alloc(16, 8);
        assert!(!ptr.is_null());
        unsafe {
            std::ptr::write_bytes(ptr, 0xAB, 16);
            glyim_drop_in_place(ptr, drop_fn);
            glyim_dealloc(ptr, 16, 8);
        }
    }

    assert_eq!(
        TOTAL_DROPS.load(Ordering::SeqCst),
        20,
        "drop_fn should have been called 20 times"
    );
}

#[test]
fn test_drop_fn_receives_correct_pointer() {
    static LAST_DROP_PTR: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn recording_drop(ptr: *mut u8) {
        LAST_DROP_PTR.store(ptr as usize, Ordering::SeqCst);
    }

    let drop_fn: Option<DropFn> = Some(recording_drop);

    let ptr = glyim_alloc(8, 8);
    assert!(!ptr.is_null());
    unsafe {
        glyim_drop_in_place(ptr, drop_fn);
    }
    assert_eq!(
        LAST_DROP_PTR.load(Ordering::SeqCst),
        ptr as usize,
        "drop_fn should receive the exact pointer passed to glyim_drop_in_place"
    );
    unsafe {
        glyim_dealloc(ptr, 8, 8);
    }
}

#[test]
fn test_drop_in_place_null_with_drop_fn() {
    // null ptr should not call drop_fn even if one is provided
    static DROP_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_ptr: *mut u8) {
        DROP_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    let drop_fn: Option<DropFn> = Some(counting_drop);
    DROP_CALL_COUNT.store(0, Ordering::SeqCst);

    unsafe {
        glyim_drop_in_place(std::ptr::null_mut(), drop_fn);
    }
    assert_eq!(
        DROP_CALL_COUNT.load(Ordering::SeqCst),
        0,
        "drop_fn should NOT be called for null pointer"
    );
}
