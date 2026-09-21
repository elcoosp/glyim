//! Unification and type resolution logic for FnCtxt.

use glyim_core::def_id::{AdtId, ConstDefId, FnDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::IntTy;
use glyim_diag::GlyimDiagnostic;
use glyim_hir::*;
use glyim_span::Span;
use glyim_solve::InferenceTable;
use glyim_type::{FieldIdx, FnSig, GenericArg, InferVar, Ty, TyCtxMut, TyKind};

use crate::check_body::FnCtxt;
use crate::thir;

impl<'a> FnCtxt<'a> {
    pub fn expr_span(&self, expr_id: ExprId) -> Span {
        if (expr_id.to_raw() as usize) < self.body.expr_spans.len() {
            self.body.expr_spans[expr_id]
        } else {
            Span::DUMMY
        }
    }

    pub fn fresh_infer_ty(&mut self) -> Ty {
        let var = self.infer.new_ty_var(self.ctx);
        self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
    }

    pub fn unify(&mut self, a: Ty, b: Ty, span: Span) -> bool {
        if a == Ty::ERROR || b == Ty::ERROR {
            return false;
        }
        match self.infer.unify(self.ctx, a, b, span) {
            Ok(_) => true,
            Err(diags) => {
                self.diagnostics.extend(diags);
                false
            }
        }
    }

    pub fn lookup_field_ty(&mut self, adt_id: AdtId, field: Name, span: Span) -> Ty {
        if let Some(field_idx) = self.ctx.field_index(adt_id, field)
            && let Some(def) = self.ctx.adt_def(adt_id)
            && let Some(field_def) = def.fields.get(FieldIdx::from_raw(field_idx as u32))
        {
            let field_ty = field_def.ty;
            return field_ty;
        }
        self.diagnostics.push(GlyimDiagnostic::type_error(
            span,
            format!("no field `{}` in ADT", self.ctx.name_str(field)),
        ));
        Ty::ERROR
    }

    pub fn check_path(&mut self, path: &Path, span: Span) -> (thir::Expr, Ty) {
        // 1. Local variable (already bound in this scope).
        if let Some(name) = path.as_name() {
            if let Some(var_info) = self.env.lookup_by_name(name) {
                self.capture_log.push((var_info.id, var_info.ty, false));
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::VarRef(var_info.id),
                    ty: var_info.ty,
                    span,
                };
                return (thir_expr, var_info.ty);
            }
            // 1b. Bare enum-variant value path (`Ok`, `Err`, `Some`, `None`).
            //     The Rust prelude references these by bare name, so search
            //     every registered enum's variant list (builtins live in
            //     `TyCtxMut`, not the def-map value namespace).
            //
            //     IMPORTANT (glyim-v2 stdlib handoff, Issue 1/3): restricted to
            //     the small set of names the prelude makes bare-accessible and
            //     that user code cannot unambiguously redeclare. This search is
            //     unscoped — it matches ANY registered ADT, builtin or
            //     user-declared, by variant name alone — so a name like
            //     `Ready`/`Pending` (`Poll`, which is NOT a prelude item and is
            //     routinely redeclared by user code) must NOT be matched here;
            //     doing so risks resolving to the wrong `Poll` depending on
            //     hashmap iteration order (this is what caused the
            //     `nested_async_single_await_compiles` regression via the
            //     mirror-image bug in `check_pat.rs`). Qualified `Poll::Ready`
            //     still resolves correctly below via scoped path resolution
            //     (step 0), which is unambiguous.
            let bare_name = self.ctx.name_str(name);
            // `drop(x)` — the stdlib calls the builtin drop function
            // directly (`sync.g`'s `Condvar::wait` does `drop(mutex_guard)`).
            // It has no source definition, so resolve it here to a synthetic
            // unit-returning fn value rather than an unresolved-name error.
            if bare_name == "drop" {
                let substs = self.ctx.intern_substitution(vec![]);
                let fn_id = FnDefId::from_raw(u32::MAX - 4);
                let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_id, substs));
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::FnRef(fn_id),
                    ty: fn_ty,
                    span,
                };
                return (thir_expr, fn_ty);
            }
            let bare_safe = matches!(
                bare_name,
                "None" | "Some" | "Ok" | "Err" | "Less" | "Equal" | "Greater"
            );
            if bare_safe {
                if let Some((adt_id, variant_idx)) = self.ctx.variant_by_name(name) {
                    return self.variant_expr(adt_id, variant_idx, span);
                }
            }
        }

        // 2. Value-namespace resolution through the def map (functions, consts,
        //    enum variants). Single- and multi-segment paths both flow through
        //    `Resolver::resolve_path`, which walks the module tree and returns a
        //    `PerNs` with separate type/value namespaces (plan: value paths).
        let core_path = glyim_core::Path {
            segments: path
                .segments
                .iter()
                .map(|s| glyim_core::PathSegment {
                    name: s.name,
                    generic_args: None,
                })
                .collect(),
            kind: path.kind,
        };
        let resolved = {
            let resolver = glyim_def_map::Resolver::new(
                &self.def_map.modules,
                self.def_map.root,
                self.current_module,
            );
            resolver.resolve_path(&core_path)
        };

        if let Some((local, _vis)) = resolved.values {
            // Enum variant value path. `Color::Red` (unit) is a value of the
            // enum type; `Some` / `Color::Green` (data-carrying) is a
            // constructor callable as `Some(x)`. The def map registers each
            // variant in the value namespace with a reverse map
            // variant_local -> (enum_local, VariantIdx).
            if let Some((enum_local, variant_idx)) = self.def_map.variant_map.get(&local) {
                let adt_id = AdtId::from_raw(enum_local.to_raw());
                // Plan unstub-5 P5: for a *generic* enum `Poll<T>`, the variant
                // value/pattern type must carry one inference variable per
                // generic parameter (so `Poll::Ready(x)` infers `Poll<i32>`
                // against an expected `Poll<i32>`), NOT a 0-argument `Poll`.
                // Building `Poll<>` here produced a spurious "mismatched type
                // argument counts" when the expected type was `Poll<i32>`.
                let arity = self.ctx.adt_generic_arity(adt_id);
                let substs: Vec<GenericArg> = (0..arity)
                    .map(|_| {
                        let var = self.infer.new_ty_var(self.ctx);
                        GenericArg::Ty(self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(var))))
                    })
                    .collect();
                let substs = self.ctx.intern_substitution(substs);
                let enum_ty = self.ctx.mk_ty(TyKind::Adt(adt_id, substs));

                // Data-carrying variant (has fields) => a constructor value
                // of function type `fn(field_tys) -> Enum`. Reuse the existing
                // `FnDefId` call machinery by registering a fn-sig for the
                // variant's value `LocalDefId`.
                let has_fields = self
                    .ctx
                    .adt_def(adt_id)
                    .and_then(|def| def.variants.get(variant_idx.index()))
                    .map(|v| !v.fields.is_empty())
                    .unwrap_or(false);

                if has_fields {
                    let ctor_fn_def_id = FnDefId::from_raw(local.to_raw());
                    let field_tys: Vec<Ty> = self
                        .ctx
                        .adt_def(adt_id)
                        .and_then(|def| def.variants.get(variant_idx.index()))
                        .map(|v| v.fields.iter().map(|f| f.ty).collect())
                        .unwrap_or_default();
                    let inputs = self.ctx.intern_substitution(
                        field_tys
                            .iter()
                            .map(|t| GenericArg::Ty(*t))
                            .collect(),
                    );
                    self.ctx.register_fn_sig(
                        ctor_fn_def_id,
                        FnSig {
                            inputs,
                            output: enum_ty,
                            c_variadic: false,
                            unsafety: glyim_core::primitives::Safety::Safe,
                            abi: glyim_core::primitives::Abi::Glyim,
                        },
                    );
                    let fn_ty = self
                        .ctx
                        .mk_ty(TyKind::FnDef(ctor_fn_def_id, substs));
                    let thir_expr = thir::Expr {
                        kind: thir::ExprKind::VariantCtor {
                            adt_id,
                            variant_idx: *variant_idx,
                        },
                        ty: fn_ty,
                        span,
                    };
                    return (thir_expr, fn_ty);
                }

                // Unit variant => a value of the enum type.
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::VariantRef(adt_id, *variant_idx),
                    ty: enum_ty,
                    span,
                };
                return (thir_expr, enum_ty);
            }

            // User enum variant referenced as a 2-segment path (e.g.
            // `FileType::Regular`) whose variant local is not in `variant_map`
            // (only some enums register there). Resolve the first segment to an
            // ADT and match the variant by name, then build the value via the
            // shared `variant_expr` helper (handles unit vs data-carrying).
            if path.segments.len() == 2 {
                let enum_path = glyim_hir::Path {
                    segments: vec![glyim_hir::PathSegment {
                        name: path.segments[0].name,
                        generic_args: None,
                    }],
                    kind: glyim_core::path::PathKind::Plain,
                };
                if let Some(adt_ty) = crate::tyconv::resolve_name_to_adt_ty(
                    self.ctx,
                    self.infer,
                    self.def_map,
                    &mut Vec::new(),
                    &enum_path,
                    &std::collections::HashMap::new(),
                    span,
                ) {
                    if let glyim_type::TyKind::Adt(adt_id, _) = self.ctx.ty_kind(adt_ty) {
                        if let Some(variant_idx) = self
                            .ctx
                            .adt_def(*adt_id)
                            .and_then(|def| {
                                def.variants
                                    .iter()
                                    .position(|v| v.name == path.segments[1].name)
                            })
                            .map(|i| glyim_core::def_id::VariantIdx::from_raw(i as u32))
                        {
                            return self.variant_expr(*adt_id, variant_idx, span);
                        }
                    }
                }
            }

            let fn_def_id = FnDefId::from_raw(local.to_raw());
            if self.ctx.fn_sig(fn_def_id).is_some() {
                let substs = self.ctx.intern_substitution(vec![]);
                let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_def_id, substs));
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::FnRef(fn_def_id),
                    ty: fn_ty,
                    span,
                };
                return (thir_expr, fn_ty);
            }
            // A constant in the value namespace: emit a `ConstRef` carrying the
            // constant's value type. The def map is the source of truth for the
            // LocalDefId; convert it to a ConstDefId for the THIR node.
            let const_def_id = ConstDefId::from_raw(local.to_raw());
            if let Some(const_ty) = self.ctx.const_ty(const_def_id) {
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::ConstRef(const_def_id),
                    ty: const_ty,
                    span,
                };
                return (thir_expr, const_ty);
            }
            // Resolved to a value that is neither a registered function nor a
            // registered constant (e.g. an enum variant, which needs a
            // dedicated `VariantRef` THIR node). Report a clear error.
            {
                let segs: Vec<String> = path.segments.iter()
                    .map(|s| self.ctx.name_str(s.name).to_string())
                    .collect();
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    format!("enum-variant value paths are not yet supported: `{}` (local={:?})",
                        segs.join("::"), local),
                ));
            }
            return (thir::Expr::err(span), Ty::ERROR);
        }

        // 2c. Builtin inherent associated function `Adt::fn` (e.g. `Vec::new()`,
        //     `String::with_capacity`, `Result::unwrap`). These are *not* enum
        //     variants and *not* (always) trait methods; they are the builtin
        //     collection/scalar constructors accessed path-style. The first
        //     segment resolves to a builtin ADT (Vec/String/Result/Option); the
        //     second is looked up in the builtin-method table (which also holds
        //     associated functions registered without a receiver).
        if path.segments.len() == 2 {
            let adt_path = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name: path.segments[0].name,
                    generic_args: None,
                }],
                kind: glyim_core::path::PathKind::Plain,
            };
            // Try strict ADT resolution first; fall back to the broader
            // `resolve_path_type` only when it fails. The fallback lets a
            // primitive first segment like `str::from_utf8` resolve to
            // `TyKind::String` (the closure below then maps it to the
            // synthetic `str` table id 1061). Doing the fallback *only on
            // `None`* keeps all previously-working cases — e.g. `extern "C"`
            // blocks and bare path resolution — on the strict path.
            let adt_ty_resolved = match crate::tyconv::resolve_name_to_adt_ty(
                self.ctx,
                self.infer,
                self.def_map,
                &mut Vec::new(),
                &adt_path,
                &std::collections::HashMap::new(),
                span,
            ) {
                Some(t) => t,
                None => crate::tyconv::resolve_path_type(
                    self.ctx,
                    self.infer,
                    self.def_map,
                    &mut Vec::new(),
                    &adt_path,
                    &std::collections::HashMap::new(),
                    span,
                ),
            };
            let adt_lookup: Option<(glyim_core::def_id::AdtId, glyim_type::Ty)> =
                match self.ctx.ty_kind(adt_ty_resolved) {
                    glyim_type::TyKind::Adt(id, _) => Some((*id, adt_ty_resolved)),
                    // `str::from_utf8(..)` — the first segment `str` resolves
                    // to `TyKind::String` (via `resolve_primitive`), not an
                    // Adt, but the builtin table stores the primitive `str`
                    // methods under the synthetic id 1061. Route it there so
                    // path-form associated fns on `str` resolve.
                    glyim_type::TyKind::String => {
                        Some((glyim_core::def_id::AdtId::from_raw(1061), adt_ty_resolved))
                    }
                    _ => None,
                };
            if let Some((adt_id, adt_ty)) = adt_lookup {
                if let Some((fn_id, sig)) = self.ctx.lookup_builtin_method(adt_id, path.segments[1].name) {
                    // Instantiate the output type against the *callee* ADT's own
                    // generic substitution (so `Vec::new` yields `Vec<T>` with
                    // `T` left as a fresh inference variable, matching how the
                    // receiver-arg substitution works for method calls).
                    let mut subst: std::collections::HashMap<u32, GenericArg> = std::collections::HashMap::new();
                    if let glyim_type::TyKind::Adt(_, s) = self.ctx.ty_kind(adt_ty) {
                        for (i, a) in self.ctx.substitution_args(*s).iter().enumerate() {
                            subst.insert(i as u32, a.clone());
                        }
                    }
                    let output = self.ctx.subst_ty(sig.output, &subst);
                    let substs = self.ctx.intern_substitution(vec![]);
                    let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_id, substs));
                    let thir_expr = thir::Expr {
                        kind: thir::ExprKind::FnRef(fn_id),
                        ty: fn_ty,
                        span,
                    };
                    // Stash the instantiated output on the fn-type so call
                    // argument/return checking sees the right type.
                    self.ctx.register_fn_sig(
                        fn_id,
                        FnSig {
                            inputs: sig.inputs,
                            output,
                            c_variadic: sig.c_variadic,
                            unsafety: sig.unsafety,
                            abi: sig.abi,
                        },
                    );
                    return (thir_expr, fn_ty);
                }

                // 2c.1 User-declared inherent associated function
                //      `Type::method` (`S::new()`, `OO::with_capacity(..)`).
                //      Method-call syntax (`recv.method()`) resolves by
                //      scanning the HIR impl blocks (see
                //      `resolve_method_call`); path syntax must do the same,
                //      otherwise `OO::new()` falls through to
                //      "unresolved value path" — `<error>` — and any chained
                //      `.set(..)` on the result fails with "no method". Fixed
                //      for inherent impls only; trait-provided associated
                //      functions use `<Type as Trait>::method`, handled by the
                //      trait-path branch below.
                for hir_item in self.hir.items.iter() {
                    let glyim_hir::ItemKind::Impl(impl_item) = &hir_item.kind else {
                        continue;
                    };
                    if impl_item.trait_ref.is_some() {
                        continue; // inherent only
                    }
                    // Does this impl's Self resolve to the same ADT?
                    let empty: std::collections::HashMap<Name, Ty> =
                        std::collections::HashMap::new();
                    let self_ty = crate::tyconv::resolve_type_ref(
                        self.ctx,
                        self.infer,
                        self.def_map,
                        &mut Vec::new(),
                        &impl_item.self_ty,
                        &empty,
                        span,
                    );
                    let self_adt = match self.ctx.ty_kind(self_ty) {
                        glyim_type::TyKind::Adt(id, _) => *id,
                        _ => continue,
                    };
                    if self_adt != adt_id {
                        continue;
                    }
                    for m in &impl_item.methods {
                        if m.name != path.segments[1].name {
                            continue;
                        }
                        let Some(body_id) = m.body else { continue };
                        let Some(local) = self.body_owner_map.get(&body_id).copied() else {
                            continue;
                        };
                        let fn_def_id = FnDefId::from_raw(local.to_raw());
                        // Instantiate the impl's own generic params (e.g.
                        // `impl<T> OO<T>`) with fresh inference vars, then
                        // substitute them through the method's signature so
                        // `OO::<T>::new() -> OO<T>` becomes `-> OO<?fresh>`.
                        let mut subst: std::collections::HashMap<u32, GenericArg> =
                            std::collections::HashMap::new();
                        let mut impl_param_idx = 0u32;
                        for gp in &impl_item.generic_params {
                            let v = self.infer.new_ty_var(self.ctx);
                            let fresh = self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(v)));
                            subst.insert(impl_param_idx, GenericArg::Ty(fresh));
                            let _ = gp;
                            impl_param_idx += 1;
                        }
                        // Plus the receiver's own substitution (if any).
                        if let glyim_type::TyKind::Adt(_, s) = self.ctx.ty_kind(adt_ty) {
                            for (i, a) in
                                self.ctx.substitution_args(*s).iter().enumerate()
                            {
                                subst.entry(i as u32).or_insert_with(|| a.clone());
                            }
                        }
                        let empty_substs = self.ctx.intern_substitution(vec![]);
                        let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_def_id, empty_substs));
                        // Re-register the sig with the substituted param types
                        // so call-argument checking sees concrete expected
                        // types (mirrors the builtin branch above).
                        if let Some(sig) = self.ctx.fn_sig(fn_def_id).cloned() {
                            let raw_inputs: Vec<GenericArg> =
                                self.ctx.substitution_args(sig.inputs).to_vec();
                            let mut inputs: Vec<GenericArg> = Vec::with_capacity(raw_inputs.len());
                            for a in raw_inputs {
                                match a {
                                    GenericArg::Ty(t) => {
                                        inputs.push(GenericArg::Ty(self.ctx.subst_ty(t, &subst)));
                                    }
                                    other => inputs.push(other),
                                }
                            }
                            let inputs = self.ctx.intern_substitution(inputs);
                            let output = self.ctx.subst_ty(sig.output, &subst);
                            self.ctx.register_fn_sig(
                                fn_def_id,
                                FnSig {
                                    inputs,
                                    output,
                                    c_variadic: sig.c_variadic,
                                    unsafety: sig.unsafety,
                                    abi: sig.abi,
                                },
                            );
                        }
                        let thir_expr = thir::Expr {
                            kind: thir::ExprKind::FnRef(fn_def_id),
                            ty: fn_ty,
                            span,
                        };
                        return (thir_expr, fn_ty);
                    }
                }
            }
        }

        if path.segments.len() == 2 {
            let enum_path = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name: path.segments[0].name,
                    generic_args: None,
                }],
                kind: glyim_core::path::PathKind::Plain,
            };
            // Resolve the enum type by name (handles both user enums registered
            // in the def-map and builtin enums like `Result`/`Option`/`Poll`).
            let enum_ty_resolved = crate::tyconv::resolve_name_to_adt_ty(
                self.ctx,
                self.infer,
                self.def_map,
                &mut Vec::new(),
                &enum_path,
                &std::collections::HashMap::new(),
                span,
            );
            if let Some(adt_id) = enum_ty_resolved.and_then(|ty| {
                if let glyim_type::TyKind::Adt(id, _) = self.ctx.ty_kind(ty) {
                    Some(*id)
                } else {
                    None
                }
            }) {
                let variant_idx = self
                    .ctx
                    .adt_def(adt_id)
                    .and_then(|def| {
                        def.variants
                            .iter()
                            .position(|v| v.name == path.segments[1].name)
                    })
                    .map(|i| glyim_core::def_id::VariantIdx::from_raw(i as u32));
                if let Some(variant_idx) = variant_idx {
                    return self.variant_expr(adt_id, variant_idx, span);
                }
            }
        }

        // 2b. Trait-method path `Trait::method`. The first segment resolves to
        //     a trait definition; the second is a method name. The concrete
        //     impl is selected by the receiver type at the `Call` site
        //     (static dispatch) — see `check_expr`'s `Call` handling. The
        //     resolution happens there, so here we only need to avoid emitting
        //     a dangling node; return a benign `Err` (the call site either
        //     rewrites the callee to a concrete `FnRef` or reports the error).
        if path.segments.len() == 2 {
            let trait_path = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name: path.segments[0].name,
                    generic_args: None,
                }],
                kind: path.kind,
            };
            if crate::tyconv::resolve_path_to_trait_def_id(self.def_map, self.ctx, &trait_path, span)
                .is_some()
            {
                return (thir::Expr::err(span), Ty::ERROR);
            }
        }

        // 3. Type-namespace resolution. A path that resolves to a *type* may
        //    still be a value expression when it names a unit struct: in Rust
        //    `let d = Dog;` is a value of type `Dog` for `struct Dog;` (a
        //    zero-field ADT). Treat such a path as a field-less struct literal.
        //    (Structs with fields must use `Dog { .. }`, handled in
        //    `check_expr` via `Expr::Struct`.)
        if let Some((type_local, _vis)) = resolved.types {
            let adt_id = AdtId::from_raw(type_local.to_raw());
            let is_unit_struct = self
                .ctx
                .adt_def(adt_id)
                .map(|def| def.fields.is_empty())
                .unwrap_or(false);
            if is_unit_struct {
                // A generic unit struct (`struct P<T>;`, `PhantomData<T>`) is
                // still written *without* its arguments at value position —
                // `_m: PhantomData`. Emitting a 0-arg substitution for a
                // 1-param ADT triggered "mismatched type argument counts" in
                // `Mutex { _marker: PhantomData }`. Fill in one fresh
                // inference var per declared generic param (mirroring the
                // `else` branch of `resolve_name_to_adt_ty`).
                let arity = self.ctx.adt_generic_arity(adt_id);
                let mut substs: Vec<GenericArg> = Vec::with_capacity(arity);
                for _ in 0..arity {
                    let v = self.infer.new_ty_var(self.ctx);
                    substs.push(GenericArg::Ty(self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(v)))));
                }
                let substs = self.ctx.intern_substitution(substs);
                let adt_ty = self.ctx.mk_ty(TyKind::Adt(adt_id, substs));
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::Struct {
                        adt_id,
                        variant_idx: 0,
                        fields: Vec::new(),
                        spread: None,
                    },
                    ty: adt_ty,
                    span,
                };
                return (thir_expr, adt_ty);
            }
        }

        // Builtin unit-struct fallback: names like `PhantomData` are registered
        // in the *type context* (TyCtxMut) but NOT in the def-map's type
        // namespace (they never appear in the source syntax), so the def-map
        // resolver above misses them. When a bare name resolves through the type
        // context to a builtin unit struct, treat it as a unit-struct literal
        // (mirroring the user unit-struct handling just above).
        if let Some(name) = path.as_name() {
            let probe = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name,
                    generic_args: None,
                }],
                kind: glyim_core::path::PathKind::Plain,
            };
            let builtin_unit = crate::tyconv::resolve_name_to_adt_ty(
                self.ctx,
                self.infer,
                self.def_map,
                &mut Vec::new(),
                &probe,
                &std::collections::HashMap::new(),
                span,
            )
            .and_then(|adt_ty| {
                if let glyim_type::TyKind::Adt(adt_id, _) = self.ctx.ty_kind(adt_ty) {
                    let def = self.ctx.adt_def(*adt_id);
                    let is_unit = def
                        .map(|d| d.fields.is_empty())
                        .unwrap_or(false);
                    // `PhantomData` is written as a bare value (`_marker: PhantomData`)
                    // even though it is declared with a `marker: T` field; treat
                    // the builtin zero-sized marker type as a unit value.
                    if is_unit || *adt_id == glyim_core::def_id::AdtId::from_raw(1030) {
                        return Some(*adt_id);
                    }
                }
                None
            });
            if let Some(adt_id) = builtin_unit {
                // Same arity reasoning as the user unit-struct arm above:
                // `PhantomData<T>` at value position carries a fresh
                // inference var for each declared param, not an empty
                // substitution.
                let arity = self.ctx.adt_generic_arity(adt_id);
                let mut substs: Vec<GenericArg> = Vec::with_capacity(arity);
                for _ in 0..arity {
                    let v = self.infer.new_ty_var(self.ctx);
                    substs.push(GenericArg::Ty(self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(v)))));
                }
                let substs = self.ctx.intern_substitution(substs);
                let adt_ty = self.ctx.mk_ty(TyKind::Adt(adt_id, substs));
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::Struct {
                        adt_id,
                        variant_idx: 0,
                        fields: Vec::new(),
                        spread: None,
                    },
                    ty: adt_ty,
                    span,
                };
                return (thir_expr, adt_ty);
            }
        }

        // Type-namespace resolution (ADTs, traits) is not a value expression;
        //    fall through to the unresolved-name diagnostic.
        if let Some(name) = path.as_name() {
            self.diagnostics.push(GlyimDiagnostic::type_error(
                span,
                format!("unresolved name `{}`", self.ctx.name_str(name)),
            ));
        } else {
            self.diagnostics.push(GlyimDiagnostic::type_error(
                span,
                "unresolved value path".to_string(),
            ));
        }
        (thir::Expr::err(span), Ty::ERROR)
    }

    /// Build a THIR expression node (and its type) for an enum variant value:
    /// a `VariantCtor` of function type for data-carrying variants (`Ok(x)`),
    /// or a `VariantRef` of the enum type for unit variants (`None`). The enum
    /// is instantiated with one fresh inference variable per generic parameter
    /// so downstream unification can pin the concrete type.
    fn variant_expr(
        &mut self,
        adt_id: AdtId,
        variant_idx: glyim_core::def_id::VariantIdx,
        span: Span,
    ) -> (thir::Expr, Ty) {
        let arity = self.ctx.adt_generic_arity(adt_id);
        let substs: Vec<GenericArg> = (0..arity)
            .map(|_| {
                let var = self.infer.new_ty_var(self.ctx);
                GenericArg::Ty(self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(var))))
            })
            .collect();
        let substs = self.ctx.intern_substitution(substs);
        let enum_ty = self.ctx.mk_ty(TyKind::Adt(adt_id, substs));
        let has_fields = self
            .ctx
            .adt_def(adt_id)
            .and_then(|def| def.variants.get(variant_idx.index()))
            .map(|v| !v.fields.is_empty())
            .unwrap_or(false);
        if has_fields {
            let ctor_fn_def_id = FnDefId::from_raw(
                self.def_map
                    .variant_map
                    .iter()
                    .find(|(_, (e, _))| e.to_raw() == adt_id.to_raw())
                    .map(|(l, _)| l.to_raw())
                    .unwrap_or_else(|| adt_id.to_raw()),
            );
            let field_tys: Vec<Ty> = self
                .ctx
                .adt_def(adt_id)
                .and_then(|def| def.variants.get(variant_idx.index()))
                .map(|v| v.fields.iter().map(|f| f.ty).collect())
                .unwrap_or_default();
            let inputs = self
                .ctx
                .intern_substitution(field_tys.iter().map(|t| GenericArg::Ty(*t)).collect());
            self.ctx.register_fn_sig(
                ctor_fn_def_id,
                FnSig {
                    inputs,
                    output: enum_ty,
                    c_variadic: false,
                    unsafety: glyim_core::primitives::Safety::Safe,
                    abi: glyim_core::primitives::Abi::Glyim,
                },
            );
            let fn_ty = self.ctx.mk_ty(TyKind::FnDef(ctor_fn_def_id, substs));
            let thir_expr = thir::Expr {
                kind: thir::ExprKind::VariantCtor { adt_id, variant_idx },
                ty: fn_ty,
                span,
            };
            return (thir_expr, fn_ty);
        }
        let thir_expr = thir::Expr {
            kind: thir::ExprKind::VariantRef(adt_id, variant_idx),
            ty: enum_ty,
            span,
        };
        (thir_expr, enum_ty)
    }

    pub fn instantiate_fn_sig(&mut self, def_id: FnDefId, span: Span) -> Ty {
        // The function's signature was registered in the type context during
        // crate type-checking (see `register_fn_sig`). Prefer that source of
        // truth for the return type; fall back to scanning HIR items only when
        // no signature was registered (e.g. builtins).
        if let Some(sig) = self.ctx.fn_sig(def_id) {
            return sig.output;
        }
        for (_id, item) in self.hir.items.iter_enumerated() {
            if let glyim_hir::ItemKind::Fn(fn_item) = &item.kind {
                if let Some(return_ty_ref) = &fn_item.return_ty {
                    let param_map = std::collections::HashMap::new();
                    return crate::tyconv::resolve_type_ref(
                        self.ctx,
                        self.infer,
                        self.def_map,
                        self.diagnostics,
                        return_ty_ref,
                        &param_map,
                        span,
                    );
                } else {
                    return Ty::UNIT;
                }
            }
        }
        self.fresh_infer_ty()
    }
}

pub fn literal_ty(ctx: &mut TyCtxMut, infer: &mut InferenceTable, lit: &Literal) -> Ty {
    match lit {
        // Unsuffixed integer literals are integral inference variables (Rust
        // semantics): they unify with whatever integer type the context
        // expects (i32, i64, isize, u8, usize, …) and only default to `i32`
        // when left fully unconstrained. The parser tags unsuffixed literals as
        // `Some(IntTy::I32)` (the default), so we must treat `Some(I32)` /
        // `Some(Isize)` as inference vars too — only explicitly-suffixed
        // non-default hints stay concrete.
        Literal::Int(_, Some(IntTy::I32)) | Literal::Int(_, Some(IntTy::Isize)) | Literal::Int(_, None) => {
            let var = infer.new_int_var(ctx);
            ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
        }
        Literal::Int(_, Some(hint)) => ctx.mk_ty(TyKind::Int(*hint)),
        Literal::Uint(_, Some(hint)) => ctx.mk_ty(TyKind::Uint(*hint)),
        // Unsuffixed unsigned literals (parser default) also infer; explicit
        // suffixes stay concrete above.
        Literal::Uint(_, None) => {
            let var = infer.new_int_var(ctx);
            ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
        }
        Literal::Float(_, ft) => ctx.mk_ty(TyKind::Float(*ft)),
        Literal::Bool(_) => Ty::BOOL,
        Literal::Char(_) => ctx.mk_ty(TyKind::Char),
        Literal::String(_) => ctx.mk_ty(TyKind::String),
        Literal::Unit => Ty::UNIT,
    }
}

pub fn thir_literal(lit: &Literal) -> thir::Literal {
    match lit {
        Literal::Int(val, hint) => thir::Literal::Int(*val, *hint),
        Literal::Uint(val, hint) => thir::Literal::Uint(*val, *hint),
        Literal::Float(bits, ft) => thir::Literal::FloatBits(*bits, *ft),
        Literal::Bool(b) => thir::Literal::Bool(*b),
        Literal::Char(c) => thir::Literal::Char(*c),
        Literal::String(name) => thir::Literal::String(*name),
        Literal::Unit => thir::Literal::Unit,
    }
}
