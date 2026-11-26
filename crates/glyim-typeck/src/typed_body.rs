//! The typed THIR boundary (plan M5.2, CT-4): the only form of a
//! type-checked body that leaves `glyim-typeck`.
//!
//! # Why this exists
//!
//! `typeck_crate` zones THIR bodies and folds every inference variable, but
//! two failure shapes historically survived the fold and flowed into MIR
//! lowering as *raw* `thir::Body` values:
//!
//! 1. an unbound `Infer(Ty(_))` folds to the [`Ty::ERROR`] sentinel, which
//!    reaches codegen inside ADT substitutions and ICEs the LLVM layout pass
//!    with `UnknownType(Ty(0))`;
//! 2. a `Ty` slot added to THIR without extending `zonk::walk_*` keeps a raw
//!    `Infer(Int(_))`/`Infer(Float(_))` — caught only by a `debug_assert!`,
//!    i.e. silently in release builds.
//!
//! [`TypedBody`] closes both: it is constructible *only* through
//! [`TypedBody::try_from_body`], which deep-checks the body's declared
//! signature slots (parameter and return types, including types nested in
//! ADT/fn/closure substitutions) and refuses any slot that still carries an
//! inference variable or the error sentinel. Lowering accepts only
//! `TypedBody` (plan M5.2: "lowering accepts only `TypedBody`"), so an
//! unverified body is unrepresentable at the lowering boundary (constitution
//! C3: make illegal states unrepresentable).
//!
//! # What is deliberately *not* checked here
//!
//! Expression-level slots are not rejected wholesale: zonk folds unbound
//! type variables to `Ty::ERROR` for *benign* shapes too (a discarded
//! `"3.14".parse::<f64>()` leaves an error-carrying call type that never
//! reaches a value-construction site — codegen handles it fine, see
//! `zonk::collect_unresolved_ty_var_spans` for the narrower
//! value-construction gate and the module docs of `zonk` for the
//! rationale). Wholesale rejection there would reject programs the compiler
//! can in fact build. The plan's acceptance criterion — "no inference vars,
//! no error types" — is enforced on the signature slots, which are exactly
//! the slots MIR lowering copies into local declarations and ABI
//! classification; value-construction shapes stay covered by the zonk-level
//! `Result` and the pipeline's reachability-gated diagnostic.

use glyim_core::def_id::DefId;
use glyim_span::Span;
use glyim_type::{GenericArg, Ty, TyCtx, TyKind};

use crate::thir;

/// A type slot known to be fully resolved: it carries neither an inference
/// variable nor the [`Ty::ERROR`] sentinel at any depth (plan M5.2:
/// "`TypedBody` holds `ResolvedTy`").
///
/// Minted exclusively by [`TypedBody::try_from_body`] — there is no public
/// constructor — and read back through [`ResolvedTy::get`] where a raw `Ty`
/// is still needed by mid-end consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTy(Ty);

impl ResolvedTy {
    /// The raw `Ty` handle, for consumers below the typed boundary.
    #[inline]
    #[must_use]
    pub fn get(self) -> Ty {
        self.0
    }
}

/// One signature slot that failed the checked-construction walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedTy {
    /// Span to point the diagnostic at (`Span::DUMMY` for the body-level
    /// return type of a hand-built body).
    pub span: Span,
    /// The offending type (the raw `Infer(_)`/`Ty::ERROR` slot).
    pub ty: Ty,
}

/// A THIR body whose declared signature — every parameter type and the
/// return type, deeply, including types nested in ADT/fn/closure
/// substitutions — is fully resolved (plan M5.2 "Typed THIR").
///
/// Construction is checked ([`TypedBody::try_from_body`] is the only
/// constructor), so "lowering accepts only `TypedBody`" is enforceable by
/// the function signature of [`crate::lower_typed_body`-style entry points]:
/// a body that failed zonk cannot reach MIR lowering at all.
#[derive(Clone, Debug)]
pub struct TypedBody {
    inner: thir::Body,
}

impl TypedBody {
    /// Check every declared signature slot and wrap the body.
    ///
    /// # Errors
    /// Returns the first offending slot — an `Infer(_)` or `Ty::ERROR` type
    /// at any depth inside a parameter or the return type — so the caller
    /// can diagnose it (the pipeline keeps the pre-existing
    /// reachability-gated "type annotations needed" path via the zonk
    /// `Result`).
    pub fn try_from_body(inner: thir::Body, ctx: &TyCtx) -> Result<Self, UnresolvedTy> {
        for param in &inner.params {
            if let Some(offender) = find_first_unresolved(ctx, param.ty) {
                return Err(UnresolvedTy {
                    span: param.span,
                    ty: offender,
                });
            }
        }
        if let Some(offender) = find_first_unresolved(ctx, inner.return_ty) {
            return Err(UnresolvedTy {
                span: inner.span,
                ty: offender,
            });
        }
        Ok(Self { inner })
    }

    /// Bridged read access to the verified body (statements, spans, owner).
    #[inline]
    #[must_use]
    pub fn get(&self) -> &thir::Body {
        &self.inner
    }

    /// The owning definition.
    #[inline]
    #[must_use]
    pub fn owner(&self) -> DefId {
        self.inner.owner
    }

    /// The resolved return type (plan M5.2: signature slots are
    /// `ResolvedTy`).
    #[inline]
    #[must_use]
    pub fn return_ty(&self) -> ResolvedTy {
        ResolvedTy(self.inner.return_ty)
    }

    /// The resolved parameter types, in declaration order (plan M5.2:
    /// signature slots are `ResolvedTy`).
    pub fn param_tys(&self) -> impl Iterator<Item = ResolvedTy> + '_ {
        self.inner.params.iter().map(|p| ResolvedTy(p.ty))
    }
}

/// Depth-first search for an `Infer(_)` or `Ty::ERROR` slot, following
/// references, raw pointers, slices, arrays, tuples, ADT/fn/closure/opaque
/// substitutions and function-pointer signatures — the same traversal
/// `zonk::walk_ty` uses, so no `Ty` slot shape can hide a sentinel.
pub(crate) fn find_first_unresolved(ctx: &TyCtx, ty: Ty) -> Option<Ty> {
    let kind = ctx.ty_kind(ty);
    match kind {
        TyKind::Infer(_) => return Some(ty),
        TyKind::Error => return Some(ty),
        // `Ty::ERROR` is the sentinel handle; some consumers mint it without
        // registering a `TyKind::Error` entry, so compare the handle too.
        _ if ty == Ty::ERROR => return Some(ty),
        _ => {}
    }
    match kind {
        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) | TyKind::Slice(inner) => {
            find_first_unresolved(ctx, *inner)
        }
        TyKind::Array(inner, _) => find_first_unresolved(ctx, *inner),
        TyKind::Adt(_, s)
        | TyKind::FnDef(_, s)
        | TyKind::Closure(_, s)
        | TyKind::Opaque(_, s)
        | TyKind::Tuple(s) => {
            for arg in ctx.substitution_args(*s) {
                if let GenericArg::Ty(t) = arg {
                    if let Some(offender) = find_first_unresolved(ctx, *t) {
                        return Some(offender);
                    }
                }
            }
            None
        }
        TyKind::FnPtr(sig) => {
            for arg in ctx.substitution_args(sig.inputs) {
                if let GenericArg::Ty(t) = arg {
                    if let Some(offender) = find_first_unresolved(ctx, *t) {
                        return Some(offender);
                    }
                }
            }
            find_first_unresolved(ctx, sig.output)
        }
        _ => None,
    }
}
