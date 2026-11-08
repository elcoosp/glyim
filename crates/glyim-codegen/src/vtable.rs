//! `VTable` layout constants and utilities for code generation backends.
//!
//! A vtable is a structure in memory containing pointers to the concrete
//! implementations of the methods of a trait, plus metadata for drop, size,
//! and alignment. The layout is:
//!
//! | Offset              | Content                       |
//! |---------------------|-------------------------------|
//! | 0 * ptr_size        | drop function pointer         |
//! | 1 * ptr_size        | size (usize)                  |
//! | 2 * ptr_size        | align (usize)                 |
//! | 3 * ptr_size + ...  | method pointers               |

/// Index of the drop function pointer entry in the vtable.
pub const VTABLE_DROP_FN_INDEX: usize = 0;

/// Index of the size entry (as usize) in the vtable.
pub const VTABLE_SIZE_INDEX: usize = 1;

/// Index of the align entry (as usize) in the vtable.
pub const VTABLE_ALIGN_INDEX: usize = 2;

/// Starting index of method pointers in the vtable.
pub const VTABLE_METHODS_START: usize = 3;

/// Returns the vtable index for a given method index (0-based).
/// The method pointer for method `i` is stored at `VTABLE_METHODS_START + i`.
#[must_use]
pub const fn method_index(i: usize) -> usize {
    VTABLE_METHODS_START + i
}

/// Number of metadata slots before the method pointers.
pub const VTABLE_METADATA_ENTRIES: usize = VTABLE_METHODS_START;
