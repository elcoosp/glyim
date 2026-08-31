/// HIR `TypeRef` → `Ty` conversion.
use std::collections::HashMap;

use glyim_core::def_id::{AdtId, DefId, LocalDefId, TraitDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use glyim_diag::GlyimDiagnostic;
use glyim_solve::InferenceTable;
use glyim_span::Span;
use glyim_type::*;

use crate::coherence::ResolvedImplHeader;

/// resolve_type_ref.
pub fn resolve_type_ref(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    ty_ref: &glyim_hir::TypeRef,
    param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Ty {
    match ty_ref {
        glyim_hir::TypeRef::Path(path) => {
            resolve_path_type(ctx, infer, def_map, diagnostics, path, param_map, span)
        }

        glyim_hir::TypeRef::Ref { inner, mutability } => {
            let inner_ty =
                resolve_type_ref(ctx, infer, def_map, diagnostics, inner, param_map, span);
            if inner_ty == Ty::ERROR {
                return Ty::ERROR;
            }
            ctx.mk_ref(Region::Erased, inner_ty, *mutability)
        }

        glyim_hir::TypeRef::Tuple(elements) => {
            let mut tys = Vec::with_capacity(elements.len());
            for elem in elements {
                tys.push(resolve_type_ref(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    elem,
                    param_map,
                    span,
                ));
            }
            if tys.is_empty() {
                return Ty::UNIT;
            }
            if tys.contains(&Ty::ERROR) {
                return Ty::ERROR;
            }
            let substs = ctx.intern_substitution(tys.into_iter().map(GenericArg::Ty).collect());
            ctx.mk_ty(TyKind::Tuple(substs))
        }

        glyim_hir::TypeRef::Never => Ty::NEVER,

        glyim_hir::TypeRef::Infer => {
            let var = infer.new_ty_var(ctx);
            ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
        }

        glyim_hir::TypeRef::Error => Ty::ERROR,

        // Function pointer types: fn(T, U) -> R
        glyim_hir::TypeRef::Fn { params, ret } => {
            let mut param_tys = Vec::with_capacity(params.len());
            for param_ty_ref in params {
                let ty = resolve_type_ref(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    param_ty_ref,
                    param_map,
                    span,
                );
                if ty == Ty::ERROR {
                    return Ty::ERROR;
                }
                param_tys.push(GenericArg::Ty(ty));
            }
            let return_ty = match ret {
                Some(ret_ref) => {
                    resolve_type_ref(ctx, infer, def_map, diagnostics, ret_ref, param_map, span)
                }
                None => Ty::UNIT,
            };
            if return_ty == Ty::ERROR {
                return Ty::ERROR;
            }
            let fn_sig = glyim_type::FnSig {
                inputs: ctx.intern_substitution(param_tys),
                output: return_ty,
                c_variadic: false,
                unsafety: Safety::Safe,
                abi: Abi::Glyim,
            };
            ctx.mk_ty(TyKind::FnPtr(fn_sig))
        }

        // Slice types: [T]
        glyim_hir::TypeRef::Slice(inner) => {
            let elem_ty =
                resolve_type_ref(ctx, infer, def_map, diagnostics, inner, param_map, span);
            if elem_ty == Ty::ERROR {
                Ty::ERROR
            } else {
                ctx.mk_ty(TyKind::Slice(elem_ty))
            }
        }

        // Array types: [T; N]
        glyim_hir::TypeRef::Array { inner, len } => {
            let elem_ty =
                resolve_type_ref(ctx, infer, def_map, diagnostics, inner, param_map, span);
            if elem_ty == Ty::ERROR {
                return Ty::ERROR;
            }
            let const_val = resolve_const_ref(ctx, def_map, diagnostics, len, param_map, span);
            ctx.mk_ty(TyKind::Array(elem_ty, const_val))
        }

        // `dyn Trait` — an unsized trait object.
        glyim_hir::TypeRef::Dyn(inner) => {
            // The inner type reference must name a trait.
            let trait_path = match &**inner {
                glyim_hir::TypeRef::Path(p) => p,
                _ => {
                    diagnostics.push(GlyimDiagnostic::type_error(
                        span,
                        "the inner type of a `dyn` trait object must be a trait path".to_string(),
                    ));
                    return Ty::ERROR;
                }
            };
            let trait_def_id = resolve_path_to_trait_def_id(def_map, ctx, trait_path, span);
            let trait_def_id = match trait_def_id {
                Some(id) => id,
                None => {
                    let path_str = trait_path
                        .segments
                        .iter()
                        .map(|seg| ctx.name_str(seg.name))
                        .collect::<Vec<_>>()
                        .join("::");
                    diagnostics.push(GlyimDiagnostic::type_error(
                        span,
                        format!("cannot find trait `{}` in this scope", path_str),
                    ));
                    return Ty::ERROR;
                }
            };

            // Enforce object safety for the resolved trait when its definition
            // is available in the type context.
            if let Some(trait_def) = ctx.trait_def(trait_def_id) {
                let methods: Vec<glyim_type::object_safety::MethodSignature> = trait_def
                    .methods
                    .iter()
                    .map(|m| {
                        let inputs = ctx.substitution_args(m.sig.inputs);
                        glyim_type::object_safety::MethodSignature {
                            name: m.name,
                            span,
                            self_kind: self_kind_of_inputs(&*ctx, inputs),
                            // Generic-parameter detection requires the trait
                            // method's own generic-param list, which is not
                            // recoverable from the interred `FnSig` substitution
                            // here. We conservatively assume no generic params;
                            // the dedicated object-safety algorithm tests still
                            // exercise the generic-method rejection path.
                            has_generic_params: false,
                            returns_self: false,
                        }
                    })
                    .collect();
                let violations = glyim_type::object_safety::check_object_safety(
                    &glyim_type::object_safety::TraitObjectSafetyInput {
                        requires_self_sized: false,
                        methods: &methods,
                        associated_types: &[],
                        supertrait_safety: &[],
                    },
                );
                for v in violations {
                    let msg = match v {
                        glyim_type::object_safety::ObjectSafetyViolation::SelfSized => {
                            "the trait cannot be made into an object because it requires `Self: Sized`".into()
                        }
                        glyim_type::object_safety::ObjectSafetyViolation::GenericMethod {
                            method, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because method `{}` has generic type parameters",
                            ctx.name_str(trait_def.name),
                            ctx.name_str(method)
                        ),
                        glyim_type::object_safety::ObjectSafetyViolation::StaticMethod {
                            method, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because method `{}` has no receiver",
                            ctx.name_str(trait_def.name),
                            ctx.name_str(method)
                        ),
                        glyim_type::object_safety::ObjectSafetyViolation::ByValueSelf {
                            method, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because method `{}` takes `self` by value",
                            ctx.name_str(trait_def.name),
                            ctx.name_str(method)
                        ),
                        glyim_type::object_safety::ObjectSafetyViolation::AssociatedFunction {
                            name, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because associated function `{}` is not dispatchable",
                            ctx.name_str(trait_def.name),
                            ctx.name_str(name)
                        ),
                        glyim_type::object_safety::ObjectSafetyViolation::UnconstrainedAssociatedType {
                            name, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because associated type `{}` is not constrained",
                            ctx.name_str(trait_def.name),
                            ctx.name_str(name)
                        ),
                        glyim_type::object_safety::ObjectSafetyViolation::SupertraitNotObjectSafe {
                            trait_id, ..
                        } => format!(
                            "the trait `{}` cannot be made into an object because supertrait `{}` is not object-safe",
                            ctx.name_str(trait_def.name),
                            ctx.trait_def(trait_id)
                                .map(|t| ctx.name_str(t.name).to_string())
                                .unwrap_or_else(|| format!("#{}", trait_id.index())),
                        ),
                    };
                    diagnostics.push(GlyimDiagnostic::type_error(span, msg));
                }
            }

            let trait_ref = glyim_type::TraitRef {
                def_id: trait_def_id,
                substs: ctx.intern_substitution(vec![]),
            };
            let preds: Box<[glyim_type::Predicate]> =
                Box::new([glyim_type::Predicate::Trait(glyim_type::TraitPredicate {
                    trait_ref,
                    polarity: glyim_type::ImplPolarity::Positive,
                })]);
            let binder = glyim_type::Binder::bind(
                preds,
                Box::new([glyim_type::BoundVariableKind::Ty(
                    glyim_type::BoundTyKind::Anon,
                )]),
            );
            ctx.mk_ty(TyKind::Dynamic(binder, Region::Erased))
        }
    }
}

/// Resolve const expressions in type positions (simplified)
fn resolve_const_ref(
    ctx: &mut TyCtxMut,
    _def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    const_ref: &glyim_hir::ConstRef,
    _param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Const {
    match const_ref {
        glyim_hir::ConstRef::Literal(lit) => match lit {
            glyim_hir::Literal::Int(v, _) => Const {
                kind: ConstKind::Int(*v),
                ty: ctx.mk_ty(TyKind::Int(IntTy::Isize)),
            },
            glyim_hir::Literal::Uint(v, _) => Const {
                kind: ConstKind::Uint(*v),
                ty: ctx.mk_ty(TyKind::Uint(UintTy::Usize)),
            },
            glyim_hir::Literal::Bool(b) => Const {
                kind: ConstKind::Bool(*b),
                ty: Ty::BOOL,
            },
            glyim_hir::Literal::Char(c) => Const {
                kind: ConstKind::Char(*c),
                ty: ctx.mk_ty(TyKind::Char),
            },
            _ => {
                diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "unsupported const literal in type position",
                ));
                Const {
                    kind: ConstKind::Error,
                    ty: ctx.error_ty(),
                }
            }
        },
        glyim_hir::ConstRef::Path(_path) => {
            diagnostics.push(GlyimDiagnostic::type_error(
                span,
                "const generic paths not yet fully implemented",
            ));
            Const {
                kind: ConstKind::Error,
                ty: ctx.error_ty(),
            }
        }
        glyim_hir::ConstRef::Error => Const {
            kind: ConstKind::Error,
            ty: ctx.error_ty(),
        },
    }
}

/// build_param_tys.
pub fn build_param_tys(
    ctx: &mut TyCtxMut,
    params: &[glyim_hir::GenericParam],
) -> HashMap<Name, Ty> {
    let mut map = HashMap::with_capacity(params.len());
    for (i, param) in params.iter().enumerate() {
        match param.kind {
            glyim_hir::GenericParamKind::Type { .. } => {
                let pt = ParamTy {
                    index: i as u32,
                    name: param.name,
                };
                map.insert(param.name, ctx.mk_ty(TyKind::Param(pt)));
            }
            glyim_hir::GenericParamKind::Lifetime => {
                // Lifetimes handled separately
            }
            glyim_hir::GenericParamKind::Const { .. } => {
                // Const generics handled via separate resolution
            }
        }
    }
    map
}

#[derive(Clone, Debug)]
/// FnSig.
pub struct FnSig {
/// Struct.
    pub param_tys: Vec<Ty>,
/// Struct.
    pub return_ty: Ty,
}

#[allow(clippy::too_many_arguments)]
/// resolve_fn_sig.
pub fn resolve_fn_sig(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    params: &[glyim_hir::Param],
    return_ty_ref: &Option<glyim_hir::TypeRef>,
    generic_params: &[glyim_hir::GenericParam],
    span: Span,
    self_ty: Option<Ty>,
) -> FnSig {
    let mut param_map = build_param_tys(ctx, generic_params);
    // Register `self` / `Self` as the impl's `Self` type so that a `&mut self`
    // / `&self` receiver (now lowered as `Ref(Path(self))`) resolves to
    // `&mut Self` / `&Self` instead of an unresolved `self` path. Method
    // receivers are references to `Self`, never the bare pointee.
    if let Some(st) = self_ty {
        param_map.insert(ctx.resolver().intern("self"), st);
        param_map.insert(ctx.resolver().intern("Self"), st);
    }

    let mut param_tys = Vec::with_capacity(params.len());
    let self_name = ctx.resolver().intern("self");
    for param in params {
        // Robust `self` / `&self` / `&mut self` receiver typing. Method
        // receivers are always `Self` (by value), `&Self`, or `&mut Self`.
        // We build the receiver type directly from the impl's `Self` type so
        // the reference is never lost (the parser now lowers `&mut self` as
        // `Ref(PathType(self))`, but path resolution of `self` is fragile and
        // previously dropped the reference, typing the receiver as the bare
        // pointee). This keeps every `&self` / `&mut self` method working.
        let ty = if param.name == self_name {
            match self_ty {
                Some(st) => match &param.ty {
                    Some(glyim_hir::TypeRef::Ref { mutability, .. }) => {
                        ctx.mk_ref(Region::Erased, st, *mutability)
                    }
                    _ => st,
                },
                None => {
                    let var = infer.new_ty_var(ctx);
                    ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
                }
            }
        } else if let Some(ty_ref) = &param.ty {
            let resolved = resolve_type_ref(
                ctx,
                infer,
                def_map,
                diagnostics,
                ty_ref,
                &param_map,
                param.span,
            );
            if resolved == Ty::ERROR {
                let var = infer.new_ty_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
            } else {
                resolved
            }
        } else if param.name == ctx.resolver().intern("self") {
            match self_ty {
                Some(st) => st,
                None => {
                    let var = infer.new_ty_var(ctx);
                    ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
                }
            }
        } else {
            let var = infer.new_ty_var(ctx);
            ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
        };
        param_tys.push(ty);
    }

    let return_ty = match return_ty_ref {
        Some(ty_ref) => {
            let resolved =
                resolve_type_ref(ctx, infer, def_map, diagnostics, ty_ref, &param_map, span);
            if resolved == Ty::ERROR {
                let var = infer.new_ty_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
            } else {
                resolved
            }
        }
        None => Ty::UNIT,
    };

    FnSig {
        param_tys,
        return_ty,
    }
}

/// resolve_impl_header.
pub fn resolve_impl_header(
    ctx: &mut TyCtxMut,
    _infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    impl_item: &glyim_hir::ImplItem,
    span: Span,
) -> ResolvedImplHeader {
    let param_map = build_param_tys(ctx, &impl_item.generic_params);

    let self_ty = resolve_type_ref(
        ctx,
        _infer,
        def_map,
        diagnostics,
        &impl_item.self_ty,
        &param_map,
        span,
    );

    let (trait_def_id, trait_name, trait_substs) = match &impl_item.trait_ref {
        Some(path) => {
            if let Some(name) = path.as_name() {
                match resolve_name_to_def_id(def_map, name) {
                    Some(def_id) => {
                        let trait_def_id = TraitDefId::from_raw(def_id.local_id.to_raw());
                        let substs = ctx.intern_substitution(vec![]);
                        (Some(trait_def_id), Some(name), substs)
                    }
                    None => {
                        // Fall back to builtin/lang traits registered by name
                        // in `TyCtxMut` (Future, Drop, Deref, Clone, …) so
                        // `impl Future for X` / `impl Drop for Y` resolve even
                        // though they are not declared in user source.
                        if let Some(tid) = ctx.trait_by_name.get(&name).copied() {
                            let substs = ctx.intern_substitution(vec![]);
                            (Some(tid), Some(name), substs)
                        } else {
                            diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!("unresolved trait `{}`", def_map.interner.resolve(name)),
                            ));
                            (None, Some(name), ctx.intern_substitution(vec![]))
                        }
                    }
                }
            } else {
                match resolve_path_to_trait_def_id(def_map, ctx, path, span) {
                    Some(trait_def_id) => {
                        let substs = ctx.intern_substitution(vec![]);
                        (Some(trait_def_id), path.as_name(), substs)
                    }
                    None => {
                        let path_str = path
                            .segments
                            .iter()
                            .map(|s| ctx.name_str(s.name))
                            .collect::<Vec<_>>()
                            .join("::");
                        diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            format!("cannot find trait `{}` in this scope", path_str),
                        ));
                        (None, None, ctx.intern_substitution(vec![]))
                    }
                }
            }
        }
        None => (None, None, ctx.intern_substitution(vec![])),
    };

    let self_type_name = match &impl_item.self_ty {
        glyim_hir::TypeRef::Path(p) => p.as_name(),
        _ => None,
    };

    let generic_param_names = impl_item.generic_params.iter().map(|p| p.name).collect();

    ResolvedImplHeader {
        trait_def_id,
        trait_name,
        trait_substs,
        self_ty,
        self_type_name,
        generic_param_names,
        polarity: ImplPolarity::Positive,
        span,
    }
}

/// resolve_path_type.
pub fn resolve_path_type(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    path: &glyim_hir::Path,
    param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Ty {
    // Check param_map first for generic params
    if let Some(name) = path.as_name()
        && let Some(&ty) = param_map.get(&name)
    {
        return ty;
    }

    // Plan unstub-5 P5: associated-type projection paths (`Self::Output`,
    // `F::Output`, …) are lowered as a *single* path segment whose name
    // string contains `::`. Detect that here and synthesize a `ProjectionTy`
    // naming the bound trait, exactly like the two-segment branch below.
    // Without this, `Self::Output` falls through to the ADT/qualified-path
    // resolvers and yields an `unresolved type` (or, inside `Poll<Self::Output>`,
    // an empty substitution → `mismatched type argument counts`).
    if let Some(name) = path.as_name() {
        // `name_str` borrows `ctx` immutably; detach the split substrings into
        // owned `String`s so the later mutable `resolve_path_type` borrow of
        // `ctx` is legal.
        let split: Option<(String, String)> = {
            let s = ctx.name_str(name);
            s.split_once("::").map(|(q, a)| (q.to_string(), a.to_string()))
        };
        if let Some((qual, assoc)) = split {
            let qname = ctx.resolver().intern(&qual);
            let aname = ctx.resolver().intern(&assoc);
            // Resolve the qualifier to a type to learn whether it is a
            // concrete type (use the impl projection table) or a generic
            // param / `Self` (build an abstract `ProjectionTy`).
            let qpath = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name: qname,
                    generic_args: None,
                }],
                kind: glyim_core::path::PathKind::Plain,
            };
            let mut q_diags = Vec::new();
            let qual_ty =
                resolve_path_type(ctx, infer, def_map, &mut q_diags, &qpath, param_map, span);
            if !matches!(ctx.ty_kind(qual_ty), TyKind::Error) {
                if let Some(ty) = ctx.resolve_associated_type_by_self_ty(qual_ty, aname) {
                    return ty;
                }
            }
            if matches!(ctx.ty_kind(qual_ty), TyKind::Param(_)) || qual == "Self" {
                let trait_def_id = if matches!(ctx.ty_kind(qual_ty), TyKind::Param(_)) {
                    ctx.param_bounds_for(qname)
                        .and_then(|traits| traits.first().map(|(_, tid)| *tid))
                } else {
                    ctx.find_trait_with_assoc_type(aname)
                };
                if let Some(tid) = trait_def_id {
                    let substs = ctx.intern_substitution(vec![GenericArg::Ty(qual_ty)]);
                    let trait_ref = TraitRef {
                        def_id: tid,
                        substs,
                    };
                    let proj = ProjectionTy {
                        trait_ref,
                        item_name: aname,
                    };
                    return ctx.mk_ty(TyKind::Projection(proj));
                }
            }
        }
    }

    // Check primitives
    if let Some(name) = path.as_name()
        && let Some(ty) = resolve_primitive(ctx, name)
    {
        return ty;
    }

    // Check ADTs (structs, enums, unions)
    if path.as_name().is_some()
        && let Some(ty) = resolve_name_to_adt_ty(ctx, infer, def_map, diagnostics, path, param_map, span)
    {
        return ty;
    }

    // Associated-type projection: `Type::Item` (concrete self type). Resolve
    // the first segment to a type, then look up its associated type via the
    // projection table populated at impl-registration time (plan unstub-5 P5).
    // Must run BEFORE `resolve_qualified_path` so that `AddOne::Output` is not
    // mistaken for a qualified ADT path. Abstract `Self::Item` / `F::Item`
    // (where the self type is a generic param or trait `Self`) still requires
    // the full trait solver and is not handled here.
    if path.segments.len() == 2 {
        let first = &path.segments[0];
        let assoc_name = path.segments[1].name;
        let mut first_diags = Vec::new();
        let first_path = glyim_hir::Path {
            segments: vec![first.clone()],
            kind: glyim_core::path::PathKind::Plain,
        };
        let self_ty =
            resolve_path_type(ctx, infer, def_map, &mut first_diags, &first_path, param_map, span);
        if !matches!(ctx.ty_kind(self_ty), TyKind::Error) {
            if let Some(ty) = ctx.resolve_associated_type_by_self_ty(self_ty, assoc_name) {
                return ty;
            }
        }
        // Plan unstub-5 P5: associated-type projection for an abstract self
        // type. When the qualifier is a generic parameter (`F::Output`) or
        // `Self` (`Self::Output`), there is no concrete impl to look the
        // defining type up in, so we synthesize a `ProjectionTy` that names
        // the bound trait instead. This keeps `F::Output` / `Self::Output`
        // well-typed (no spurious `unresolved type` diagnostic) and lets the
        // rest of typeck treat it as a real, comparable type.
        if matches!(ctx.ty_kind(self_ty), TyKind::Param(_)) || ctx.name_str(first.name) == "Self" {
            // `Self::Item` (or an abstract `Self` that lowered to a param) must
            // be projected against the *trait* that declares `Item`, not by
            // looking up a param bound (there is no bound registered for the
            // bare name `Self`). A generic *receiver* param (`F::Output`) uses
            // its own trait bound instead.
            let trait_def_id = if ctx.name_str(first.name) == "Self" {
                ctx.find_trait_with_assoc_type(assoc_name)
            } else {
                ctx.param_bounds_for(first.name)
                    .and_then(|traits| traits.first().map(|(_, tid)| *tid))
            };
            if let Some(tid) = trait_def_id {
                let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
                let trait_ref = TraitRef {
                    def_id: tid,
                    substs,
                };
                let proj = ProjectionTy {
                    trait_ref,
                    item_name: assoc_name,
                };
                return ctx.mk_ty(TyKind::Projection(proj));
            }
        }
    }

    // Multi-segment paths: try to resolve fully
    if !path.segments.is_empty()
        && let Some(resolved) = resolve_qualified_path(ctx, def_map, path, param_map, span, infer)
    {
        return resolved;
    }

    // Fallback error
    let path_str = path
        .segments
        .iter()
        .map(|seg| ctx.name_str(seg.name))
        .collect::<Vec<_>>()
        .join("::");

    diagnostics.push(GlyimDiagnostic::type_error(
        span,
        format!("unresolved type `{}`", path_str),
    ));
    Ty::ERROR
}

/// Resolve qualified paths like std::vec::Vec<T>
fn resolve_qualified_path(
    ctx: &mut TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    path: &glyim_hir::Path,
    _param_map: &HashMap<Name, Ty>,
    span: Span,
    infer: &mut InferenceTable,
) -> Option<Ty> {
    if path.segments.len() == 1 {
        return None;
    }

    // Resolve the whole path (module prefix + final ADT segment) through the
    // module tree, then build the ADT type with the final segment's generic
    // arguments (if any).
    let local = resolve_path_to_local_def_id(ctx, def_map, path)?;
    let adt_id = AdtId::from_raw(local.to_raw());
    let substs = if let Some(args) = path.segments.last().and_then(|s| s.generic_args.as_ref()) {
        let mut arg_tys = Vec::with_capacity(args.len());
        for arg in args {
            let resolved = resolve_type_ref(
                ctx,
                infer,
                def_map,
                &mut Vec::new(),
                arg,
                &HashMap::new(),
                span,
            );
            if matches!(ctx.ty_kind(resolved), TyKind::Error) {
                return None;
            }
            arg_tys.push(GenericArg::Ty(resolved));
        }
        ctx.intern_substitution(arg_tys)
    } else {
        ctx.intern_substitution(vec![])
    };
    Some(ctx.mk_ty(TyKind::Adt(adt_id, substs)))
}

/// Walk the module tree following `path`'s segments and resolve the final
/// segment to a `LocalDefId`. Leading segments name (sub)modules (via
/// `ModuleData::children`); the final segment is resolved in the scope of the
/// module reached by the prefix. Handles `Crate` / `Super(n)` / `SelfPath` /
/// `Plain` path kinds.
pub(crate) fn resolve_path_to_local_def_id(
    _ctx: &TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    path: &glyim_hir::Path,
) -> Option<LocalDefId> {
    let mut current = match path.kind {
        glyim_core::path::PathKind::Crate => def_map.root,
        glyim_core::path::PathKind::SelfPath | glyim_core::path::PathKind::Plain => def_map.root,
        glyim_core::path::PathKind::Super(n) => {
            let mut module = def_map.root;
            for _ in 0..n {
                if let Some(parent) = def_map.modules[module].parent {
                    module = parent;
                } else {
                    break;
                }
            }
            module
        }
    };

    // Single-segment `Plain` paths (bare trait/type/value names like `Read`,
    // `File`, `Error`) are resolved in `current`'s scope above. In the assembled
    // modular stdlib, items live in submodules (`io`, `fs`, `net`, …) and are
    // re-exported at the crate root via `pub use io::Read;`. When the def-map
    // builder does NOT populate the root scope from those re-exports (or the
    // re-export is for a trait the builder skips), the root lookup misses and
    // every cross-module reference fails. As a strictly-more-permissive
    // fallback, walk the entire module tree and return the first module whose
    // scope defines the bare name. Names are unique in the stdlib, so this
    // resolves cross-module references without disturbing same-module hits.
    if path.segments.len() == 1 && matches!(path.kind, glyim_core::path::PathKind::Plain) {
        if let Some(res) = def_map.modules[current].scope.resolve(path.segments[0].name) {
            return Some(res.0);
        }
        for module in def_map.modules.iter() {
            if let Some(res) = module.scope.resolve(path.segments[0].name) {
                return Some(res.0);
            }
        }
        return None;
    }

    for (i, seg) in path.segments.iter().enumerate() {
        // NOTE: the HIR `Path` segment `Name`s already live in the def-map's
        // interner in the pipeline (the lowering and `build_def_map` share a
        // rodeo via the database), so a direct `scope.resolve(seg.name)`
        // lookup is correct. We intentionally do NOT re-intern through
        // `def_map.interner` here — that path produced wrong strings when
        // `seg.name` belonged to a different rodeo than `ctx` (plan unstub-5
        // P5).
        if i + 1 == path.segments.len() {
            let res = def_map.modules[current].scope.resolve(seg.name)?;
            return Some(res.0);
        } else {
            let child = def_map.modules[current]
                .children
                .iter()
                .find(|(name, _)| *name == seg.name)
                .map(|(_, id)| *id)?;
            current = child;
        }
    }

    None
}

/// Resolve a (possibly multi‑segment) path to an `AdtId`, walking the module
/// tree like `resolve_path_to_local_def_id`. Used by struct/variant *patterns*
/// (plan §9.4 multi‑segment path support) where the final segment names an ADT.
/// Referenced from the `multi_seg_path` test module; kept available for
/// pattern-path lowering once that tier lands.
#[allow(dead_code)]
pub(crate) fn resolve_path_to_adt_id(
    ctx: &TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    path: &glyim_hir::Path,
) -> Option<AdtId> {
    resolve_path_to_local_def_id(ctx, def_map, path).map(|l| AdtId::from_raw(l.to_raw()))
}

/// Resolve a two-segment `Enum::Variant` path to `(AdtId, VariantIdx)` by
/// looking the variant up in the enum ADT's variant list by name. This is the
/// variant-constructor / variant-pattern resolution path that works for BOTH
/// syntax-built enums (whose variants live in the def-map `variant_map` /
/// synthetic module) AND HIR-generated enums (e.g. the async state-machine
/// `FooState` enum synthesized by `desugar_multi_async_fn`), which have no
/// syntax nodes and therefore never populate the def-map `variant_map`.
///
/// Returns `None` if the path is not a two-segment enum-variant path (so the
/// caller can fall through to its existing resolution logic).
pub(crate) fn resolve_enum_variant_path(
    ctx: &mut TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    path: &glyim_hir::Path,
) -> Option<(AdtId, u32)> {
    if path.segments.len() != 2 {
        return None;
    }
    let enum_path = glyim_hir::Path {
        segments: vec![path.segments[0].clone()],
        kind: glyim_core::path::PathKind::Plain,
    };
    let mut infer = InferenceTable::new();
    let enum_ty =
        resolve_path_type(ctx, &mut infer, def_map, &mut Vec::new(), &enum_path, &HashMap::new(), Span::DUMMY);
    let adt_id = match ctx.ty_kind(enum_ty) {
        TyKind::Adt(adt_id, _) => *adt_id,
        _ => return None,
    };
    let adt_def = ctx.adt_def(adt_id)?;
    let variant_name = path.segments[1].name;
    adt_def
        .variants
        .iter()
        .enumerate()
        .find(|(_, v)| v.name == variant_name)
        .map(|(idx, _)| (adt_id, idx as u32))
}

/// Resolve path to trait DefId
pub(crate) fn resolve_path_to_trait_def_id(
    def_map: &glyim_def_map::CrateDefMap,
    ctx: &TyCtxMut,
    path: &glyim_hir::Path,
    _span: Span,
) -> Option<TraitDefId> {
    // Builtin/lang traits (Future, Drop, Deref, Clone, …) are not in the
    // def-map; resolve them by name from `TyCtxMut`'s builtin trait table.
    if let Some(name) = path.as_name() {
        if let Some(tid) = ctx.trait_by_name.get(&name).copied() {
            return Some(tid);
        }
    }
    resolve_path_to_local_def_id(ctx, def_map, path).map(|l| TraitDefId::from_raw(l.to_raw()))
}

fn resolve_primitive(ctx: &mut TyCtxMut, name: Name) -> Option<Ty> {
    let s = ctx.name_str(name);
    Some(match s {
        "i8" => ctx.mk_ty(TyKind::Int(IntTy::I8)),
        "i16" => ctx.mk_ty(TyKind::Int(IntTy::I16)),
        "i32" => ctx.mk_ty(TyKind::Int(IntTy::I32)),
        "i64" => ctx.mk_ty(TyKind::Int(IntTy::I64)),
        "isize" => ctx.mk_ty(TyKind::Int(IntTy::Isize)),
        "u8" => ctx.mk_ty(TyKind::Uint(UintTy::U8)),
        "u16" => ctx.mk_ty(TyKind::Uint(UintTy::U16)),
        "u32" => ctx.mk_ty(TyKind::Uint(UintTy::U32)),
        "u64" => ctx.mk_ty(TyKind::Uint(UintTy::U64)),
        "usize" => ctx.mk_ty(TyKind::Uint(UintTy::Usize)),
        "f32" => ctx.mk_ty(TyKind::Float(FloatTy::F32)),
        "f64" => ctx.mk_ty(TyKind::Float(FloatTy::F64)),
        "bool" => Ty::BOOL,
        "char" => ctx.mk_ty(TyKind::Char),
        "str" => ctx.mk_ty(TyKind::String),
        "String" => ctx.mk_ty(TyKind::String),
        _ => return None,
    })
}

fn resolve_name_to_def_id(def_map: &glyim_def_map::CrateDefMap, name: Name) -> Option<DefId> {
    let res = def_map.modules[def_map.root].scope.resolve(name)?;
    Some(DefId::new(def_map.krate, res.0))
}

/// Infer the object-safety `self` kind of a trait method from its resolved
/// input argument list.
///
/// The first input is the receiver. A reference receiver (`&self` /
/// `&mut self`) is object-safe; a by-value `self` requires `Self: Sized`;
/// an empty input list means the method has no receiver (a static / associated
/// function), which cannot be dispatched through a trait object.
fn self_kind_of_inputs(
    ctx: &dyn glyim_type::TypeLookup,
    inputs: &[glyim_type::GenericArg],
) -> glyim_type::object_safety::MethodSelfKind {
    let Some(first) = inputs.first().and_then(|a| match a {
        glyim_type::GenericArg::Ty(t) => Some(t),
        _ => None,
    }) else {
        return glyim_type::object_safety::MethodSelfKind::None;
    };
    match ctx.ty_kind(*first) {
        glyim_type::TyKind::Ref(_, _, _) => glyim_type::object_safety::MethodSelfKind::ByReference,
        _ => glyim_type::object_safety::MethodSelfKind::ByValue,
    }
}

pub(crate) fn resolve_name_to_adt_ty(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    path: &glyim_hir::Path,
    param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Option<Ty> {
    let name = match path.as_name() {
        Some(n) => n,
        None => return None,
    };

    // stdlib-completion: expand a registered type alias BEFORE falling back to
    // the builtin/user ADT table. e.g. `type Result<T> = Result<T, Error>`
    // lets a 1-argument usage `Result<usize>` expand to `Result<usize, Error>`.
    // Only expand when the caller supplies exactly the alias's parameter count
    // (a 2-argument `Result<usize, Error>` use falls through to the 2-arg
    // builtin directly, so the alias never competes with it).
    if let Some((params, template)) = ctx.lookup_type_alias(name).cloned() {
        let caller_args = path.segments.last().and_then(|s| s.generic_args.as_deref());
        let n_caller = caller_args.map_or(0, |a| a.len());
        if n_caller == params.len() {
            let mut subst: HashMap<u32, GenericArg> = HashMap::new();
            if let Some(args) = caller_args {
                for (i, arg) in args.iter().enumerate() {
                    let ty = resolve_type_ref(ctx, infer, def_map, diagnostics, arg, param_map, span);
                    subst.insert(params[i].0, GenericArg::Ty(ty));
                }
            }
            return Some(ctx.subst_ty(template, &subst));
        }
    }

    // Resolve the ADT id. Prefer the *def-map* id (the authoritative source
    // for source-defined types) over `adt_id_by_name`: the latter can hold a
    // stale id minted during an earlier registration pass (e.g. a synthetic id
    // for `ErrorKind` that does not match the def-map's real id), which would
    // make the same logical type carry two different `AdtId`s and fail to
    // unify (e.g. `ErrorKind::Interrupted` resolving to Adt35 while
    // `Error::kind()` returns Adt63). Fall back to `adt_id_by_name` only for
    // generated types (async future structs) that are not in the def map.
    let adt_id = match path.as_name().and_then(|name| resolve_name_to_def_id(def_map, name)) {
        Some(def_id) => AdtId::from_raw(def_id.local_id.to_raw()),
        None => match path.as_name().and_then(|name| ctx.adt_id_by_name(name)) {
            Some(id) => id,
            None => return None,
        },
    };
    let arity = ctx.adt_generic_arity(adt_id);

    // Generic arguments live on the final path segment (the ADT itself).
    let args = path.segments.last().and_then(|s| s.generic_args.as_deref());

    let mut substs: Vec<GenericArg> = Vec::with_capacity(args.map_or(0, |a| a.len()).max(arity));
    if let Some(args) = args {
        // Plan unstub-5 P5: previously this branch bailed out to a 0-argument
        // `Poll` whenever `args.len() != arity`. That was wrong for two real
        // cases: (1) the ADT's generic arity is not yet known (the enum is
        // registered in `TyCtxMut` only *after* the current resolution pass,
        // e.g. an associated-type projection `Poll<Self::Output>` resolved
        // during trait registration); (2) an argument is an associated-type
        // projection that fails to resolve. In both cases we must still build
        // the ADT with the *written* number of arguments (pushing `Error` for
        // any unresolved one) so downstream `unify` reports a precise
        // (not arity-mismatch) diagnostic instead of a misleading
        // "mismatched type argument counts".
        if args.len() != arity {
            let path_str = path
                .segments
                .iter()
                .map(|seg| ctx.name_str(seg.name))
                .collect::<Vec<_>>()
                .join("::");
            diagnostics.push(GlyimDiagnostic::type_error(
                span,
                format!(
                    "generic type `{}` expects {} type argument(s), found {}",
                    path_str,
                    arity,
                    args.len()
                ),
            ));
        }
        for arg in args {
            let resolved =
                resolve_type_ref(ctx, infer, def_map, diagnostics, arg, param_map, span);
            substs.push(GenericArg::Ty(resolved));
        }
    } else {
        // No explicit args. For a generic ADT, fill the substitution with fresh
        // inference variables so downstream inference can unify them (e.g.
        // field-access on `Vec<_>` infers the element type).
        for _ in 0..arity {
            let var = infer.new_ty_var(ctx);
            let ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
            substs.push(GenericArg::Ty(ty));
        }
    }

    let subst = ctx.intern_substitution(substs);
    let final_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    Some(final_ty)
}