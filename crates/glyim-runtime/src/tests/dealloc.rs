//! Tests for `glyim_dealloc` FFI function.
//!
//! S06-T02: `glyim_dealloc` frees memory without crash.

use crate::{glyim_alloc, glyim_dealloc};

#[test]
fn test_dealloc_null_is_safe() {
    unsafe {
        glyim_dealloc(std::ptr::null_mut(), 8, 8);
    }
}

#[test]
fn test_dealloc_zero_size_is_safe() {
    let ptr = glyim_alloc(0, 1);
    unsafe {
        glyim_dealloc(ptr, 0, 1);
    }
}

#[test]
fn test_alloc_dealloc_varying_sizes() {
    // Test allocation and deallocation with various sizes
    for size in [
        1, 2, 4, 7, 8, 13, 16, 31, 32, 63, 64, 127, 128, 255, 256, 512, 1024, 4096,
    ] {
        let ptr = glyim_alloc(size, 8);
        assert!(!ptr.is_null(), "alloc of size {size} failed");
        unsafe {
            std::ptr::write_bytes(ptr, 0x42, size);
        }
        unsafe {
            crate::glyim_drop_in_place(ptr, None);
        }
        unsafe {
            glyim_dealloc(ptr, size, 8);
        }
    }
}

#[test]
fn test_alloc_dealloc_interleaved() {
    // Interleave allocations and deallocations to test heap consistency
    let mut live: Vec<(*mut u8, usize)> = Vec::new();
    for i in 0..100 {
        if i % 3 == 0 && !live.is_empty() {
            // Deallocate the oldest
            let (ptr, size) = live.remove(0);
            unsafe {
                crate::glyim_drop_in_place(ptr, None);
            }
            unsafe {
                glyim_dealloc(ptr, size, 8);
            }
        } else {
            let size = 8 + (i % 4) * 8;
            let ptr = glyim_alloc(size, 8);
            assert!(!ptr.is_null());
            unsafe {
                std::ptr::write_bytes(ptr, (i % 256) as u8, size);
            }
            live.push((ptr, size));
        }
    }
    // Clean up remaining
    for (ptr, size) in live {
        unsafe {
            crate::glyim_drop_in_place(ptr, None);
        }
        unsafe {
            glyim_dealloc(ptr, size, 8);
        }
    }
}

#[test]
fn dealloc_after_write_read_cycle() {
    let ptr = glyim_alloc(std::mem::size_of::<u64>(), std::mem::align_of::<u64>());
    assert!(!ptr.is_null());
    unsafe {
        let typed = ptr as *mut u64;
        typed.write(0xCAFE_BABEu64);
        assert_eq!(*typed, 0xCAFE_BABEu64);
        glyim_dealloc(ptr, std::mem::size_of::<u64>(), std::mem::align_of::<u64>());
    }
}

#[test]
fn dealloc_preserves_heap_consistency() {
    // Allocate and deallocate many times to stress the allocator
    for _ in 0..200 {
        let ptr = glyim_alloc(64, 8);
        assert!(!ptr.is_null());
        unsafe {
            std::ptr::write_bytes(ptr, 0xFF, 64);
            glyim_dealloc(ptr, 64, 8);
        }
    }
    // If we got here without a crash, the heap is consistent
}
