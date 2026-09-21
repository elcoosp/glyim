//! Core type system & Type Context.
/// adt_def.
pub mod adt_def;
/// auto_trait.
pub mod auto_trait;
/// binder.
pub mod binder;
pub mod cast;
/// deref.
pub mod deref;
pub use cast::is_valid_cast;
/// const_val.
pub mod const_val;
/// display.
pub mod display;
/// flags.
pub mod flags;
/// fn_sig.
pub mod fn_sig;
pub mod lang_items;
/// predicate.
pub mod predicate;
/// region.
pub mod region;
/// substitution.
pub mod substitution;
/// ty.
pub mod ty;
/// ty_ctx.
pub mod ty_ctx;
/// ty_ctx_mut.
pub mod ty_ctx_mut;
/// type_arena: per-compilation canonical type interner (P0).
pub mod type_arena;

pub use lang_items::{LangItem, LangItemError, LangItems};

pub use adt_def::*;
pub use auto_trait::*;
pub use binder::*;
pub use const_val::*;
pub use deref::DerefRegistry;
pub use display::*;
pub use flags::*;
pub use fn_sig::*;
pub use predicate::*;
pub use region::*;
pub use substitution::*;
pub use ty::*;
pub use ty_ctx::*;
pub use ty_ctx_mut::*;

pub mod object_safety;

#[cfg(test)]
mod tests;

/// Definition of a trait for the type context.
#[derive(Clone, Debug)]
pub struct TraitDef {
    /// Struct.
    pub name: glyim_core::interner::Name,
    /// Struct.
    pub methods: Vec<MethodDef>,
    /// Associated-type names declared by the trait (e.g. `Output` in
    /// `trait Future { type Output; }`). Populated during HIR lowering
    /// (plan unstub-5 P5) so impls can be checked against the trait's
    /// associated-type surface and projections resolved.
    pub associated_types: Vec<glyim_core::interner::Name>,
}

/// Definition of a method in a trait.
#[derive(Clone, Debug)]
pub struct MethodDef {
    /// Struct.
    pub name: glyim_core::interner::Name,
    /// Struct.
    pub sig: FnSig,
    /// The `FnDefId` of the (canonical, trait-level) method definition.
    /// When a vtable is generated for a concrete impl, this identifies the
    /// method whose monomorphized body should be dispatched. `None` when the
    /// method's def id is not yet known (e.g. built-in traits).
    pub fn_def_id: Option<glyim_core::def_id::FnDefId>,
}
