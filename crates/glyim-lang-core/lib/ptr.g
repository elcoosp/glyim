//! Pointer-related types and functions for the Glyim core library.

/// A wrapper around a raw non-null pointer.
struct NonNull<T> {
    pointer: *const T,
}

impl<T> NonNull<T> {
    /// Creates a new `NonNull` if the pointer is not null.
    fn new(ptr: *mut T) -> Option<Self> {
        if !ptr.is_null() {
            Option::Some(NonNull { pointer: ptr })
        } else {
            Option::None
        }
    }

    /// Creates a new `NonNull` without checking for null.
    fn new_unchecked(ptr: *mut T) -> Self {
        // unsafe - compiler intrinsic
    }

    /// Creates a dangling but well-aligned `NonNull`.
    fn dangling() -> Self {
        // compiler intrinsic
    }

    /// Returns the pointer as a raw pointer.
    fn as_ptr(self) -> *const T {
        self.pointer
    }

    /// Returns the pointer as a raw mutable pointer.
    fn as_mut_ptr(&mut self) -> *mut T {
        self.pointer as *mut T
    }
}

/// Creates a null raw pointer.
///
/// T140-PATCHED [STD-15]: implemented as a zero-cast. `Int → RawPtr` is
/// accepted by `is_valid_cast`, so `0 as *const T` folds to the null
/// pointer without needing a compiler intrinsic.
fn null<T>() -> *const T {
    0 as *const T
}

/// Creates a null mutable raw pointer.
///
/// T140-PATCHED [STD-15]: zero-cast, matching `ptr::null`.
fn null_mut<T>() -> *mut T {
    0 as *mut T
}

/// Reads the value from `src` without moving it.
fn read<T>(src: *const T) -> T {
    // compiler intrinsic - unsafe
}

/// Writes `src` to `dst`.
fn write<T>(dst: *mut T, src: T) {
    // compiler intrinsic - unsafe
}

/// Copies bytes from `src` to `dst`. The source and destination may overlap.
fn copy<T>(src: *const T, dst: *mut T, count: usize) {
    // compiler intrinsic - unsafe
}

/// Copies bytes from `src` to `dst`. The source and destination must not overlap.
fn copy_nonoverlapping<T>(src: *const T, dst: *mut T, count: usize) {
    // compiler intrinsic - unsafe
}

/// Executes the destructor (if any) of the pointed-to value.
fn drop_in_place<T>(to_drop: *mut T) {
    // compiler intrinsic - unsafe
}
