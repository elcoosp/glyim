//! Low-level buffer management for the Glyim alloc library.

use alloc::{GlobalAlloc, Layout};
use default::Default;

/// A raw, untyped memory buffer with a capacity.
struct RawVec<T> {
    ptr: *mut T,
    cap: usize,
}

impl<T> RawVec<T> {
    /// Creates a new empty `RawVec`.
    fn new() -> Self {
        RawVec {
            ptr: ptr::null_mut(),
            cap: 0,
        }
    }

    /// Grows the buffer to have at least `required_cap` capacity.
    fn reserve(&mut self, required_cap: usize) {
        if self.cap >= required_cap {
            return;
        }
        // T061-PATCHED [STD-8]: check the multiplication `new_cap *
        // size_of::<T>()` before it is evaluated. Without this guard,
        // growth past `usize::MAX / size_of::<T>()` wraps to a tiny
        // size, `from_size_align` passes, and the following
        // `copy_nonoverlapping` copies `self.cap` huge elements into
        // the undersized buffer (heap overflow). `next_power_of_two`
        // on `required_cap > 2^63` also overflows internally.
        let elem_size = mem::size_of::<T>();
        if elem_size != 0 && required_cap > usize::MAX / elem_size {
            handle_alloc_error(Layout::from_size_align(1, 1).expect("trivial layout"));
        }
        if required_cap > usize::MAX / 2 {
            handle_alloc_error(Layout::from_size_align(1, 1).expect("trivial layout"));
        }
        let new_cap = required_cap.next_power_of_two().max(8);
        let layout = Layout::from_size_align(
            new_cap * elem_size,
            mem::align_of::<T>(),
        ).expect("RawVec layout invalid");
        let new_ptr = GLOBAL.alloc(layout) as *mut T;
        if new_ptr.is_null() {
            handle_alloc_error(layout);
        }
        if !self.ptr.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(self.ptr, new_ptr, self.cap);
            }
            let old_layout = Layout::from_size_align(
                self.cap * mem::size_of::<T>(),
                mem::align_of::<T>(),
            ).expect("Old layout invalid");
            unsafe { GLOBAL.dealloc(self.ptr as *mut u8, old_layout) };
        }
        self.ptr = new_ptr;
        self.cap = new_cap;
    }

    fn as_ptr(&self) -> *const T {
        self.ptr
    }

    fn as_mut_ptr(&self) -> *mut T {
        self.ptr
    }

    fn capacity(&self) -> usize {
        self.cap
    }
}

impl<T> Default for RawVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for RawVec<T> {
    fn drop(&mut self) {
        if self.cap == 0 || self.ptr.is_null() {
            return;
        }
        let layout = Layout::from_size_align(
            self.cap * mem::size_of::<T>(),
            mem::align_of::<T>(),
        ).expect("RawVec layout invalid at drop");
        unsafe { GLOBAL.dealloc(self.ptr as *mut u8, layout) };
    }
}
