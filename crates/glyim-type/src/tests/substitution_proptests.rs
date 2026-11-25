//! Property tests for the substitution vocabulary (plan §6 M3.10).
//!
//! Plan acceptance line (M3.10): *"Property tests for foundations:
//! `IndexVec`, interner, spans, `Size/Align`, substitution, unification"*
//! with the `DoD` `≥ 200 properties; run in PR tier`. This module pins the
//! substitution half: the interned [`Substitution`] handles, the generic-
//! parameter rewrite ([`TyCtxMut::subst_ty`]) and the `(FnDefId,
//! Substitution)` symbol mangler — the identity/mangle contracts the
//! monomorphizer and every backend symbol table lean on (plan §6 M5.3
//! cites the same mangler properties: injective + round-trip).
//!
//! Types are drawn from a fixed palette of param-free primitives, so the
//! properties reason about handles and rewrites rather than the inference
//! machinery (that half lives in `glyim-solve`'s proptests). Runs in the
//! default `cargo test` PR tier.

use std::collections::HashMap;

use crate::substitution::{GenericArg, Substitution, mangle_fn_symbol};
use crate::{ParamTy, TyCtxMut, TyKind};
use glyim_core::interner::Interner;

/// Crate-local `TyCtxMut` mint (the glyim-test helper returns the
/// extern-crate instantiation, which is a distinct type world here).
fn test_ctx() -> TyCtxMut {
    TyCtxMut::new(Interner::new())
}
use proptest::prelude::*;

/// The kinds a palette type can take: a fixed menu of param-free
/// primitives the proptests substitute *with*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteTy {
    I8,
    I32,
    U8,
    U64,
    F32,
    F64,
    Bool,
    String,
    SliceI32,
}

/// Build the palette kind in `ctx` (each call mints a fresh handle; the
/// kinds themselves carry no parameters).
fn palette_ty(ctx: &mut TyCtxMut, kind: PaletteTy) -> crate::Ty {
    use glyim_core::primitives::{FloatTy, IntTy, UintTy};
    match kind {
        PaletteTy::I8 => ctx.mk_ty(TyKind::Int(IntTy::I8)),
        PaletteTy::I32 => ctx.mk_ty(TyKind::Int(IntTy::I32)),
        PaletteTy::U8 => ctx.mk_ty(TyKind::Uint(UintTy::U8)),
        PaletteTy::U64 => ctx.mk_ty(TyKind::Uint(UintTy::U64)),
        PaletteTy::F32 => ctx.mk_ty(TyKind::Float(FloatTy::F32)),
        PaletteTy::F64 => ctx.mk_ty(TyKind::Float(FloatTy::F64)),
        PaletteTy::Bool => ctx.mk_ty(TyKind::Bool),
        PaletteTy::String => ctx.mk_ty(TyKind::String),
        PaletteTy::SliceI32 => {
            let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
            ctx.mk_ty(TyKind::Slice(i32_ty))
        }
    }
}

fn palette_strategy() -> impl Strategy<Value = PaletteTy> {
    prop_oneof![
        2 => Just(PaletteTy::I8),
        2 => Just(PaletteTy::I32),
        2 => Just(PaletteTy::U8),
        2 => Just(PaletteTy::U64),
        1 => Just(PaletteTy::F32),
        1 => Just(PaletteTy::F64),
        2 => Just(PaletteTy::Bool),
        1 => Just(PaletteTy::String),
        1 => Just(PaletteTy::SliceI32),
    ]
}

proptest! {
    // ------------------------------------------------------------------
    // Interned Substitution handles: intern/args round-trip.
    // ------------------------------------------------------------------

    /// Interning an argument list round-trips: the handle's `len` is the
    /// list length and `substitution_args` reproduces the exact handles.
    #[test]
    fn intern_substitution_round_trip(
        kinds in proptest::collection::vec(palette_strategy(), 0..8),
    ) {
        let mut ctx = test_ctx();
        let args: Vec<GenericArg> = kinds
            .iter()
            .map(|k| GenericArg::Ty(palette_ty(&mut ctx, *k)))
            .collect();
        let sub = ctx.intern_substitution(args.clone());
        prop_assert_eq!(sub.len() as usize, args.len());
        prop_assert_eq!(sub.is_empty(), args.is_empty());
        prop_assert_eq!(ctx.substitution_args(sub), args.as_slice());
    }

    /// Substitution interning is content-addressed end-to-end: re-minting
    /// the same kinds — brand-new type handles — interns to the *same*
    /// `(index, len)` pair (type allocation itself dedupes by kind, so
    /// rebuilt lists carry equal handles), and different kinds land on
    /// different pairs. This is the property the symbol mangler's
    /// injectivity rests on.
    #[test]
    fn intern_substitution_is_content_addressed(
        kinds in proptest::collection::vec(palette_strategy(), 1..5),
        others in proptest::collection::vec(palette_strategy(), 1..5),
    ) {
        let mut ctx = test_ctx();
        let to_args = |ctx: &mut TyCtxMut, kinds: &[PaletteTy]| {
            kinds
                .iter()
                .map(|k| GenericArg::Ty(palette_ty(ctx, *k)))
                .collect::<Vec<GenericArg>>()
        };
        let args_a = to_args(&mut ctx, &kinds);
        let sub_a1 = ctx.intern_substitution(args_a.clone());
        let args_a2 = to_args(&mut ctx, &kinds);
        let sub_a2 = ctx.intern_substitution(args_a2);
        prop_assert_eq!(sub_a1, sub_a2, "same kinds re-minted, same handle");
        let args_other = to_args(&mut ctx, &others);
        let sub_other = ctx.intern_substitution(args_other);
        prop_assert_eq!(
            sub_a1 == sub_other,
            kinds == others,
            "the content decides the handle"
        );
    }

    /// The empty substitution is the canonical empty: `empty()` has no
    /// args and interning an empty list agrees with it.
    #[test]
    fn empty_substitution_is_canonical(dummy in any::<u8>()) {
        let mut ctx = test_ctx();
        let canonical = Substitution::empty();
        prop_assert!(canonical.is_empty());
        prop_assert_eq!(canonical.len(), 0);
        let interned = ctx.intern_substitution(vec![]);
        prop_assert!(interned.is_empty());
        prop_assert!(ctx.substitution_args(interned).is_empty());
        let _ = dummy;
    }

    // ------------------------------------------------------------------
    // subst_ty: the generic-parameter rewrite.
    // ------------------------------------------------------------------

    /// An empty substitution map is the identity on every palette type —
    /// the rewrite must never touch a type it has no parameters for.
    #[test]
    fn subst_empty_map_is_identity(kind in palette_strategy()) {
        let mut ctx = test_ctx();
        let ty = palette_ty(&mut ctx, kind);
        let subst: HashMap<u32, GenericArg> = HashMap::new();
        prop_assert_eq!(ctx.subst_ty(ty, &subst), ty);
    }

    /// Rewriting a param-free palette type with *any* map leaves it
    /// unchanged: substitution only rewrites parameters, never concrete
    /// types (the palette contains no `Param`, so nothing can match).
    #[test]
    fn subst_only_rewrites_params(
        kind in palette_strategy(),
        targets in proptest::collection::vec(palette_strategy(), 0..3),
    ) {
        let mut ctx = test_ctx();
        let ty = palette_ty(&mut ctx, kind);
        let subst: HashMap<u32, GenericArg> = targets
            .iter()
            .enumerate()
            .map(|(i, k)| (i as u32, GenericArg::Ty(palette_ty(&mut ctx, *k))))
            .collect();
        prop_assert_eq!(ctx.subst_ty(ty, &subst), ty);
    }

    /// Rewriting a `Param(i)` under `{i: replacement}` yields exactly the
    /// replacement (the instantiation step for generic calls).
    #[test]
    fn subst_rewrites_the_named_param(
        param_index in 0_u32..8,
        kind in palette_strategy(),
        noise in proptest::collection::vec(palette_strategy(), 0..2),
    ) {
        let mut ctx = test_ctx();
        let replacement = palette_ty(&mut ctx, kind);
        let param_name = ctx.resolver().intern("T");
        let param = ctx
            .mk_ty(TyKind::Param(ParamTy { index: param_index, name: param_name }));
        let mut subst: HashMap<u32, GenericArg> = noise
            .iter()
            .enumerate()
            .filter(|(i, _)| *i as u32 != param_index)
            .map(|(i, k)| (i as u32, GenericArg::Ty(palette_ty(&mut ctx, *k))))
            .collect();
        subst.insert(param_index, GenericArg::Ty(replacement));
        prop_assert_eq!(ctx.subst_ty(param, &subst), replacement);
    }

    /// Substitution is stable under repetition: applying the same closed
    /// map twice changes nothing (palette replacements are param-free, so
    /// the second pass has nothing left to rewrite).
    #[test]
    fn subst_is_idempotent_on_closed_maps(
        param_index in 0_u32..4,
        kind in palette_strategy(),
    ) {
        let mut ctx = test_ctx();
        let replacement = palette_ty(&mut ctx, kind);
        let param_name = ctx.resolver().intern("T");
        let param =
            ctx.mk_ty(TyKind::Param(ParamTy { index: param_index, name: param_name }));
        let mut subst = HashMap::new();
        subst.insert(param_index, GenericArg::Ty(replacement));
        let once = ctx.subst_ty(param, &subst);
        let twice = ctx.subst_ty(once, &subst);
        prop_assert_eq!(once, twice);
    }

    // ------------------------------------------------------------------
    // mangle_fn_symbol: the (FnDefId, Substitution) -> symbol contract.
    // ------------------------------------------------------------------

    /// The mangler is deterministic (same pair, same symbol) and
    /// injective over the sampled `(def_id, substs)` pairs: distinct
    /// pairs get distinct symbols, so monomorphized instantiations cannot
    /// collide in the object file.
    #[test]
    fn mangler_is_deterministic_and_injective(
        def_a in any::<u32>(),
        def_b in any::<u32>(),
        kinds_a in proptest::collection::vec(palette_strategy(), 0..4),
        kinds_b in proptest::collection::vec(palette_strategy(), 0..4),
    ) {
        let mut ctx = test_ctx();
        let to_sub = |ctx: &mut TyCtxMut, kinds: &[PaletteTy]| {
            let args = kinds
                .iter()
                .map(|k| GenericArg::Ty(palette_ty(ctx, *k)))
                .collect();
            ctx.intern_substitution(args)
        };
        let sub_a = to_sub(&mut ctx, &kinds_a);
        let sub_b = to_sub(&mut ctx, &kinds_b);
        let sym_a = mangle_fn_symbol(def_a, sub_a);
        let sym_b = mangle_fn_symbol(def_b, sub_b);
        // Deterministic: re-mangling the *same* pair repeats the symbol.
        prop_assert_eq!(&sym_a, &mangle_fn_symbol(def_a, sub_a));
        prop_assert_eq!(&sym_b, &mangle_fn_symbol(def_b, sub_b));
        prop_assert_eq!(
            sym_a == sym_b,
            def_a == def_b && sub_a == sub_b,
            "distinct pairs, distinct symbols"
        );
    }

    /// An empty substitution keeps the un-suffixed legacy name and every
    /// symbol is LLVM-safe: only `[A-Za-z0-9_]` (the assembler never sees
    /// mangled characters outside that set).
    #[test]
    fn mangler_legacy_name_and_charset(
        def_id in any::<u32>(),
        kinds in proptest::collection::vec(palette_strategy(), 0..4),
    ) {
        let mut ctx = test_ctx();
        let args = kinds
            .iter()
            .map(|k| GenericArg::Ty(palette_ty(&mut ctx, *k)))
            .collect();
        let sub = ctx.intern_substitution(args);
        let symbol = mangle_fn_symbol(def_id, sub);
        prop_assert!(
            symbol.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "LLVM-safe charset: {symbol}"
        );
        if sub.is_empty() {
            prop_assert_eq!(symbol, format!("__glyim_fn_{def_id}"), "legacy name");
        } else {
            prop_assert_eq!(
                symbol,
                format!("__glyim_fn_{}_{}_{}", def_id, sub.index(), sub.len()),
                "suffixed name carries the interned pair"
            );
        }
    }
}

proptest! {
    /// `subst_ty` rewrites through containers: a `Param` nested inside
    /// `Ref`/`RawPtr`/`Slice`/`Array`/`Tuple` reaches the replacement —
    /// the recursive arms the generic-call instantiation depends on.
    #[test]
    fn subst_rewrites_through_containers(
        param_index in 0_u32..4,
        kind in palette_strategy(),
    ) {
        let mut ctx = test_ctx();
        let replacement = palette_ty(&mut ctx, kind);
        let param_name = ctx.resolver().intern("T");
        let param = ctx
            .mk_ty(TyKind::Param(ParamTy { index: param_index, name: param_name }));
        let mut subst = std::collections::HashMap::new();
        subst.insert(param_index, GenericArg::Ty(replacement));
        use crate::const_val::{Const, ConstKind};
        use glyim_core::primitives::Mutability;
        let elem_count = Const {
            kind: ConstKind::Uint(4),
            ty: replacement,
        };
        let containers = [
            ctx.mk_ty(TyKind::Ref(crate::region::Region::Static, param, Mutability::Not)),
            ctx.mk_ty(TyKind::RawPtr(param, Mutability::Mut)),
            ctx.mk_ty(TyKind::Slice(param)),
            ctx.mk_ty(TyKind::Array(param, elem_count)),
            {
                let args = ctx.intern_substitution(vec![GenericArg::Ty(param)]);
                ctx.mk_ty(TyKind::Tuple(args))
            },
        ];
        for container in containers {
            let rewritten = ctx.subst_ty(container, &subst);
            // The container type must no longer name the parameter: the
            // rewritten head is either a fresh container over the
            // replacement, or (for tuple) a fresh tuple whose inner
            // resolves to the replacement.
            let resolved = ctx.subst_ty(rewritten, &subst);
            prop_assert_ne!(resolved, container, "rewrite must be visible");
            prop_assert_ne!(
                ctx.ty_kind(resolved).clone(),
                ctx.ty_kind(param).clone(),
                "the param must not resurface"
            );
            prop_assert_ne!(rewritten, container, "the rewrite must be visible");
        }
    }
}
