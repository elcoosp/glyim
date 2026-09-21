use crate::adt_def::AdtDef;
use crate::flags::TypeFlags;
use crate::predicate::Predicate;
use crate::region::{BoundRegionKind, Region};
use crate::substitution::*;
use crate::ty::*;
use glyim_core::AdtId;
use glyim_core::def_id::OpaqueTyId;
use glyim_core::interner::Name;
use glyim_core::primitives::{Abi, Mutability, Safety};
use std::fmt;

/// TypeLookup.
pub trait TypeLookup {
    /// ty_kind.
    fn ty_kind(&self, ty: Ty) -> &TyKind;
    /// ty_flags.
    fn ty_flags(&self, ty: Ty) -> TypeFlags;
    /// substitution_args.
    fn substitution_args(&self, sub: Substitution) -> &[GenericArg];
    /// name_str.
    fn name_str(&self, name: Name) -> &str;
    /// error_ty.
    fn error_ty(&self) -> Ty;
    /// Returns `true` if the given ADT has interior mutability (contains `UnsafeCell`).
    fn is_interior_mutable_adt(&self, _adt_id: AdtId) -> bool {
        false
    }
    /// Returns the `AdtDef` for the given ADT, if registered.
    /// Default returns `None`.
    fn adt_def(&self, _adt_id: AdtId) -> Option<&AdtDef> {
        None
    }
    /// Returns the concrete hidden type for an opaque type (`impl Trait` /
    /// `type X = impl Trait` position), if it has been resolved at its
    /// defining use. Used by auto-trait computation (§7.2) to recurse into the
    /// underlying type rather than assuming zero auto traits. Defaults to
    /// `None` (no hidden type known — treated as no auto traits).
    fn opaque_hidden_ty(&self, _id: OpaqueTyId) -> Option<Ty> {
        None
    }
    /// Returns the type of the field at the given index in the ADT.
    /// Default implementation uses `adt_def`; returns `error_ty()` if not found.
    fn field_ty(&self, adt_id: AdtId, field_idx: usize) -> Ty {
        self.adt_def(adt_id)
            .and_then(|def| {
                // Use as_slice().get() to avoid debug_assert! in IndexVec::get()
                // which panics on out-of-bounds even though the method returns Option.
                def.fields.as_slice().get(field_idx).map(|f| f.ty)
            })
            .unwrap_or_else(|| self.error_ty())
    }
}

const MAX_DISPLAY_DEPTH: u32 = 10;

/// PrintTy.
pub struct PrintTy<'a, L: TypeLookup> {
    ty: Ty,
    lookup: &'a L,
    depth: u32,
}

impl<'a, L: TypeLookup> PrintTy<'a, L> {
    /// new.
    pub fn new(ty: Ty, lookup: &'a L) -> Self {
        Self {
            ty,
            lookup,
            depth: 0,
        }
    }
    fn nested(&self, ty: Ty) -> Self {
        Self {
            ty,
            lookup: self.lookup,
            depth: self.depth + 1,
        }
    }
}

impl<L: TypeLookup> fmt::Display for PrintTy<'_, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.depth > MAX_DISPLAY_DEPTH {
            return write!(f, "…");
        }
        match self.lookup.ty_kind(self.ty) {
            TyKind::Bool => write!(f, "bool"),
            TyKind::Never => write!(f, "!"),
            TyKind::Unit => write!(f, "()"),
            TyKind::Int(i) => write!(f, "{}", i.name()),
            TyKind::Uint(u) => write!(f, "{}", u.name()),
            TyKind::Float(fl) => write!(f, "{}", fl.name()),
            TyKind::Char => write!(f, "char"),
            TyKind::String => write!(f, "str"),
            TyKind::Error => write!(f, "<error>"),
            TyKind::Infer(InferVar::Ty(v)) => write!(f, "?ty{}", v.to_raw()),
            TyKind::Infer(InferVar::Int(v)) => write!(f, "?int{}", v.to_raw()),
            TyKind::Infer(InferVar::Float(v)) => write!(f, "?float{}", v.to_raw()),
            TyKind::Ref(_, ty, Mutability::Mut) => write!(f, "&mut {}", self.nested(*ty)),
            TyKind::Ref(_, ty, Mutability::Not) => write!(f, "&{}", self.nested(*ty)),
            TyKind::RawPtr(ty, Mutability::Mut) => write!(f, "*mut {}", self.nested(*ty)),
            TyKind::RawPtr(ty, Mutability::Not) => write!(f, "*const {}", self.nested(*ty)),
            TyKind::Slice(ty) => write!(f, "[{}]", self.nested(*ty)),
            TyKind::Array(ty, _) => write!(f, "[{}; _]", self.nested(*ty)),
            TyKind::Tuple(substs) => {
                let args = self.lookup.substitution_args(*substs);
                write!(f, "(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match arg {
                        GenericArg::Ty(t) => write!(f, "{}", self.nested(*t))?,
                        GenericArg::Lifetime(_) => write!(f, "'_")?,
                        GenericArg::Const(_) => write!(f, "{{const}}")?,
                    }
                }
                write!(f, ")")
            }
            TyKind::FnPtr(sig) => {
                if sig.unsafety == Safety::Unsafe {
                    write!(f, "unsafe ")?;
                }
                if sig.abi != Abi::Glyim {
                    write!(f, "extern \"{}\" ", sig.abi.name())?;
                }
                write!(f, "fn(")?;
                let inputs = self.lookup.substitution_args(sig.inputs);
                for (i, arg) in inputs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match arg {
                        GenericArg::Ty(t) => write!(f, "{}", self.nested(*t))?,
                        _ => write!(f, "_")?,
                    }
                }
                if sig.c_variadic {
                    if !inputs.is_empty() {
                        write!(f, ", ")?;
                    }
                    write!(f, "...")?;
                }
                write!(f, ")")?;
                write!(f, " -> {}", self.nested(sig.output))
            }
            TyKind::Adt(adt_id, substs) => {
                write!(f, "Adt{}", adt_id.to_raw())?;
                if !substs.is_empty() {
                    write!(f, "<")?;
                    let args = self.lookup.substitution_args(*substs);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        match arg {
                            GenericArg::Ty(t) => write!(f, "{}", self.nested(*t))?,
                            GenericArg::Lifetime(_) => write!(f, "'_")?,
                            GenericArg::Const(_) => write!(f, "{{const}}")?,
                        }
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            TyKind::FnDef(fn_id, substs) => {
                write!(f, "FnDef{}", fn_id.to_raw())?;
                if !substs.is_empty() {
                    write!(f, "<...>")?;
                }
                Ok(())
            }
            TyKind::Closure(closure_id, substs) => {
                write!(f, "Closure{}", closure_id.to_raw())?;
                if !substs.is_empty() {
                    write!(f, "<...>")?;
                }
                Ok(())
            }
            TyKind::Dynamic(preds, region) => {
                write!(f, "dyn ")?;
                let predicates = preds.as_ref().skip_binder().as_ref();
                for (i, pred) in predicates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " + ")?;
                    }
                    match pred {
                        Predicate::Trait(tp) => {
                            write!(f, "Trait{}", tp.trait_ref.def_id.to_raw())?;
                        }
                        _ => write!(f, "?")?,
                    }
                }
                match region {
                    Region::Static => write!(f, " + 'static")?,
                    Region::Placeholder(p) => write!(f, " + '!{}", p.index)?,
                    _ => {}
                }
                Ok(())
            }
            TyKind::Opaque(opaque_id, substs) => {
                write!(f, "Opaque{}", opaque_id.to_raw())?;
                if !substs.is_empty() {
                    write!(f, "<...>")?;
                }
                Ok(())
            }
            TyKind::Projection(proj) => {
                write!(f, "<")?;
                let args = self.lookup.substitution_args(proj.trait_ref.substs);
                if args.len() == 1 {
                    if let GenericArg::Ty(t) = args[0] {
                        write!(f, "{}", self.nested(t))?;
                    } else {
                        write!(f, "?")?;
                    }
                } else {
                    write!(f, "?")?;
                }
                write!(
                    f,
                    " as Trait{}>::{}",
                    proj.trait_ref.def_id.to_raw(),
                    self.lookup.name_str(proj.item_name)
                )
            }
            TyKind::Param(param) => write!(f, "{}", self.lookup.name_str(param.name)),
            TyKind::Bound(var, bound) => match bound.kind {
                BoundTyKind::Param(n) => write!(f, "{}", self.lookup.name_str(n)),
                BoundTyKind::Anon => write!(f, "?{}", var),
            },
        }
    }
}

/// Display a region in type position, given a TypeLookup for resolving names.
pub fn write_region<L: TypeLookup>(
    f: &mut fmt::Formatter<'_>,
    region: &Region,
    lookup: &L,
) -> fmt::Result {
    match region {
        Region::Static => write!(f, "'static"),
        Region::EarlyBound(e) => write!(f, "'{}", e.index),
        Region::LateBound(_, _idx, kind) => match kind {
            BoundRegionKind::BrNamed(name) => write!(f, "'{}", lookup.name_str(*name)),
            BoundRegionKind::BrAnon(n) => write!(f, "'_{}", n),
            BoundRegionKind::BrEnv => write!(f, "'_env"),
        },
        Region::Var(v) => write!(f, "'?{}", v.to_raw()),
        Region::Placeholder(p) => match &p.bound {
            BoundRegionKind::BrNamed(name) => write!(f, "'!{}", lookup.name_str(*name)),
            BoundRegionKind::BrAnon(_) => write!(f, "'!{}", p.index),
            BoundRegionKind::BrEnv => write!(f, "'!env"),
        },
        Region::Erased => Ok(()),
        Region::Error => write!(f, "'<error>"),
    }
}

/// DebugTy.
pub struct DebugTy<'a, L: TypeLookup>(pub PrintTy<'a, L>);

impl<L: TypeLookup> fmt::Debug for DebugTy<'_, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
