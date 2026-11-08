//! `VTable` layout and construction for trait objects.
//!
//! A vtable is a structure containing function pointers for all methods
//! of a trait, plus metadata (drop, size, alignment). It is emitted as
//! a static constant in the codegen backend and referenced by trait
//! object fat pointers.

use crate::{Align, Size};
use glyim_core::{FnDefId, Name, TraitDefId};
use glyim_type::*;

/// Represents a single entry in a vtable: a pointer to a method or metadata function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VTableEntry {
    /// Name of the method (for debugging)
    pub name: Name,
    /// The function type signature
    pub sig: FnSig,
    /// The `DefId` of the concrete method being pointed to
    pub fn_def_id: FnDefId,
}

/// The layout of a vtable for a specific trait and concrete type.
#[derive(Debug, Clone)]
pub struct VTableLayout {
    /// The trait this vtable implements
    pub trait_def_id: TraitDefId,
    /// The concrete type this vtable is for
    pub concrete_ty: Ty,
    /// Size of the concrete type (for deallocation)
    pub size: Size,
    /// Alignment of the concrete type (for deallocation)
    pub align: Align,
    /// Drop function pointer (null if no drop needed)
    pub drop_fn: Option<FnDefId>,
    /// Method entries in vtable order
    pub methods: Vec<VTableEntry>,
}

/// Computed size and alignment of a vtable in memory.
#[derive(Debug, Clone, Copy)]
pub struct VTableSize {
    /// Total size of the vtable in bytes
    pub size: u64,
    /// Alignment of the vtable (typically pointer alignment)
    pub align: u64,
}

impl VTableLayout {
    /// Compute the memory layout (size and alignment) of this vtable.
    #[must_use]
    pub fn memory_size(&self, ptr_size: u64) -> VTableSize {
        // Vtable structure:
        // [0] drop_fn pointer (or null)
        // [1] size: usize
        // [2] align: usize
        // [3..] method pointers
        let entry_count = 3 + self.methods.len();
        VTableSize {
            size: entry_count as u64 * ptr_size,
            align: ptr_size,
        }
    }

    /// Returns the offset (in bytes) of the method pointer at the given index.
    #[must_use]
    pub fn method_offset(&self, index: usize, ptr_size: u64) -> u64 {
        // Skip drop_fn, size, align
        (3 + index) as u64 * ptr_size
    }
}

/// Trait for layout computers that can compute vtable layouts.
pub trait VTableComputer {
    /// Compute the vtable layout for a given trait and concrete type.
    ///
    /// Returns `Err(LayoutError::UnknownTrait)` when the trait definition
    /// cannot be resolved, rather than silently producing an empty vtable
    /// (plan §10.1). Supertrait/associated-type ordering is coordinated with
    /// the trait solver (§9.4).
    fn vtable_of(
        &self,
        trait_def_id: TraitDefId,
        concrete_ty: Ty,
    ) -> Result<VTableLayout, crate::LayoutError>;
}
