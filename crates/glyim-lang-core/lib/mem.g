//! Memory management functions for the Glyim core library.

/// Replaces the value at `dest` with `src`, returning the old `dest` value.
fn replace<T>(dest: &mut T, src: T) -> T {
    let old = unsafe { ptr::read(dest as *mut T) };
    unsafe { ptr::write(dest as *mut T, src); }
    old
}

/// Swaps the values at two mutable locations, without deinitializing either one.
fn swap<T>(x: &mut T, y: &mut T) {
    let tmp = unsafe { ptr::read(x as *mut T) };
    unsafe { ptr::write(x as *mut T, ptr::read(y as *mut T)); }
    unsafe { ptr::write(y as *mut T, tmp); }
}

/// Returns the size of a type in bytes.
fn size_of<T>() -> usize {
    // compiler intrinsic
}

/// Returns the ABI-required minimum alignment of a type in bytes.
fn align_of<T>() -> usize {
    // compiler intrinsic
}

/// Returns the size of the pointed-to value in bytes.
///
/// The stdlib uses this from `Drop` impls (`mem::size_of_val(self)` inside
/// `impl<T> Drop for Box<T>`). The reference is to a sized type, so this is
/// the size of that referent — equivalent to `size_of::<T>()` here.
fn size_of_val<T>(_val: &T) -> usize {
    size_of::<T>()
}

/// Returns the ABI-required minimum alignment of the type of the value
/// pointed to by the reference.
fn align_of_val<T>(_val: &T) -> usize {
    align_of::<T>()
}

/// Returns `true` if dropping values of type `T` matters.
fn needs_drop<T>() -> bool {
    // compiler intrinsic
}

/// Forgets about `value` without running its destructor.
fn forget<T>(value: T) {
    // compiler intrinsic - skips drop
}

/// Takes ownership and returns the value, replacing it with `Default::default()`.
fn take<T: Default>(dest: &mut T) -> T {
    replace(dest, T::default())
}

/// A wrapper type that inhibits the compiler from automatically calling `T`'s destructor.
struct ManuallyDrop<T> {
    value: T,
}

impl<T> ManuallyDrop<T> {
    /// Wraps a value to be manually dropped.
    fn new(value: T) -> ManuallyDrop<T> {
        ManuallyDrop { value }
    }

    /// Extracts the value from the `ManuallyDrop` container.
    fn into_inner(slot: ManuallyDrop<T>) -> T {
        slot.value
    }
}

impl<T> Deref for ManuallyDrop<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.value }
}

impl<T> DerefMut for ManuallyDrop<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.value }
}

/// A wrapper type that may represent an uninitialized value.
struct MaybeUninit<T> {
    // compiler intrinsic representation
}

impl<T> MaybeUninit<T> {
    /// Creates a new `MaybeUninit` in an uninitialized state.
    fn uninit() -> MaybeUninit<T> {
        // compiler intrinsic
    }

    /// Creates a new `MaybeUninit` with the given value.
    fn new(val: T) -> MaybeUninit<T> {
        // compiler intrinsic
    }

    /// Assumes the value is initialized and returns it.
    fn assume_init(self) -> T {
        // compiler intrinsic - unsafe
    }
}
