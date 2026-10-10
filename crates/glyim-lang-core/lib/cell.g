//! Shareable mutable containers for the Glyim core library.

/// The core primitive for interior mutability in Glyim.
///
/// `UnsafeCell<T>` is a wrapper type that allows interior mutability.
struct UnsafeCell<T> {
    value: T,
}

impl<T> UnsafeCell<T> {
    /// Constructs a new instance of `UnsafeCell` which wraps the given value.
    fn new(value: T) -> UnsafeCell<T> {
        UnsafeCell { value }
    }

    /// Returns a raw pointer to the underlying data.
    ///
    /// T140: `&self.value as *mut T` — a shared reference to the field cast
    /// to a mutable raw pointer. `&T as *mut T` is a legal RawPtr cast.
    fn get(&self) -> *mut T {
        &self.value as *mut T
    }

    /// Unwraps the value, returning the underlying data.
    fn into_inner(self) -> T {
        self.value
    }
}

/// A mutable memory location with dynamically checked borrow rules.
///
/// `Cell<T>` is a safe wrapper around `UnsafeCell<T>` for `Copy` types.
struct Cell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> Cell<T> {
    /// Creates a new `Cell` containing the given value.
    fn new(value: T) -> Cell<T> {
        Cell { value: UnsafeCell::new(value) }
    }

    /// Returns a copy of the contained value.
    fn get(&self) -> T {
        unsafe { *self.value.get() }
    }

    /// Sets the contained value.
    fn set(&self, val: T) {
        unsafe { *self.value.get() = val; }
    }

    /// Swaps the contained value with `other`.
    fn swap(&self, other: &Cell<T>) {
        unsafe {
            let tmp = *self.value.get();
            *self.value.get() = *other.value.get();
            *other.value.get() = tmp;
        }
    }

    /// Replaces the contained value with `val`, and returns the old contained value.
    fn replace(&self, val: T) -> T {
        unsafe {
            let old = *self.value.get();
            *self.value.get() = val;
            old
        }
    }
}

/// A mutable memory location with dynamically checked borrow rules.
///
/// Unlike `Cell<T>`, `RefCell<T>` allows borrowing the value at runtime.
struct RefCell<T> {
    borrow: Cell<isize>,
    value: UnsafeCell<T>,
}

impl<T> RefCell<T> {
    /// Creates a new `RefCell` containing `value`.
    fn new(value: T) -> RefCell<T> {
        RefCell {
            borrow: Cell::new(0),
            value: UnsafeCell::new(value),
        }
    }

    /// Immutably borrows the wrapped value.
    ///
    /// Panics if the value is currently mutably borrowed.
    fn borrow(&self) -> Ref<T> {
        if self.borrow.get() < 0 {
            panic!("RefCell already mutably borrowed");
        }
        self.borrow.set(self.borrow.get() + 1);
        Ref {
            value: unsafe { &*self.value.get() },
            borrow: &self.borrow,
        }
    }

    /// Mutably borrows the wrapped value.
    ///
    /// Panics if the value is currently borrowed.
    fn borrow_mut(&self) -> RefMut<T> {
        if self.borrow.get() != 0 {
            panic!("RefCell already borrowed");
        }
        self.borrow.set(-1);
        RefMut {
            value: unsafe { &mut *self.value.get() },
            borrow: &self.borrow,
        }
    }

    /// Consumes the `RefCell`, returning the wrapped value.
    fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

/// A wrapper type for an immutably borrowed value from a `RefCell<T>`.
struct Ref<'b, T> {
    value: &'b T,
    borrow: &'b Cell<isize>,
}

impl<T> Deref for Ref<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.value }
}

impl<T> Drop for Ref<'_, T> {
    fn drop(&mut self) {
        let b = self.borrow.get();
        self.borrow.set(b - 1);
    }
}

/// A wrapper type for a mutably borrowed value from a `RefCell<T>`.
struct RefMut<'b, T> {
    value: &'b mut T,
    borrow: &'b Cell<isize>,
}

impl<T> Deref for RefMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.value }
}

impl<T> DerefMut for RefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.value }
}

impl<T> Drop for RefMut<'_, T> {
    fn drop(&mut self) {
        self.borrow.set(0);
    }
}
