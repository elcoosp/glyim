//! Pattern checking logic for FnCtxt.

use glyim_core::def_id::AdtId;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Name;
use glyim_core::primitives::Mutability;
use glyim_diag::GlyimDiagnostic;
use glyim_hir::{Pat, PatId};
use glyim_span::Span;
use glyim_type::{Ty, TyKind};

use crate::check_body::FnCtxt;
use crate::thir;


impl<'a> FnCtxt<'a> {
    /// Re-map a `Name` taken from the HIR (valid in `self.hir.interner`) into
    /// the type-checker's interner (`self.ctx.resolver()`). The HIR body and
    /// the `TyCtx` are sometimes built from different `Interner` instances, so
    /// a `Name` id that means `"y"` in one can mean `"x"` in the other.
    /// Resolving through the HIR interner recovers the textual identifier,
    /// which is then interned in the type-checker's interner so lexical
    /// lookups (guard/body `VarRef`s) match the binding.
    pub fn remap_name(&self, name: Name) -> Name {
        let s = self.hir.interner.resolve(name);
        self.ctx.resolver().intern(s)
    }

    /// Bind a HIR pattern into the local environment, returning the
    /// `LocalVarId` of the (first) binding it introduces. Used for closure
    /// parameters, mirroring how `check_pattern` / `let` statements bind
    /// `Pat::Binding` names into `self.env`.
    pub fn bind_pattern(&mut self, pat_id: PatId, ty: Ty, mutability: Mutability) -> thir::LocalVarId {
        let pat = &self.body.pats[pat_id];
        match pat {
            Pat::Binding { name, .. } => {
                let id = self.env.add_binding(*name, ty, mutability);
                thir::LocalVarId::from_raw(id.to_raw())
            }
            _ => {
                let id = self.env.add_binding(self.ctx.resolver().intern("_"), ty, mutability);
                thir::LocalVarId::from_raw(id.to_raw())
            }
        }
    }

    /// Type-check a pattern, producing the THIR pattern and binding names.
    pub fn check_pattern(&mut self, pat_id: PatId, expected_ty: Ty) -> thir::Pattern {
        let pat = &self.body.pats[pat_id];
        let span = Span::DUMMY;
        match pat {
            Pat::Wild => thir::Pattern::wild(expected_ty, span),
            Pat::Binding {
                name,
                mutability,
                subpattern,
            } => {
                // Re-map the HIR `Name` (valid in `self.hir.interner`) into the
                // type-checker's interner so lexical lookups in the guard/body
                // (which use `self.ctx.resolver()`) resolve against the same
                // identifier. See `CrateHir::interner` for why the two can
                // diverge.
                let ctx_name = self.remap_name(*name);
                // Bind under the ORIGINAL HIR `Name` as the primary key. Later
                // references resolve through `check_path` via `path.as_name()`
                // (the HIR `Name`), so the binding must be reachable under that
                // key too — otherwise `let x = ...; x` fails with "unresolved
                // name `x`" inside impl-method bodies, where the HIR and
                // type-checker interners differ (plain `fn` bodies share one
                // interner, so the bug was hidden there).
                let id = self.env.add_binding(*name, expected_ty, *mutability);
                if ctx_name != *name {
                    self.env.add_alias(ctx_name, id);
                }
                let sub =
                    subpattern.map(|sub_id| Box::new(self.check_pattern(sub_id, expected_ty)));
                thir::Pattern {
                    kind: thir::PatternKind::Binding {
                        var_id: thir::LocalVarId::from_raw(id.to_raw()),
                        name: ctx_name,
                        mutability: *mutability,
                        subpattern: sub,
                    },
                    ty: expected_ty,
                    span,
                }
            }
            Pat::Path(path) => {
                // Unit enum-variant pattern, e.g. `None` or `Color::Red`.
                let local = self.resolve_pat_path_local(path);
                match local {
                    Some(local) => {
                        if let Some((enum_local, vidx)) =
                            self.def_map.variant_map.get(&local)
                        {
                            let adt_id = AdtId::from_raw(enum_local.to_raw());
                            thir::Pattern {
                                kind: thir::PatternKind::Struct {
                                    adt_id,
                                    variant_idx: vidx.index() as u32,
                                    fields: Vec::new(),
                                    rest: false,
                                },
                                ty: expected_ty,
                                span,
                            }
                        } else {
                            let label = if let Some(name) = path.as_name() {
                                format!(
                                    "unsupported path pattern `{}`",
                                    self.ctx.name_str(name)
                                )
                            } else {
                                "unsupported path pattern".to_string()
                            };
                            self.diagnostics
                                .push(GlyimDiagnostic::type_error(span, label));
                            thir::Pattern::err(span)
                        }
                    }
                    None => {
                        let label = if let Some(name) = path.as_name() {
                            format!(
                                "unresolved path pattern `{}`",
                                self.ctx.name_str(name)
                            )
                        } else {
                            "unresolved path pattern".to_string()
                        };
                        self.diagnostics
                            .push(GlyimDiagnostic::type_error(span, label));
                        thir::Pattern::err(span)
                    }
                }
            }
            Pat::Struct { path, fields, rest } => {
                // The path may name an enum *variant* (data-carrying, e.g.
                // `OptionI32::Some(y)`) or a plain *struct*. The def map
                // registers each variant in the value namespace and provides a
                // reverse map `variant_local -> (enum_local, VariantIdx)`.
                let local = self.resolve_pat_path_local(path);
                let (adt_id, variant_idx, is_variant) = match local {
                    Some(local) => {
                        if let Some((enum_local, vidx)) =
                            self.def_map.variant_map.get(&local)
                        {
                            (
                                AdtId::from_raw(enum_local.to_raw()),
                                vidx.index() as u32,
                                true,
                            )
                        } else {
                            (AdtId::from_raw(local.to_raw()), 0, false)
                        }
                    }
                    None => {
                        // Fallback for HIR-generated enums (e.g. the async
                        // state-machine `FooState` enum synthesized by
                        // `desugar_multi_async_fn`) that have no syntax nodes
                        // and therefore never populate the def-map
                        // `variant_map` / synthetic variant module. Resolve the
                        // `Enum::Variant` path directly against the enum ADT's
                        // variant list by name.
                        if let Some((adt_id, vidx)) =
                            crate::tyconv::resolve_enum_variant_path(self.ctx, self.def_map, path)
                        {
                            (adt_id, vidx, true)
                        } else {
                            let label = if let Some(name) = path.as_name() {
                                format!("unresolved struct `{}`", self.ctx.name_str(name))
                            } else {
                                "unresolved struct path".to_string()
                            };
                            self.diagnostics.push(GlyimDiagnostic::type_error(span, label));
                            return thir::Pattern::err(span);
                        }
                    }
                };
                let adt_def = self.ctx.adt_def(adt_id);
                let adt_known = adt_def.is_some();
                let mut field_pats = Vec::new();
                if is_variant {
                    // Variant pattern: `fields` are positional (the inner
                    // patterns, in source order). Precompute each field's name
                    // and type so we can release the `adt_def` borrow before
                    // recursing into `check_pattern` (which needs `&mut self`).
                    let (field_names, field_tys): (Vec<_>, Vec<_>) = match adt_def
                        .as_ref()
                        .and_then(|d| d.variants.get(variant_idx as usize))
                    {
                        Some(variant) => (0..variant.fields.len())
                            .map(|i| {
                                let f = &variant.fields[glyim_type::FieldIdx::from_raw(i as u32)];
                                (f.name, f.ty)
                            })
                            .unzip(),
                        None => (Vec::new(), Vec::new()),
                    };
                    for (i, (_, field_pat_id)) in fields.iter().enumerate() {
                        let field_name = field_names
                            .get(i)
                            .copied()
                            .unwrap_or_else(|| self.ctx.resolver().intern(&format!("_{}", i)));
                        // Plan unstub-5 P5: the formal field type (e.g. `T` for
                        // `Poll::Ready(T)`) must be substituted through the
                        // scrutinee's substitution. When matching
                        // `Poll<F::Output>` against `Poll::Ready(v)`, `v` must
                        // get type `F::Output`, not the bare formal `T`.
                        let field_ty = match field_tys.get(i).copied() {
                            Some(formal) => {
                                let subst = match self.ctx.ty_kind(expected_ty) {
                                    TyKind::Adt(_, sub) => {
                                        let args = self.ctx.substitution_args(*sub);
                                        let mut m = std::collections::HashMap::new();
                                        for (idx, arg) in args.iter().enumerate() {
                                            if let glyim_type::GenericArg::Ty(t) = arg {
                                                m.insert(idx as u32, glyim_type::GenericArg::Ty(*t));
                                            }
                                        }
                                        m
                                    }
                                    _ => std::collections::HashMap::new(),
                                };
                                self.ctx.subst_ty(formal, &subst)
                            }
                            None => expected_ty,
                        };
                        let field_pat = self.check_pattern(*field_pat_id, field_ty);
                        field_pats.push(thir::FieldPat {
                            field: field_name,
                            pattern: field_pat,
                            span,
                        });
                    }
                } else {
                    // Struct pattern: named fields.
                    for (field_name, field_pat_id) in fields {
                        let field_ty = if adt_known {
                            self.lookup_field_ty(adt_id, *field_name, span)
                        } else {
                            expected_ty
                        };
                        self.env.add_binding(*field_name, field_ty, Mutability::Not);
                        let field_pat = self.check_pattern(*field_pat_id, field_ty);
                        field_pats.push(thir::FieldPat {
                            field: *field_name,
                            pattern: field_pat,
                            span,
                        });
                    }
                }
                thir::Pattern {
                    kind: thir::PatternKind::Struct {
                        adt_id,
                        variant_idx,
                        fields: field_pats,
                        rest: *rest,
                    },
                    ty: expected_ty,
                    span,
                }
            }
            Pat::Tuple(pats) => {
                let mut thir_pats = Vec::new();
                for &p_id in pats {
                    thir_pats.push(self.check_pattern(p_id, Ty::ERROR));
                }
                thir::Pattern {
                    kind: thir::PatternKind::Tuple(thir_pats),
                    ty: expected_ty,
                    span,
                }
            }
            Pat::Literal(lit) => {
                let thir_lit = crate::unify::thir_literal(lit);
                if expected_ty != Ty::ERROR {
                    let lit_ty = crate::unify::literal_ty(self.ctx, self.infer, lit);
                    self.unify(lit_ty, expected_ty, span);
                }
                thir::Pattern {
                    kind: thir::PatternKind::Literal(thir_lit),
                    ty: expected_ty,
                    span,
                }
            }
            Pat::Or(pats) => {
                let mut thir_pats = Vec::with_capacity(pats.len());
                let mut first_ty = None;
                for p_id in pats {
                    let sub_pat = self.check_pattern(*p_id, expected_ty);
                    if first_ty.is_none() {
                        first_ty = Some(sub_pat.ty);
                    } else if let Some(ty) = first_ty {
                        // All alternatives must have the same type
                        self.unify(ty, sub_pat.ty, span);
                    }
                    thir_pats.push(sub_pat);
                }
                let unified_ty = first_ty.unwrap_or(expected_ty);
                thir::Pattern {
                    kind: thir::PatternKind::Or(thir_pats),
                    ty: unified_ty,
                    span,
                }
            }
            Pat::Range {
                start,
                end,
                inclusive,
            } => {
                let start_opt = start.as_ref().map(crate::unify::thir_literal);
                let end_opt = end.as_ref().map(crate::unify::thir_literal);
                let ty = expected_ty;
                if ty != Ty::ERROR {
                    if let Some(lit) = start.as_ref() {
                        let lit_ty = crate::unify::literal_ty(self.ctx, self.infer, lit);
                        self.unify(lit_ty, ty, span);
                    }
                    if let Some(lit) = end.as_ref() {
                        let lit_ty = crate::unify::literal_ty(self.ctx, self.infer, lit);
                        self.unify(lit_ty, ty, span);
                    }
                }
                thir::Pattern {
                    kind: thir::PatternKind::Range {
                        start: start_opt,
                        end: end_opt,
                        inclusive: *inclusive,
                    },
                    ty,
                    span,
                }
            }
            Pat::Slice(elements) => {
                // Determine element type of the expected array/slice.
                let elem_ty = match self.ctx.ty_kind(expected_ty) {
                    TyKind::Array(ety, _) | TyKind::Slice(ety) => *ety,
                    _ => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            "slice pattern requires array or slice type",
                        ));
                        Ty::ERROR
                    }
                };
                if elem_ty == Ty::ERROR {
                    return thir::Pattern::err(span);
                }
                let mut prefix = Vec::new();
                let mut slice_pat = None;
                let mut suffix = Vec::new();
                let mut found_slice = false;
                for &sub_id in elements {
                    let sub = &self.body.pats[sub_id];
                    // A slice‑binding pattern is a wildcard or a simple binding (no subpattern).
                    let is_slice = matches!(
                        sub,
                        Pat::Wild
                            | Pat::Binding {
                                subpattern: None,
                                ..
                            }
                    );
                    if !found_slice && is_slice {
                        // This is the `..` or `rest @ ..` pattern.
                        slice_pat = Some(Box::new(self.check_pattern(sub_id, expected_ty)));
                        found_slice = true;
                    } else if !found_slice {
                        prefix.push(self.check_pattern(sub_id, elem_ty));
                    } else {
                        suffix.push(self.check_pattern(sub_id, elem_ty));
                    }
                }
                // Fixed-size array patterns without a `..` must name exactly
                // `N` elements; otherwise the pattern can never match. (Plan
                // §9.2: slice-pattern length must be validated. Slices `[T]`
                // are runtime-sized and cannot be checked statically, so only
                // `Array` subjects get the arity check.)
                if let TyKind::Array(_, len_const) = self.ctx.ty_kind(expected_ty) {
                    if slice_pat.is_none() {
                        let n = match &len_const.kind {
                            glyim_type::ConstKind::Uint(n) => *n as usize,
                            glyim_type::ConstKind::Int(n) => *n as usize,
                            _ => usize::MAX,
                        };
                        let named = prefix.len() + suffix.len();
                        if named != n {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!(
                                    "slice pattern matches {named} element(s) but array has {n} element(s)"
                                ),
                            ));
                        }
                    }
                }
                // No length check needed for `..`-containing or slice-typed
                // patterns; the runtime match handles those.
                thir::Pattern {
                    kind: thir::PatternKind::Slice {
                        prefix,
                        slice: slice_pat,
                        suffix,
                    },
                    ty: expected_ty,
                    span,
                }
            }
            _ => {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "unsupported pattern kind",
                ));
                thir::Pattern::err(span)
            }
        }
    }

    /// Resolve a pattern path (struct or enum-variant name) to the
    /// `LocalDefId` of the thing it names. Variants live in the value
    /// namespace; structs/enums live in the type namespace. Uses the def map's
    /// `Resolver` so multi‑segment paths (e.g. `OptionI32::Some`) walk the
    /// module tree correctly.
    fn resolve_pat_path_local(&self, path: &glyim_hir::Path) -> Option<LocalDefId> {
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
        let resolver = glyim_def_map::Resolver::new(
            &self.def_map.modules,
            self.def_map.root,
            self.current_module,
        );
        let resolved = resolver.resolve_path(&core_path);
        resolved
            .values
            .or(resolved.types)
            .map(|(local, _vis)| local)
    }
}
