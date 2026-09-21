use crate::ty::{FieldIdx, Ty};
use glyim_core::arena::IndexVec;
use glyim_core::interner::Name;

/// Declared syntax style of an enum variant, used to synthesize
/// arity-correct match-arm skeletons in the LSP (plan §5.1) and to carry
/// variant shape in structured diagnostics (plan §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VariantStyle {
    /// `Variant` — no associated data.
    #[default]
    Unit,
    /// `Variant(a, b)` — positional fields.
    Tuple,
    /// `Variant { x, y }` — named fields.
    Struct,
}

#[derive(Clone, Debug)]
/// AdtDef.
pub struct AdtDef {
    /// Struct.
    pub kind: AdtKind,
    /// Struct.
    pub fields: IndexVec<FieldIdx, FieldDef>,
    /// Struct.
    pub variants: Vec<VariantDef>,
    /// Names of the generic type parameters declared on the ADT
    /// (`struct S<T, U>`, `enum E<T>`). Empty for non-generic ADTs. Used to
    /// compute substitution arity when resolving type paths that carry generic
    /// arguments (unstub-5 P1.4). Stored as `Name`s (not the full HIR
    /// `GenericParam`) to avoid a `glyim-type` → `glyim-hir` dependency.
    pub generic_params: Vec<Name>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// AdtKind.
pub enum AdtKind {
    /// Variant.
    Struct,
    /// Variant.
    Enum,
    /// Variant.
    Union,
}

#[derive(Clone, Debug)]
/// VariantDef.
pub struct VariantDef {
    /// Struct.
    pub name: Name,
    /// Struct.
    pub fields: IndexVec<FieldIdx, FieldDef>,
    /// Declared syntax style of the variant (plan §5.1 / §5.2). Defaults to
    /// `Unit` for synthetic/non-enum variants; the enum-registration path sets
    /// the real style from the HIR.
    pub style: VariantStyle,
}

#[derive(Clone, Debug)]
/// FieldDef.
pub struct FieldDef {
    /// Struct.
    pub name: Name,
    /// Struct.
    pub ty: Ty,
}
