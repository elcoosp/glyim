//! Heap-allocated owned pointer `Box<T>` for the Glyim alloc library.

use alloc::{GlobalAlloc, Layout};

/// A pointer type that uniquely owns a heap allocation of type `T`.
struct Box<T> {
    ptr: *mut T,
}

impl<T> Box<T> {
    /// Allocate memory on the heap and move `value` into it.
    fn new(value: T) -> Self {
        let layout = Layout::from_size_align(
            mem::size_of::<T>(),
            mem::align_of::<T>(),
        ).expect("Box layout invalid");
        let ptr = GLOBAL.alloc(layout) as *mut T;
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        ptr::write(ptr, value);
        Box { ptr }
    }
}

impl<T> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

impl<T> Drop for Box<T> {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(
            mem::size_of_val(self),
            mem::align_of_val(self),
        ).expect("Box layout invalid at drop");
        unsafe {
            ptr::drop_in_place(self.ptr);
            GLOBAL.dealloc(self.ptr as *mut u8, layout);
        }
    }
}

impl<T> Box<T> {
    /// Consume the box, returning the raw pointer without running `Drop`.
    ///
    /// The caller now owns the allocation and must eventually convert it back
    /// with `Box::from_raw` (or otherwise deallocate it with the same global
    /// allocator) or the memory leaks.
    fn into_raw(b: Box<T>) -> *mut T {
        let ptr = b.ptr;
        mem::forget(b);
        ptr
    }

    /// Reconstruct a `Box<T>` from a raw pointer previously produced by
    /// `Box::into_raw` (or otherwise pointing at a live `T` allocated with the
    /// global allocator using `T`'s layout).
    ///
    /// # Safety
    /// `ptr` must have been obtained from `Box::into_raw` (same `T`, same
    /// allocator) and must not already have been converted back into a `Box`.
    unsafe fn from_raw(ptr: *mut T) -> Box<T> {
        Box { ptr }
    }

    /// Consume the box and return a `&'static mut T`, permanently leaking the
    /// allocation (it is never freed). Useful for FFI registries and
    /// process-lifetime singletons.
    fn leak(b: Box<T>) -> &'static mut T {
        let ptr = b.ptr;
        mem::forget(b);
        unsafe { &mut *ptr }
    }
}
