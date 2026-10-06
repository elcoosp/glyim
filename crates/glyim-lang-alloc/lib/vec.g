//! A contiguous growable array type, `Vec<T>`, for the Glyim alloc library.

use raw_vec::RawVec;
use ops::{Index, IndexMut};
use iter::{IntoIterator, FromIterator};
use clone::Clone;
use default::Default;

/// A growable array.
struct Vec<T> {
    buf: RawVec<T>,
    len: usize,
}

impl<T> Vec<T> {
    /// Creates a new empty `Vec`.
    fn new() -> Self {
        Vec {
            buf: RawVec::new(),
            len: 0,
        }
    }

    /// Appends an element to the back.
    fn push(&mut self, value: T) {
        if self.len == self.buf.capacity() {
            self.buf.reserve(self.len + 1);
        }
        unsafe {
            let end = self.buf.as_mut_ptr().add(self.len);
            ptr::write(end, value);
        }
        self.len += 1;
    }

    /// Removes the last element and returns it, or `None` if empty.
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            Option::None
        } else {
            self.len -= 1;
            unsafe { Option::Some(ptr::read(self.buf.as_ptr().add(self.len))) }
        }
    }

    /// Returns the number of elements.
    fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the vector contains no elements.
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a slice containing the entire vector.
    fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.buf.as_ptr(), self.len) }
        }
    }

    /// Extends the vector with elements from a slice.
    // T062-PATCHED [STD-9]: the body uses `ptr::read(item)` to bit-
    // duplicate each element; without `T: Copy` this aliases ownership
    // and double-frees non-Copy values like `String` (both vectors drop
    // the same heap buffer). The bound makes the bit-copy sound.
    fn extend_from_slice(&mut self, other: &[T]) where T: Copy {
        self.buf.reserve(self.len + other.len());
        for item in other {
            unsafe {
                let end = self.buf.as_mut_ptr().add(self.len);
                ptr::write(end, ptr::read(item));
            }
            self.len += 1;
        }
    }
}

impl<T> Default for Vec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for Vec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut vec = Vec::new();
        for item in iter {
            vec.push(item);
        }
        vec
    }
}

impl<T> Index<usize> for Vec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        assert!(index < self.len, "Vec index out of bounds");
        unsafe { &*self.buf.as_ptr().add(index) }
    }
}

impl<T> IndexMut<usize> for Vec<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        assert!(index < self.len, "Vec index out of bounds");
        unsafe { &mut *self.buf.as_mut_ptr().add(index) }
    }
}
