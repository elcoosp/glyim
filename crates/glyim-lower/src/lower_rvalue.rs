use crate::builder::{LoopInfo, MirBuilder};
use crate::lower_terminator::TerminatorExt;
use glyim_core::TargetInfo;
use glyim_core::primitives::{BinOp, Mutability};
use glyim_diag::GlyimDiagnostic;
use glyim_layout::LayoutComputer;
use glyim_mir::{
    self, BasicBlockIdx, CastKind, LocalIdx, MirConst, MirConstKind, Operand, Place,
    ProjectionElem, Rvalue, StatementKind, SwitchTargets, TerminatorKind,
};
use glyim_type::{self, FieldIdx, Ty, TyKind};
use glyim_typeck::thir;

/// Whether `ty` is or contains a `TyKind::Param` / `TyKind::Infer` anywhere.
fn contains_param(ctx: &glyim_type::TyCtx, ty: Ty) -> bool {
    match ctx.ty_kind(ty) {
        TyKind::Param(_) | TyKind::Infer(_) => true,
        TyKind::Ref(_, i, _) | TyKind::RawPtr(i, _) | TyKind::Slice(i) | TyKind::Array(i, _) => {
            contains_param(ctx, *i)
        }
        TyKind::Adt(_, s) | TyKind::Tuple(s) => ctx
            .substitution_args(*s)
            .iter()
            .any(|a| matches!(a, glyim_type::GenericArg::Ty(t) if contains_param(ctx, *t))),
        _ => false,
    }
}

impl<'a> MirBuilder<'a> {
    // ---- Statement lowering ----
    /// lower_stmt.
    pub fn lower_stmt(&mut self, stmt: &thir::Stmt) {
        match stmt {
            thir::Stmt::Let {
                name,
                ty,
                init,
                span,
                pat,
            } => {
                let init_local = if let Some(init_expr) = init {
                    let temp_local =
                        self.alloc_local(*ty, glyim_core::primitives::Mutability::Mut, *span);
                    self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), *span);
                    let rvalue = self.lower_expr_to_rvalue(init_expr);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(glyim_mir::Place::new(temp_local), rvalue),
                        *span,
                    );
                    Some(temp_local)
                } else {
                    None
                };
                self.bind_pattern(
                    pat,
                    init_local.map(glyim_mir::Place::new),
                    init_local.map(glyim_mir::Place::new),
                    *span,
                );
                if let thir::PatternKind::Binding {
                    name: bind_name, ..
                } = &pat.kind
                {
                    if !self.var_map.contains_key(bind_name)
                        && let Some(local) = init_local
                    {
                        self.var_map.insert(*bind_name, local);
                    }
                } else if !self.var_map.contains_key(name)
                    && let Some(local) = init_local
                {
                    self.var_map.insert(*name, local);
                }
            }
            thir::Stmt::Assign { lhs, rhs, span } => {
                let place = self.lower_expr_to_place(lhs);
                let rvalue = self.lower_expr_to_rvalue(rhs);
                self.push_stmt(glyim_mir::StatementKind::Assign(place, rvalue), *span);
            }
            thir::Stmt::Return { value, span } => {
                if let Some(val_expr) = value {
                    let rvalue = self.lower_expr_to_rvalue(val_expr);
                    let ret_place = glyim_mir::Place::new(LocalIdx::from_raw(0));
                    self.push_stmt(glyim_mir::StatementKind::Assign(ret_place, rvalue), *span);
                }
                self.terminate(glyim_mir::TerminatorKind::Return, *span);
            }
            thir::Stmt::Expr { expr } => {
                let rvalue = self.lower_expr_to_rvalue(expr);
                let temp =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                self.push_stmt(glyim_mir::StatementKind::StorageLive(temp), expr.span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(glyim_mir::Place::new(temp), rvalue),
                    expr.span,
                );
            }
        }
    }

    // ---- Expression → Rvalue lowering ----
    /// lower_expr_to_rvalue.
    pub fn lower_expr_to_rvalue(&mut self, expr: &thir::Expr) -> glyim_mir::Rvalue {
        match &expr.kind {
            thir::ExprKind::Literal(lit) => {
                let mir_const = self.lower_literal(lit, expr.ty, expr.span);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(mir_const))
            }
            thir::ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                // Lower a range to a real `Range<T>` / `RangeInclusive<T>`
                // aggregate (the builtin ADTs registered at ids 1000/1001)
                // carrying its `start` and `end` operands, instead of a dummy
                // empty tuple.
                let adt_id =
                    glyim_core::def_id::AdtId::from_raw(if *inclusive { 1001 } else { 1000 });

                // The range type is resolved during type-checking to
                // `Adt(1000|1001, [T])`; reuse its substitution so the element
                // type lines up with the operand types.
                let (substs, elem_ty) = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                    glyim_type::TyKind::Adt(_, s) => {
                        let args = self.ctx.ty_ctx().substitution_args(*s);
                        let elem = args
                            .first()
                            .and_then(|a| match a {
                                glyim_type::GenericArg::Ty(t) => Some(*t),
                                _ => None,
                            })
                            .unwrap_or_else(|| self.ctx.ty_ctx().error_ty());
                        (*s, elem)
                    }
                    _ => {
                        let elem = start
                            .as_ref()
                            .map(|e| e.ty)
                            .or_else(|| end.as_ref().map(|e| e.ty))
                            .unwrap_or_else(|| self.ctx.ty_ctx().error_ty());
                        // No resolved substitution available; emit an empty one
                        // and rely on the element type for operand shapes.
                        (glyim_type::Substitution::empty(), elem)
                    }
                };

                let start_op = match start {
                    Some(e) => self.lower_expr_to_operand(e),
                    None => glyim_mir::Operand::Constant(glyim_mir::MirConst {
                        kind: glyim_mir::MirConstKind::Error,
                        ty: elem_ty,
                        span: expr.span,
                    }),
                };
                let end_op = match end {
                    Some(e) => self.lower_expr_to_operand(e),
                    None => glyim_mir::Operand::Constant(glyim_mir::MirConst {
                        kind: glyim_mir::MirConstKind::Error,
                        ty: elem_ty,
                        span: expr.span,
                    }),
                };

                glyim_mir::Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Adt(
                        adt_id,
                        glyim_mir::VariantIdx::from_raw(0),
                        substs,
                    ),
                    vec![start_op, end_op],
                )
            }
            thir::ExprKind::VarRef(var_id) => {
                let local = self.local_for_var(*var_id);
                // HIR-31: a by-ref capture holds `&T`; the body reads `T`.
                let place = if self.byref_capture_vars.contains(var_id) {
                    self.place_with_projection(glyim_mir::Place::new(local), ProjectionElem::Deref)
                } else {
                    glyim_mir::Place::new(local)
                };
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(place))
            }
            thir::ExprKind::FnRef(_def_id) => {
                let (fn_def_id, substs) = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                    TyKind::FnDef(id, sub) => (id, sub),
                    _ => {
                        tracing::warn!("FnRef with non-FnDef type, emitting Error constant");
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Error,
                                ty: expr.ty,
                                span: expr.span,
                            },
                        ));
                    }
                };
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Fn(*fn_def_id, *substs),
                    ty: expr.ty,
                    span: expr.span,
                }))
            }
            thir::ExprKind::ConstRef(const_def_id) => {
                // Part C: const value materialization. If typeck const-
                // evaluated this constant, fold it into a concrete `MirConst`
                // via `LowerCtx::const_value` (scalar constants fold fully;
                // aggregate/range constants return `None` here and fall back to
                // the `ConstRef` zero-initialized global below).
                let substs = glyim_type::Substitution::empty();
                if let Some(mir_const) = self.ctx.const_value(*const_def_id, substs) {
                    return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(mir_const));
                }
                // Fallback: emit a `ConstRef` referencing the constant's
                // definition. Handled by mono, polymorphize, and the LLVM
                // backend (global `__glyim_const_{id}`). Substs are empty for
                // the monomorphic consts supported here.
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::ConstRef(*const_def_id, substs),
                    ty: expr.ty,
                    span: expr.span,
                }))
            }
            thir::ExprKind::StaticRef(static_def_id) => {
                // B.1: `StaticRef` in *rvalue* position (by-value read).
                // Lower to:
                //   _addr = Use(Constant(StaticRef { .. }))     ; the address
                //   _val  = Use(Copy(Place{_addr, [Deref]}))    ; the load
                // expressed as a two-statement pattern inside a single rvalue.
                // The builder only supports one statement here, so emit the
                // load via a `Deref` on a temporary local holding the address.
                //
                // Fallback: if the initializer hasn't been const-evaluated
                // (e.g. it references a runtime value), surface a diagnostic.
                let const_alias =
                    glyim_core::def_id::ConstDefId::from_raw(static_def_id.to_raw());
                let substs = glyim_type::Substitution::empty();
                let Some(init) = self.ctx.const_value(const_alias, substs) else {
                    // Typeck must have rejected a non-const-evaluable
                    // static initializer (Rust E0015). Reaching this arm
                    // means typeck's `ItemKind::Static` diagnostic was
                    // suppressed — surface a loud internal error rather
                    // than silently folding to 0, which would turn a
                    // compile error into a runtime miscompile.
                    self.diagnostics.push(GlyimDiagnostic::internal_error(format!(
                        "static {} reached lowering without a const-evaluated \
                         initializer; typeck should have rejected it",
                        static_def_id.to_raw()
                    )));
                    return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                        glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Error,
                            ty: expr.ty,
                            span: expr.span,
                        },
                    ));
                };
                let is_mut = self
                    .ctx
                    .ty_ctx()
                    .static_is_mut(*static_def_id);
                let static_ref_const = glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::StaticRef {
                        def_id: *static_def_id,
                        init: Box::new(init),
                        is_mut,
                    },
                    ty: expr.ty,
                    span: expr.span,
                };
                // Emit a place that reads through a Deref of the address. We
                // need a temp local holding the address; builders in this
                // module expose `alloc_local`.
                let ptr_ty = self.ctx.ty_ctx().mk_ty(glyim_type::TyKind::RawPtr(
                    expr.ty,
                    glyim_core::primitives::Mutability::Not,
                ));
                let addr_local = self
                    .alloc_local(ptr_ty, glyim_core::primitives::Mutability::Not, expr.span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(addr_local),
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(static_ref_const)),
                    ),
                    expr.span,
                );
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(
                    glyim_mir::Place {
                        local: addr_local,
                        projection: vec![glyim_mir::ProjectionElem::Deref].into_boxed_slice(),
                    },
                ))
            }
            thir::ExprKind::VariantRef(adt_id, variant_idx) => {
                // A unit enum variant used as a value (`Color::Red`) constructs
                // the enum via an `Aggregate` of its ADT with no fields. Data
                // variants (call-like constructors) are a follow-up; this
                // covers the unit-variant value path.
                let substs = glyim_type::Substitution::empty();
                glyim_mir::Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Adt(
                        *adt_id,
                        glyim_mir::VariantIdx::from_raw(variant_idx.to_raw()),
                        substs,
                    ),
                    vec![],
                )
            }
            thir::ExprKind::Binary { op, lhs, rhs } => {
                // LL-10: `&&`/`||` must short-circuit. Lowering them to an
                // eager `BinaryOp(And/Or)` (which the LLVM backend turns into
                // `build_and`/`build_or`) evaluated the RHS unconditionally —
                // `i < len && arr[i] == x` read `arr[i]` even when the bounds
                // check failed. Desugar into control flow (mirroring the `If`
                // arm) with a destination local.
                if matches!(*op, BinOp::And | BinOp::Or) {
                    let bool_ty = self.ctx.ty_ctx().bool_ty();
                    let dest_local = self.alloc_local(
                        bool_ty,
                        glyim_core::primitives::Mutability::Mut,
                        expr.span,
                    );
                    let dest_place = glyim_mir::Place::new(dest_local);
                    let rhs_bb = self.new_block();
                    let short_bb = self.new_block();
                    let merge_bb = self.new_block();
                    let mk_bool = |v: bool| {
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Bool(v),
                            ty: bool_ty,
                            span: expr.span,
                        }))
                    };
                    // For `&&`: lhs true (1) -> evaluate rhs, else short to false.
                    // For `||`: lhs true (1) -> short to true, else evaluate rhs.
                    let is_and = matches!(*op, BinOp::And);
                    let lhs_op = self.lower_expr_to_operand(lhs);
                    let (one_target, otherwise_target) = if is_and {
                        (rhs_bb, short_bb)
                    } else {
                        (short_bb, rhs_bb)
                    };
                    self.terminate(
                        glyim_mir::TerminatorKind::SwitchInt {
                            discr: lhs_op,
                            switch_ty: bool_ty,
                            targets: glyim_mir::SwitchTargets::new(
                                Box::new([(1, one_target)]),
                                otherwise_target,
                            ),
                        },
                        expr.span,
                    );
                    // Short-circuit block: the constant result (false for `&&`,
                    // true for `||`).
                    self.current_block = Some(short_bb);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(dest_place.clone(), mk_bool(!is_and)),
                        expr.span,
                    );
                    self.terminate(
                        glyim_mir::TerminatorKind::Goto { target: merge_bb },
                        expr.span,
                    );
                    // RHS block: evaluate the right operand.
                    self.current_block = Some(rhs_bb);
                    let rhs_val = self.lower_expr_to_rvalue(rhs);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(dest_place.clone(), rhs_val),
                        rhs.span,
                    );
                    self.terminate(
                        glyim_mir::TerminatorKind::Goto { target: merge_bb },
                        expr.span,
                    );
                    self.current_block = Some(merge_bb);
                    return glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(dest_place));
                }
                let lhs_op = self.lower_expr_to_operand(lhs);
                let rhs_op = self.lower_expr_to_operand(rhs);
                glyim_mir::Rvalue::BinaryOp(*op, Box::new((lhs_op, rhs_op)))
            }
            thir::ExprKind::Unary { op, operand } => {
                let op_val = self.lower_expr_to_operand(operand);
                glyim_mir::Rvalue::UnaryOp(*op, op_val)
            }
            thir::ExprKind::Ref {
                mutability,
                operand,
            } => {
                // `&STATIC` / `&mut STATIC`: the operand lowers to a
                // `StaticRef` constant which IS the pointer value; wrapping
                // it in `Rvalue::Ref(Place{addr,[Deref]})` makes the
                // interpreter capture a temp-local ref instead of the
                // static itself (correctness gap in `read_place`/`write_place`
                // deref handling — treating the StaticRef constant as the
                // pointer directly avoids it).
                if let thir::ExprKind::StaticRef(static_def_id) = &operand.kind {
                    let const_alias =
                        glyim_core::def_id::ConstDefId::from_raw(static_def_id.to_raw());
                    let substs = glyim_type::Substitution::empty();
                    if let Some(init) = self.ctx.const_value(const_alias, substs) {
                        let is_mut = self.ctx.ty_ctx().static_is_mut(*static_def_id);
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::StaticRef {
                                    def_id: *static_def_id,
                                    init: Box::new(init),
                                    is_mut,
                                },
                                ty: expr.ty,
                                span: expr.span,
                            },
                        ));
                    }
                }
                // `&base[a..b]` (a sub-slice) is *not* a `Ref` of a `Place`:
                // the sub-slice value is itself a fat pointer `{data_ptr, len}`,
                // so a reference to it is the same fat pointer. `Place` cannot
                // represent an unsized `[T]`, so route the range-slice case
                // through `lower_dynamic_range_slice` (via the `Index { Range }`
                // arm in `lower_expr_to_rvalue`) and return that value directly.
                if let thir::ExprKind::Index { index, .. } = &operand.kind
                    && matches!(index.kind, thir::ExprKind::Range { .. })
                {
                    return self.lower_expr_to_rvalue(operand);
                }
                let place = self.lower_expr_to_place(operand);
                let borrow_kind = match mutability {
                    glyim_core::primitives::Mutability::Mut => glyim_mir::BorrowKind::Mut {
                        // T121-PATCHED [BCK-1]: `&mut x` as a direct call
                        // argument is a two-phase borrow (`f(&mut x, x)`
                        // is legal until the mutable borrow activates).
                        allow_two_phase_borrow: self.lowering_call_arg,
                    },
                    glyim_core::primitives::Mutability::Not => glyim_mir::BorrowKind::Shared,
                };
                glyim_mir::Rvalue::Ref(place, borrow_kind)
            }
            thir::ExprKind::Call { func, args } => {
                // CONST-FN-FOLD: `mem::size_of::<T>()` / `align_of::<T>()` /
                // `needs_drop::<T>()` resolve to the empty `.g` stub fns. Fold
                // them into an integer constant using the generic `T` from the
                // callee's `FnDef` substitution + the layout computer. The
                // callee is identified by name (see the `const_fns` registry).
                if let thir::ExprKind::FnRef(def_id) = func.kind
                    && let Some(name) = self.ctx.ty_ctx().const_fn_name(def_id)
                    && let glyim_type::TyKind::FnDef(_, substs) = self.ctx.ty_ctx().ty_kind(func.ty)
                {
                    let ty_arg = self
                        .ctx
                        .ty_ctx()
                        .substitution_args(*substs)
                        .first()
                        .and_then(|a| match a {
                            glyim_type::GenericArg::Ty(t) => Some(*t),
                            _ => None,
                        });
                    if let Some(t) = ty_arg {
                        let value: i128 = match name {
                            "needs_drop" => i128::from(self.ctx.ty_ctx().needs_drop(t)),
                            _ => {
                                let lc = glyim_layout::SimpleLayoutComputer::new(
                                    self.ctx.ty_ctx(),
                                    TargetInfo::x86_64(),
                                );
                                match lc.layout_of(t) {
                                    Ok(l) => {
                                        let n = if name == "align_of" {
                                            l.align.0
                                        } else {
                                            l.size.0
                                        };
                                        n as i128
                                    }
                                    Err(_) => 0,
                                }
                            }
                        };
                        let _ = args;
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Uint(value as u128),
                                ty: expr.ty,
                                span: expr.span,
                            },
                        ));
                    }
                }
                // Data-carrying variant constructor call (`Some(x)` /
                // `Color::Green(x)`): lower to an `Aggregate` of the enum ADT
                // instead of a function call.
                if let thir::ExprKind::VariantCtor {
                    adt_id,
                    variant_idx,
                } = &func.kind
                {
                    let mut mir_args = Vec::new();
                    for arg in args {
                        mir_args.push(self.lower_expr_to_operand(arg));
                    }
                    // Derive the aggregate's substitution from the call
                    // expression's type. The previous `Substitution::empty()`
                    // gave a generic enum like `Result<T, E>` a zero-arity
                    // substitution even though the ADT's arity is 2, so
                    // layout computing for the aggregate's fields saw
                    // `Ty::ERROR` (`UnknownType(Ty(0))`). Non-generic enums
                    // (arity 0) worked by accident. Mirrors the struct-literal
                    // arm below, which already reads `expr.ty`.
                    let substs = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                        glyim_type::TyKind::Adt(_, s) => *s,
                        _ => glyim_type::Substitution::empty(),
                    };
                    return glyim_mir::Rvalue::Aggregate(
                        glyim_mir::AggregateKind::Adt(
                            *adt_id,
                            glyim_mir::VariantIdx::from_raw(variant_idx.to_raw()),
                            substs,
                        ),
                        mir_args,
                    );
                }

                // T140-PATCHED-SENTINEL: compiler-synthetic builtin methods
                // (`into`, `to_owned`/`to_string`, `clone`, `drop`) have no
                // source-level `FnDef`, so typeck resolves them to a sentinel
                // `FnDefId` in the top four id slots (`u32::MAX - 4 ..=
                // u32::MAX - 1`; see `check_expr.rs:2574/2579/2587` and
                // `unify.rs:94`). Emitting a real `Call` with a sentinel id
                // makes LLVM codegen ICE with `no signature registered for
                // function FnDefId(4294967293)`.
                //
                // These calls appear only on runtime-dead error paths — the
                // canonical case is the error arm of `Result::unwrap`
                // (`E::to_string()` for a generic `E`). Store an `Error`
                // constant into the destination instead of a `Call`
                // terminator: codegen lowers that to a zero value of the
                // destination type (`T140-PATCHED-LLVM-ERROR`), so the
                // function stays well-formed and the live path executes
                // correctly.
                //
                // Follow-up (session handoff): replace the sentinel with a
                // real virtual-method reference resolved at monomorphization,
                // so the error branch produces a correct message rather than
                // a zero value.
                if let glyim_type::TyKind::FnDef(def_id, _) = self.ctx.ty_ctx().ty_kind(func.ty) {
                    if def_id.to_raw() >= u32::MAX - 4 {
                        // P1-C1 (production-readiness plan §6): the `clone`
                        // (u32::MAX - 3) and `into` (u32::MAX - 1) sentinels
                        // were lowering to a zeroing `Error` constant even on
                        // *live* paths — `let x: i32 = 5; let y = x.clone();`
                        // produced `y = 0`. The typeck-side fix is
                        // Phase 1 P1-C1's registry-first pass; this lowering
                        // handles the concrete case the plan calls out first:
                        // **for a Copy receiver whose result type is the same
                        // as the receiver type**, a `clone`/`into` is exactly
                        // a plain operand use. Any other shape falls through
                        // to the existing `Error` path (unchanged — no
                        // regression).
                        let is_copy_sentinel = matches!(
                            def_id.to_raw(),
                            x if x == u32::MAX - 3 || x == u32::MAX - 1
                        );
                        if is_copy_sentinel
                            && let Some(arg_expr) = args.first()
                        {
                            let arg_ty = arg_expr.ty;
                            if arg_ty == expr.ty
                                && self.ctx.ty_ctx().is_copy(arg_ty)
                            {
                                return glyim_mir::Rvalue::Use(
                                    self.lower_expr_to_operand(arg_expr),
                                );
                            }
                        }
                        let dest_local = self.alloc_local(
                            expr.ty,
                            glyim_core::primitives::Mutability::Mut,
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(dest_local),
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(dest_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                                    glyim_mir::MirConst {
                                        kind: glyim_mir::MirConstKind::Error,
                                        ty: expr.ty,
                                        span: expr.span,
                                    },
                                )),
                            ),
                            expr.span,
                        );
                        // A `-> !` sentinel (`panic_any`) never returns, so
                        // the current block must be terminated as
                        // `Unreachable`. Leaving it open makes the *caller*
                        // (e.g. the `assert!` desugar's match arm) still
                        // emit its normal "assign + Goto(merge)" epilogue
                        // into an already-terminated block — and, when the
                        // caller has no fall-through, the freshly-created
                        // continuation block is never patched and stays a
                        // dangling `Unreachable` that later gets rewritten to
                        // `Goto(self)`, an infinite loop. `assert!(false)`
                        // hung for exactly this reason.
                        // `expr.ty` is unreliable here: for `panic_any(msg:
                        // impl Display)` the call expression's recorded type
                        // is the unresolved impl-Trait `Param(0)`, not `Never`.
                        // Consult the *registered* signature instead.
                        let sig_returns_never = self
                            .ctx
                            .ty_ctx()
                            .fn_sig(*def_id)
                            .map(|sig| {
                                matches!(
                                    self.ctx.ty_ctx().ty_kind(sig.output),
                                    glyim_type::TyKind::Never
                                )
                            })
                            .unwrap_or(false);
                        if sig_returns_never {
                            self.terminate(
                                glyim_mir::TerminatorKind::Unreachable,
                                expr.span,
                            );
                        }
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(
                            glyim_mir::Place::new(dest_local),
                        ));
                    }
                }

                let mut mir_args = Vec::new();
                // T121-PATCHED-ACTIVATION [BCK-1]: precise two-phase borrow
                // activation. Instead of marking *every* call argument as a
                // two-phase borrow, mark only `&mut <local>` arguments whose
                // sibling arguments read the same local — the exact
                // precondition under which rustc reserves a two-phase borrow
                // (`f(&mut x, x)`). An `&mut x` with no sibling read is a
                // plain mutable borrow; leaving the flag off restores rustc's
                // activation semantics for it (the reservation no longer
                // outlives the call). The analysis is over-approximate in the
                // "keep the reservation" direction, so it never rejects a
                // program the coarse version accepted.
                let saved_flag = self.lowering_call_arg;
                let tp_flags = compute_two_phase_flags(args);
                for (i, arg) in args.iter().enumerate() {
                    self.lowering_call_arg = tp_flags.get(i).copied().unwrap_or(false);
                    mir_args.push(self.lower_expr_to_operand(arg));
                }
                self.lowering_call_arg = saved_flag;
                let func_op = self.lower_expr_to_operand(func);
                let dest_local =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                let dest_place = glyim_mir::Place::new(dest_local);
                let next_bb = self.new_block();
                self.terminate(
                    glyim_mir::TerminatorKind::Call {
                        func: func_op,
                        args: mir_args,
                        destination: dest_place.clone(),
                        target: Some(next_bb),
                        cleanup: None,
                    },
                    expr.span,
                );
                self.current_block = Some(next_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(dest_place))
            }
            thir::ExprKind::VariantCtor { .. } => {
                // A bare variant constructor value is not yet supported as a
                // first-class function value; it is only valid as a call
                // target (handled in the `Call` arm above). Emit an error
                // rvalue to keep lowering exhaustive.
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Error,
                    ty: expr.ty,
                    span: expr.span,
                }))
            }
            thir::ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_op = self.lower_expr_to_operand(cond);
                let then_bb = self.new_block();
                let else_bb = self.new_block();
                let merge_bb = self.new_block();
                let dest_local =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                let dest_place = glyim_mir::Place::new(dest_local);
                let targets = glyim_mir::SwitchTargets::new(Box::new([(1, then_bb)]), else_bb);
                self.terminate(
                    glyim_mir::TerminatorKind::SwitchInt {
                        discr: cond_op,
                        switch_ty: cond.ty,
                        targets,
                    },
                    expr.span,
                );
                self.current_block = Some(then_bb);
                let then_val = self.lower_expr_to_rvalue(then_branch);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(dest_place.clone(), then_val),
                    then_branch.span,
                );
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: merge_bb },
                    then_branch.span,
                );
                self.current_block = Some(else_bb);
                if let Some(else_b) = else_branch {
                    let else_val = self.lower_expr_to_rvalue(else_b);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(dest_place.clone(), else_val),
                        else_b.span,
                    );
                }
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: merge_bb },
                    expr.span,
                );
                self.current_block = Some(merge_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(dest_place))
            }
            thir::ExprKind::Match { scrutinee, arms } => {
                self.lower_match(scrutinee, arms, expr.ty, expr.span)
            }
            thir::ExprKind::Block { stmts, tail } => {
                // A block is a drop scope: `let` bindings inside it are
                // destroyed at its `}`, not at the enclosing function's
                // return. Track them, and emit their `Drop`s on the way out.
                self.enter_block_scope();
                for stmt in stmts {
                    self.lower_stmt(stmt);
                    if self.current_block.is_none() {
                        // Diverged (`return`/`break`/`panic!`): the block's
                        // scope-exit drops are unreachable from here, and the
                        // divergent path emits its own terminators.
                        self.block_scopes.pop();
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Unit,
                                ty: Ty::NEVER,
                                span: expr.span,
                            },
                        ));
                    }
                }

                // Fast path: nothing in this block needs dropping, so the
                // block is purely structural — return the tail rvalue
                // directly, with no extra block or temp. This keeps the MIR
                // shape of `{ <tail> }` identical to a bare `<tail>`.
                let needs_scope_drops = self
                    .block_scopes
                    .last()
                    .map(|frame| !frame.is_empty())
                    .unwrap_or(false);
                if !needs_scope_drops {
                    self.block_scopes.pop();
                    return match tail {
                        Some(t) => self.lower_expr_to_rvalue(t),
                        None => glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Unit,
                                ty: Ty::UNIT,
                                span: expr.span,
                            },
                        )),
                    };
                }

                // Otherwise the tail value must be computed *before* the
                // drops run (the drops end the block), so stage it into a
                // temp and hand the caller a read of that temp.
                let tail_ty = tail.as_ref().map(|t| t.ty).unwrap_or(Ty::UNIT);
                let tail_rvalue = match tail {
                    Some(t) => self.lower_expr_to_rvalue(t),
                    None => glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                        glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Unit,
                            ty: Ty::UNIT,
                            span: expr.span,
                        },
                    )),
                };
                if self.current_block.is_none() {
                    // The tail itself diverged.
                    self.block_scopes.pop();
                    return tail_rvalue;
                }

                let result_local = self.alloc_local(
                    tail_ty,
                    glyim_core::primitives::Mutability::Mut,
                    expr.span,
                );
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(result_local),
                        tail_rvalue,
                    ),
                    expr.span,
                );

                // Emit the block's drops, then continue in `after_bb`.
                let after_bb = self.new_block();
                self.leave_block_scope(after_bb, expr.span);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(glyim_mir::Place::new(
                    result_local,
                )))
            }
            thir::ExprKind::While { cond, body } => {
                let header_bb = self.new_block();
                let body_bb = self.new_block();
                let exit_bb = self.new_block();
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: header_bb },
                    expr.span,
                );
                self.current_block = Some(header_bb);
                let cond_op = self.lower_expr_to_operand(cond);
                let targets = glyim_mir::SwitchTargets::new(Box::new([(1, body_bb)]), exit_bb);
                self.terminate(
                    glyim_mir::TerminatorKind::SwitchInt {
                        discr: cond_op,
                        switch_ty: cond.ty,
                        targets,
                    },
                    cond.span,
                );
                let break_place =
                    self.alloc_local(Ty::UNIT, glyim_core::primitives::Mutability::Mut, expr.span);
                self.loop_stack.push(LoopInfo {
                    continue_bb: header_bb,
                    break_bb: exit_bb,
                    break_place,
                });
                self.current_block = Some(body_bb);
                let _ = self.lower_expr_to_rvalue(body);
                self.loop_stack.pop();
                if self.current_block.is_some() {
                    self.terminate(
                        glyim_mir::TerminatorKind::Goto { target: header_bb },
                        body.span,
                    );
                }
                self.current_block = Some(exit_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: expr.span,
                }))
            }
            thir::ExprKind::Loop { body } => {
                let loop_bb = self.new_block();
                let exit_bb = self.new_block();
                // HIR-30: `loop { break v; }` evaluates to `v`. Materialize a
                // result local of the loop's type; `Break` writes it, and the
                // loop expression reads it back at `exit_bb`.
                let break_place =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: loop_bb },
                    expr.span,
                );
                self.loop_stack.push(LoopInfo {
                    continue_bb: loop_bb,
                    break_bb: exit_bb,
                    break_place,
                });
                self.current_block = Some(loop_bb);
                let _ = self.lower_expr_to_rvalue(body);
                self.loop_stack.pop();
                if self.current_block.is_some() {
                    self.terminate(
                        glyim_mir::TerminatorKind::Goto { target: loop_bb },
                        body.span,
                    );
                }
                self.current_block = Some(exit_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(glyim_mir::Place::new(break_place)))
            }
            thir::ExprKind::For {
                pat,
                iterable,
                body,
                next,
                into_iter,
            } => {
                let iter_ty = iterable.ty;
                let elem_ty = pat.ty;
                let initial_iter_local = self.alloc_local(
                    iter_ty,
                    glyim_core::primitives::Mutability::Mut,
                    iterable.span,
                );
                self.push_stmt(
                    glyim_mir::StatementKind::StorageLive(initial_iter_local),
                    iterable.span,
                );
                let iter_rvalue = self.lower_expr_to_rvalue(iterable);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(initial_iter_local),
                        iter_rvalue,
                    ),
                    iterable.span,
                );
                // Vec-by-value (`for x in v` where `v: Vec<T>`): emit
                // `IntoIterator::into_iter(v)` into a fresh local and rebind
                // `iter_local` to it. The subsequent `next` call then operates
                // on `VecIntoIter<T>` (its declared output), not on the Vec
                // itself — avoiding the segfaulting 3-word-vs-2-word mismatch.
                // `iter_ty` is rebound too, so the `&Vec` slice-walk match
                // below does not fire (its Vec-arm requires an `Adt(1020)`).
                let (iter_ty, iter_local) = if let Some(info) = into_iter.as_ref() {
                    let output_local = self.alloc_local(
                        info.output_ty,
                        glyim_core::primitives::Mutability::Mut,
                        iterable.span,
                    );
                    self.push_stmt(
                        glyim_mir::StatementKind::StorageLive(output_local),
                        iterable.span,
                    );
                    let after_call = self.new_block();
                    self.terminate(
                        glyim_mir::TerminatorKind::Call {
                            func: glyim_mir::Operand::Constant(glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Fn(
                                    info.fn_def_id,
                                    info.fn_substs,
                                ),
                                ty: info.fn_ty,
                                span: iterable.span,
                            }),
                            args: vec![glyim_mir::Operand::Move(glyim_mir::Place::new(
                                initial_iter_local,
                            ))],
                            destination: glyim_mir::Place::new(output_local),
                            target: Some(after_call),
                            cleanup: None,
                        },
                        iterable.span,
                    );
                    self.current_block = Some(after_call);
                    (info.output_ty, output_local)
                } else {
                    (iter_ty, initial_iter_local)
                };
                // ARRAY-ITERATION PRE-HEADER INIT: for `for x in [a,b,c]`
                // the lowering uses a direct index loop instead of the
                // `Iterator` trait (typeck sets `next = None` for arrays,
                // and `item_ty = T` — the by-value element type). The
                // idx/len init MUST live in the current block (the
                // pre-header `bb0`) rather than in the loop header:
                // emitting it in `header_bb` reset `idx = 0` on every
                // iteration, so `idx < len` stayed true forever (an
                // infinite loop). Hoist the allocation + init up here so
                // it runs exactly once, before the loop.
                // Use the index-based array path only when typeck did not
                // resolve a trait-based `next` — real compiles of
                // `for x in [a,b,c]` set `next = None`, while tests that
                // explicitly exercise the trait path supply a `next` and
                // must keep it.
                // Take the index-based array path only when *neither*
                // typeck (`next`) nor the lowering-context fallback
                // (`iterator_next_fn`) supplied an `Iterator::next`. The
                // fallback is what a mocked test context supplies; real
                // array iterables arrive here with both absent because
                // typeck deliberately routes them around the trait.
                // SLICE-ITER: also route `for x in <&[T]>` (from
                // `v.as_slice()` or `a.iter()`) through the index-walk,
                // NOT the trait path. The trait path calls
                // `impl Iterator for &[T]`'s `next`, whose body returns
                // `&T` into its own transient frame — the interpreter
                // cannot model that (KNOWN_GAPS §7, "deref of Ref from
                // dead frame"). Index-walk keeps everything in the
                // caller's frame. `s[i]` on `&[i32]` already works in
                // both backends (verified: LLVM exit 20 for `s[1]`).
                //
                // The item type differs per iterable shape: for a bare
                // array `[T; N]`, `pat.ty = T`; for `&[T]`, `pat.ty = &T`
                // (mirroring `impl Iterator for &[T]`'s `Item = &T`). The
                // wrapper is a `Ref` around the raw element value below.
                // For `for x in &v` where `v: Vec<T>`, materialize `&[T]`
                // via the `as_slice` builtin (FnDefId 9162, exactly what
                // `v.as_slice()` / `(&v).as_slice()` resolve to; the
                // codegen path handles both pointer- and struct-valued
                // receivers). This lets the slice index-walk below apply
                // unchanged, giving both backends the same proven path
                // (no trait-path layout mismatch, no interp "dead frame"
                // reference). Item type is `&T` (already set by typeck
                // through `ref_vec_elem`); the Ref-wrap below builds it
                // from the raw `T` at each index.
                let (iter_ty, iter_local) = match self.ctx.ty_ctx().ty_kind(iter_ty) {
                    TyKind::Ref(_, inner, glyim_core::primitives::Mutability::Not) => {
                        match self.ctx.ty_ctx().ty_kind(*inner) {
                            TyKind::Adt(id, substs) if id.to_raw() == 1020 => {
                                let elem_t = self
                                    .ctx
                                    .ty_ctx()
                                    .substitution_args(*substs)
                                    .first()
                                    .and_then(|a| match a {
                                        glyim_type::GenericArg::Ty(t) => Some(*t),
                                        _ => None,
                                    });
                                match elem_t {
                                    Some(elem_t) => {
                                        let slice_ty = self
                                            .ctx
                                            .ty_ctx()
                                            .mk_ty(TyKind::Slice(elem_t));
                                        let ref_slice_ty = self.ctx.ty_ctx().mk_ref(
                                            glyim_type::Region::Erased,
                                            slice_ty,
                                            Mutability::Not,
                                        );
                                        let slice_local = self.alloc_local(
                                            ref_slice_ty,
                                            Mutability::Not,
                                            iterable.span,
                                        );
                                        self.push_stmt(
                                            StatementKind::StorageLive(slice_local),
                                            iterable.span,
                                        );
                                        // 9144 = `<Vec<T> as _>::as_slice` —
                                        // verified via MIR: `let s =
                                        // (&v).as_slice()` emits `Fn(9144)`
                                        // (this is NOT 9162, which is
                                        // `to_vec`; a first pass used 9162
                                        // and the synthesized call hit an
                                        // undefined `__glyim_fn_9162` at
                                        // link time). Both backends already
                                        // handle `as_slice` (Vec receiver,
                                        // reads word 0 = ptr, word 2 = len,
                                        // returns a `{ptr, len}` fat
                                        // pointer).
                                        let as_slice_fn =
                                            glyim_core::def_id::FnDefId::from_raw(9144);
                                        let substs = self.ctx.ty_ctx().intern_substitution(
                                            vec![glyim_type::GenericArg::Ty(elem_t)],
                                        );
                                        let fn_ty = self
                                            .ctx
                                            .ty_ctx()
                                            .mk_ty(TyKind::FnDef(as_slice_fn, substs));
                                        let after_call = self.new_block();
                                        self.terminate(
                                            glyim_mir::TerminatorKind::Call {
                                                func: glyim_mir::Operand::Constant(
                                                    glyim_mir::MirConst {
                                                        kind: glyim_mir::MirConstKind::Fn(
                                                            as_slice_fn,
                                                            substs,
                                                        ),
                                                        ty: fn_ty,
                                                        span: iterable.span,
                                                    },
                                                ),
                                                args: vec![glyim_mir::Operand::Copy(
                                                    glyim_mir::Place::new(iter_local),
                                                )],
                                                destination: glyim_mir::Place::new(slice_local),
                                                target: Some(after_call),
                                                cleanup: None,
                                            },
                                            iterable.span,
                                        );
                                        self.current_block = Some(after_call);
                                        (ref_slice_ty, slice_local)
                                    }
                                    None => (iter_ty, iter_local),
                                }
                            }
                            _ => (iter_ty, iter_local),
                        }
                    }
                    _ => (iter_ty, iter_local),
                };
                let iter_is_ref_slice = matches!(
                    self.ctx.ty_ctx().ty_kind(iter_ty),
                    TyKind::Ref(_, inner, _)
                        if matches!(self.ctx.ty_ctx().ty_kind(*inner), TyKind::Slice(_))
                );
                let iter_is_bare_slice =
                    matches!(self.ctx.ty_ctx().ty_kind(iter_ty), TyKind::Slice(_));
                let take_array_path = (matches!(
                    self.ctx.ty_ctx().ty_kind(iter_ty),
                    TyKind::Array(_, _)
                ) && next.is_none()
                    && self.ctx.iterator_next_fn(iter_ty, elem_ty).is_none())
                    || iter_is_ref_slice
                    || iter_is_bare_slice;
                let array_loop_state: Option<(glyim_mir::LocalIdx, glyim_mir::LocalIdx)> =
                    if take_array_path {
                        let usize_ty = Ty::USIZE;
                        let idx_local = self.alloc_local(usize_ty, Mutability::Mut, expr.span);
                        self.push_stmt(StatementKind::StorageLive(idx_local), expr.span);
                        self.push_stmt(
                            StatementKind::Assign(
                                Place::new(idx_local),
                                Rvalue::Use(Operand::Constant(MirConst {
                                    kind: MirConstKind::Uint(0),
                                    ty: usize_ty,
                                    span: expr.span,
                                })),
                            ),
                            expr.span,
                        );
                        let len_local = self.alloc_local(usize_ty, Mutability::Not, expr.span);
                        self.push_stmt(StatementKind::StorageLive(len_local), expr.span);
                        // `Rvalue::Len` expects an Array/Slice/str place,
                        // not a reference. For a `&[T]` iterable, the
                        // local holds a fat pointer — Deref it first, the
                        // same way `s[i]` does. `Len` on the resulting
                        // `[T]` place reads field 1 of the fat pointer
                        // correctly (codegen's `pointee_is_fat` path).
                        let len_place = if iter_is_ref_slice {
                            let mut p = Place::new(iter_local);
                            p.projection = Box::new([glyim_mir::ProjectionElem::Deref]);
                            p
                        } else {
                            Place::new(iter_local)
                        };
                        self.push_stmt(
                            StatementKind::Assign(
                                Place::new(len_local),
                                Rvalue::Len(len_place),
                            ),
                            expr.span,
                        );
                        Some((idx_local, len_local))
                    } else {
                        None
                    };
                let header_bb = self.new_block();
                let exit_bb = self.new_block();
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: header_bb },
                    expr.span,
                );
                let break_place =
                    self.alloc_local(Ty::UNIT, glyim_core::primitives::Mutability::Mut, expr.span);
                self.loop_stack.push(LoopInfo {
                    continue_bb: header_bb,
                    break_bb: exit_bb,
                    break_place,
                });
                if let Some((idx_local, len_local)) = array_loop_state {
                    let usize_ty = Ty::USIZE;
                    let bool_ty = self.ctx.ty_ctx().bool_ty();
                    self.current_block = Some(header_bb);
                    let cond_local = self.alloc_local(bool_ty, Mutability::Not, expr.span);
                    self.push_stmt(StatementKind::StorageLive(cond_local), expr.span);
                    self.push_stmt(
                        StatementKind::Assign(
                            Place::new(cond_local),
                            Rvalue::BinaryOp(
                                BinOp::Lt,
                                Box::new((
                                    Operand::Copy(Place::new(idx_local)),
                                    Operand::Copy(Place::new(len_local)),
                                )),
                            ),
                        ),
                        expr.span,
                    );
                    let body_bb = self.new_block();
                    self.terminate(
                        TerminatorKind::SwitchInt {
                            discr: Operand::Copy(Place::new(cond_local)),
                            switch_ty: bool_ty,
                            targets: SwitchTargets::if_switch(body_bb, exit_bb),
                        },
                        expr.span,
                    );
                    self.current_block = Some(body_bb);
                    // For a `&[T]` iterable, the base local holds a fat
                    // pointer (`{ptr, len}`). Indexing needs a `Deref`
                    // first (to reach the `[T]` pointee), matching what
                    // typeck emits for `s[i]` — `[Deref, Index(i)]`. A
                    // bare `[T]` (already a slice, not a ref) indexes
                    // directly, matching the array shape.
                    let elem_value_place = {
                        let mut base = Place::new(iter_local);
                        if iter_is_ref_slice {
                            base.projection = Box::new([
                                glyim_mir::ProjectionElem::Deref,
                                glyim_mir::ProjectionElem::Index(idx_local),
                            ]);
                        } else {
                            base.projection = Box::new([
                                glyim_mir::ProjectionElem::Index(idx_local),
                            ]);
                        }
                        base
                    };
                    // Raw element type: `iter_local[idx]` yields the bare
                    // element. For a slice iterable, `pat.ty` is `&T`, but
                    // `iter_local[idx]` is `T` — so we read into an `T`
                    // temp then Ref-wrap to build `&T`. For an array, the
                    // two coincide.
                    let raw_elem_ty = if iter_is_ref_slice {
                        match self.ctx.ty_ctx().ty_kind(iter_ty) {
                            TyKind::Ref(_, inner, _) => match self.ctx.ty_ctx().ty_kind(*inner) {
                                TyKind::Slice(e) => *e,
                                _ => elem_ty,
                            },
                            _ => elem_ty,
                        }
                    } else if iter_is_bare_slice {
                        match self.ctx.ty_ctx().ty_kind(iter_ty) {
                            TyKind::Slice(e) => *e,
                            _ => elem_ty,
                        }
                    } else {
                        elem_ty
                    };
                    let raw_elem_local =
                        self.alloc_local(raw_elem_ty, Mutability::Not, expr.span);
                    self.push_stmt(StatementKind::StorageLive(raw_elem_local), expr.span);
                    self.push_stmt(
                        StatementKind::Assign(
                            Place::new(raw_elem_local),
                            Rvalue::Use(Operand::Copy(elem_value_place)),
                        ),
                        expr.span,
                    );
                    // For slices, wrap `&raw_elem_local` to get the `&T`
                    // item `pat` expects. The ref points into a
                    // function-scope temp, valid for the body's duration —
                    // matching the observable semantics of `impl Iterator
                    // for &[T]`'s transient-ref return, without the frame
                    // lifetime problem.
                    let item_place = if iter_is_ref_slice || iter_is_bare_slice {
                        let ref_local = self.alloc_local(elem_ty, Mutability::Not, expr.span);
                        self.push_stmt(StatementKind::StorageLive(ref_local), expr.span);
                        self.push_stmt(
                            StatementKind::Assign(
                                Place::new(ref_local),
                                Rvalue::Ref(
                                    Place::new(raw_elem_local),
                                    glyim_mir::BorrowKind::Shared,
                                ),
                            ),
                            expr.span,
                        );
                        Place::new(ref_local)
                    } else {
                        Place::new(raw_elem_local)
                    };
                    self.bind_pattern(pat, Some(item_place.clone()), Some(item_place), expr.span);
                    let _ = self.lower_expr_to_rvalue(body);
                    if self.current_block.is_some() {
                        self.push_stmt(
                            StatementKind::Assign(
                                Place::new(idx_local),
                                Rvalue::BinaryOp(
                                    BinOp::Add,
                                    Box::new((
                                        Operand::Copy(Place::new(idx_local)),
                                        Operand::Constant(MirConst {
                                            kind: MirConstKind::Uint(1),
                                            ty: usize_ty,
                                            span: expr.span,
                                        }),
                                    )),
                                ),
                            ),
                            expr.span,
                        );
                        self.terminate(TerminatorKind::Goto { target: header_bb }, body.span);
                    }
                    self.loop_stack.pop();
                    self.current_block = Some(exit_bb);
                    return glyim_mir::Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Unit,
                        ty: Ty::UNIT,
                        span: expr.span,
                    }));
                }

                // Get the iterator_next info. Prefer the resolved `next` method
                // threaded from typeck through the THIR `For` node (production
                // path, Phase 1 GLYIM_DESTUB_PLAN); fall back to the lowering
                // context's own lookup (tests / genuinely-unresolvable iterators).
                let iter_info = next
                    .as_ref()
                    .map(|n| crate::lower::IteratorNextInfo {
                        fn_def_id: n.fn_def_id,
                        fn_substs: n.fn_substs,
                        fn_ty: n.fn_ty,
                        option_ty: n.option_ty,
                        discr_ty: n.discr_ty,
                        ref_iter_ty: n.ref_iter_ty,
                    })
                    .or_else(|| self.ctx.iterator_next_fn(iter_ty, elem_ty));
                match iter_info {
                    Some(iter_info) => {
                        self.current_block = Some(header_bb);
                        let ref_iter_local = self.alloc_local(
                            iter_info.ref_iter_ty,
                            glyim_core::primitives::Mutability::Mut,
                            iterable.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(ref_iter_local),
                            iterable.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(ref_iter_local),
                                glyim_mir::Rvalue::Ref(
                                    glyim_mir::Place::new(iter_local),
                                    glyim_mir::BorrowKind::Mut {
                                        // T121-PATCHED [BCK-1]: see above.
                                        allow_two_phase_borrow: self.lowering_call_arg,
                                    },
                                ),
                            ),
                            iterable.span,
                        );
                        let next_fn_const = glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Fn(
                                iter_info.fn_def_id,
                                iter_info.fn_substs,
                            ),
                            ty: iter_info.fn_ty,
                            span: expr.span,
                        };
                        let next_fn_op = glyim_mir::Operand::Constant(next_fn_const);
                        let ref_iter_op =
                            glyim_mir::Operand::Copy(glyim_mir::Place::new(ref_iter_local));
                        let option_local = self.alloc_local(
                            iter_info.option_ty,
                            glyim_core::primitives::Mutability::Mut,
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(option_local),
                            expr.span,
                        );
                        let after_call_bb = self.new_block();
                        self.terminate(
                            glyim_mir::TerminatorKind::Call {
                                func: next_fn_op,
                                args: vec![ref_iter_op],
                                destination: glyim_mir::Place::new(option_local),
                                target: Some(after_call_bb),
                                cleanup: None,
                            },
                            expr.span,
                        );
                        self.current_block = Some(after_call_bb);
                        // `next()` yields an `Option<Item>` *aggregate*.
                        // `SwitchInt` must switch on its discriminant, not the
                        // whole struct; compute `Rvalue::Discriminant` into a
                        // local of the declared discriminant type. (Switching
                        // on the aggregate made codegen load a struct where an
                        // `IntValue` was expected.)
                        let discr_local = self.alloc_local(
                            iter_info.discr_ty,
                            glyim_core::primitives::Mutability::Not,
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(discr_local),
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(discr_local),
                                glyim_mir::Rvalue::Discriminant(glyim_mir::Place::new(option_local)),
                            ),
                            expr.span,
                        );
                        let discr_op =
                            glyim_mir::Operand::Copy(glyim_mir::Place::new(discr_local));
                        let some_bb = self.new_block();
                        let none_bb = exit_bb;
                        let switch_targets =
                            glyim_mir::SwitchTargets::new(Box::new([(1, some_bb)]), none_bb);
                        self.terminate(
                            glyim_mir::TerminatorKind::SwitchInt {
                                discr: discr_op,
                                switch_ty: iter_info.discr_ty,
                                targets: switch_targets,
                            },
                            expr.span,
                        );
                        self.current_block = Some(some_bb);
                        let payload_place = {
                            let mut proj = vec![glyim_mir::ProjectionElem::Downcast(
                                glyim_mir::VariantIdx::from_raw(1),
                            )];
                            proj.push(glyim_mir::ProjectionElem::Field(FieldIdx::from_raw(0)));
                            glyim_mir::Place {
                                local: option_local,
                                projection: proj.into_boxed_slice(),
                            }
                        };
                        let payload_local = self.alloc_local(
                            elem_ty,
                            glyim_core::primitives::Mutability::Not,
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(payload_local),
                            expr.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(payload_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(payload_place)),
                            ),
                            expr.span,
                        );
                        self.bind_pattern(
                            pat,
                            Some(glyim_mir::Place::new(payload_local)),
                            Some(glyim_mir::Place::new(payload_local)),
                            expr.span,
                        );
                        let _ = self.lower_expr_to_rvalue(body);
                        if self.current_block.is_some() {
                            self.terminate(
                                glyim_mir::TerminatorKind::Goto { target: header_bb },
                                body.span,
                            );
                        }
                        self.loop_stack.pop();
                        self.current_block = Some(exit_bb);
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Unit,
                            ty: Ty::UNIT,
                            span: expr.span,
                        }))
                    }
                    None => {
                        // Simplified fallback: execute the body once and break.
                        // This is only used in tests where iterator protocol is not available.
                        self.current_block = Some(header_bb);
                        // Bind the pattern to a dummy value (the iterable itself).
                        // We'll just execute the body without binding the pattern.
                        let _ = self.lower_expr_to_rvalue(body);
                        self.loop_stack.pop();
                        self.terminate(
                            glyim_mir::TerminatorKind::Goto { target: exit_bb },
                            expr.span,
                        );
                        self.current_block = Some(exit_bb);
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Unit,
                            ty: Ty::UNIT,
                            span: expr.span,
                        }))
                    }
                }
            }
            thir::ExprKind::Field {
                receiver,
                field,
                ty: _field_ty,
            } => {
                let base_place = self.lower_expr_to_place(receiver);
                // Auto-deref the receiver for field access (mirrors Rust):
                // `(&mut Counter).field` needs a `Deref` projection before the
                // `Field` projection can be applied to the pointee.
                //
                // Decision is based on the *MIR base-local's declared type*
                // (`locals[base.local].ty`), NOT the THIR `receiver.ty`. THIR
                // typeck has already stripped the `&mut` from a method `self`,
                // so `receiver.ty` would report `Counter` and skip the Deref —
                // but the MIR local still holds `&mut Counter`, and the
                // interpreter requires a `Deref` step to reach the pointee.
                let base_ty = self.locals[base_place.local].ty;
                // Only peel a reference at the *root* of the place. A by-ref
                // capture (`VarRef` to a `&T` local) already produced a `Deref`
                // projection; adding another would double-deref.
                let base_place = if base_place.projection.is_empty()
                    && matches!(self.ctx.ty_ctx().ty_kind(base_ty), TyKind::Ref(..))
                {
                    self.place_with_projection(base_place, ProjectionElem::Deref)
                } else {
                    base_place
                };
                let field_idx = self.resolve_field_index(receiver.ty, *field, expr.span);
                let field_idx = match field_idx {
                    Some(idx) => idx,
                    None => {
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Error,
                                ty: *_field_ty,
                                span: expr.span,
                            },
                        ));
                    }
                };
                // Capture the parent local before `base_place` is moved into
                // `place_with_projection` — it's needed by register_partial_move.
                let parent_local = base_place.local;
                let place =
                    self.place_with_projection(base_place, ProjectionElem::Field(field_idx));

                // Phase 4 (GLYIM_DESTUB_PLAN): a non-Copy field read here is a
                // *partial move* out of its parent (this arm produces an rvalue
                // operand, not a borrow — `&x.field` goes through ExprKind::Ref).
                // Register a drop-flag for the whole parent local so
                // `elaborate_scope_drops` guards its scope-exit Drop, and clear
                // the flag at the move site. Copy fields are read by Copy.
                let field_is_copy = self.ctx.ty_ctx().is_copy(*_field_ty);
                let operand = if field_is_copy {
                    glyim_mir::Operand::Copy(place)
                } else {
                    self.register_partial_move(parent_local, expr.span);
                    glyim_mir::Operand::Move(place)
                };
                glyim_mir::Rvalue::Use(operand)
            }
            thir::ExprKind::Index { base, index } => {
                // Check if the index is a Range expression.
                if let thir::ExprKind::Range {
                    start,
                    end,
                    inclusive,
                } = &index.kind
                {
                    let base_place = self.lower_expr_to_place(base);
                    self.lower_dynamic_range_slice(
                        base_place,
                        start.as_ref().map(|e| e.as_ref()),
                        end.as_ref().map(|e| e.as_ref()),
                        *inclusive,
                        expr.ty,
                        expr.span,
                    )
                } else {
                    // Regular indexing (single element). Delegate the whole
                    // `base[index]` to `lower_expr_to_place`, whose `Index`
                    // arm auto-derefs a `&[T]` / `&mut [T]` / `&Vec<T>` base
                    // before applying the projection. This arm previously
                    // rebuilt the projection from just the *base*
                    // (`lower_expr_to_place(base)` then
                    // `ProjectionElem::Index`), which skipped that auto-deref:
                    // `bytes[0]` where `bytes: &[u8]` lowered to
                    // `Place{[Index]}` on a `Ref`-typed local, and codegen
                    // ICEd with `Index projection on non-array/slice type`
                    // (the current type was `Ref(_, [u8])`, not a slice).
                    let place = self.lower_expr_to_place(expr);
                    glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(place))
                }
            }
            thir::ExprKind::Cast { expr: inner } => {
                let operand = self.lower_expr_to_operand(inner);
                let inner_ty = inner.ty;
                let target_ty = expr.ty;
                // `IntToInt` covers every integer-to-integer cast, including
                // mixed signedness (`Int -> Uint`, `Uint -> Int`) and
                // unsigned-to-unsigned (`Uint -> Uint`). Previously those
                // three shapes fell into the `_ => PtrToPtr` fallback, which
                // is a *pointer* cast kind — the LLVM backend then rejected
                // `Uint(Usize) -> Uint(U64)` with `[X0000] PtrToPtr cast on
                // non-pointer`, blocking `--emit=exec` for every program whose
                // stdlib touches `usize as u64` (`io.g`'s `total += n as u64`).
                let cast_kind =
                    match (
                        self.ctx.ty_ctx().ty_kind(inner_ty),
                        self.ctx.ty_ctx().ty_kind(target_ty),
                    ) {
                        (TyKind::Int(_), TyKind::Int(_))
                        | (TyKind::Uint(_), TyKind::Uint(_))
                        | (TyKind::Int(_), TyKind::Uint(_))
                        | (TyKind::Uint(_), TyKind::Int(_)) => CastKind::IntToInt,
                        (TyKind::Float(_), TyKind::Int(_))
                        | (TyKind::Float(_), TyKind::Uint(_)) => CastKind::FloatToInt,
                        (TyKind::Int(_), TyKind::Float(_))
                        | (TyKind::Uint(_), TyKind::Float(_)) => CastKind::IntToFloat,
                        (TyKind::Float(_), TyKind::Float(_)) => CastKind::FloatToFloat,
                        (TyKind::Ref(..), TyKind::RawPtr(..))
                        | (TyKind::RawPtr(..), TyKind::RawPtr(..))
                        | (TyKind::RawPtr(..), TyKind::Ref(..)) => CastKind::PtrToPtr,
                        (TyKind::FnDef(..) | TyKind::FnPtr(..), TyKind::RawPtr(..)) => {
                            CastKind::FnPtrToPtr
                        }
                        _ => {
                            // Unknown pair: fall back to a plain `Use` instead of a
                            // pointer cast that the LLVM backend will reject. This
                            // is a conservative no-op that keeps `--emit=obj` /
                            // `--emit=exec` resilient to casts the classifier does
                            // not yet cover, at the cost of skipping the cast
                            // semantics for that (currently unreachable) pair.
                            return glyim_mir::Rvalue::Use(operand);
                        }
                    };
                glyim_mir::Rvalue::Cast(cast_kind, operand, target_ty)
            }
            thir::ExprKind::Tuple(elements) => {
                let mut mir_operands = Vec::new();
                for op_expr in elements {
                    mir_operands.push(self.lower_expr_to_operand(op_expr));
                }
                glyim_mir::Rvalue::Aggregate(glyim_mir::AggregateKind::Tuple, mir_operands)
            }
            thir::ExprKind::Array(elements) => {
                let elem_ty = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                    TyKind::Slice(inner) | TyKind::Array(inner, _) => *inner,
                    _ => Ty::ERROR,
                };
                let mut mir_operands = Vec::new();
                for op_expr in elements {
                    mir_operands.push(self.lower_expr_to_operand(op_expr));
                }
                glyim_mir::Rvalue::Aggregate(glyim_mir::AggregateKind::Array(elem_ty), mir_operands)
            }
            thir::ExprKind::Struct {
                adt_id,
                variant_idx,
                fields,
                spread,
            } => {
                let substs = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                    TyKind::Adt(_, substs) => substs,
                    _ => {
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Error,
                                ty: expr.ty,
                                span: expr.span,
                            },
                        ));
                    }
                };
                // T024-PATCHED [LOW-2]: resolve each explicit field to its
                // *declaration* index instead of pushing operands in source
                // order. Layout, `Field(i)` projections, and codegen all
                // follow the ADT / variant declaration order, so
                // `P { y: 2, x: 1 }` previously stored 2 in the `x` slot
                // and 1 in the `y` slot (silent field-value swap).
                //
                // SPREAD FIX: struct-update syntax (`P { x: 10, ..p }`)
                // was previously lowered as if the spread were absent — the
                // `..p` source was type-checked, and its `Box<Expr>` stored
                // on the THIR `spread` slot, but `lower_rvalue`'s arm
                // discarded it (`spread: _`). The result was an aggregate
                // with only the *explicit* fields; every field that was
                // supposed to be copied from the base was an `Error`
                // constant. In codegen, the Error const is materialised as
                // a zero-sized placeholder in the wrong field slot, so
                // `q.y` read the `x`-shaped slot and returned `x`'s value
                // (silent wrong-value bug).
                //
                // Fix: when `spread` is present, evaluate it to a temporary
                // local and, for each declaration-order field that the
                // source *didn't* override, project that field out of the
                // temporary into the aggregate. This is what `..p` desugars
                // to in rustc.
                let variant = glyim_mir::VariantIdx::from_raw(*variant_idx);
                let mut resolved: Vec<(usize, glyim_mir::Operand)> = fields
                    .iter()
                    .enumerate()
                    .map(|(src_idx, (name, field_expr))| {
                        let operand = self.lower_expr_to_operand(field_expr);
                        let idx = self
                            .ctx
                            .field_index_by_name(*adt_id, *variant_idx, *name)
                            .map(|fidx| fidx.index())
                            .unwrap_or(src_idx);
                        (idx, operand)
                    })
                    .collect();
                resolved.sort_by_key(|(i, _)| *i);
                let explicit_max = resolved.iter().map(|(i, _)| i + 1).max().unwrap_or(0);
                // If a spread is present, materialise it *before* the
                // aggregate so its field projections can be read in the
                // same Rvalue construction. `spread_expr.ty` is already
                // unified to `expr.ty` by type-checking.
                let spread_local: Option<LocalIdx> = if let Some(spread_expr) = spread {
                    let op = self.lower_expr_to_operand(spread_expr);
                    let temp = self.alloc_local(
                        spread_expr.ty,
                        glyim_core::primitives::Mutability::Not,
                        spread_expr.span,
                    );
                    self.push_stmt(
                        glyim_mir::StatementKind::StorageLive(temp),
                        spread_expr.span,
                    );
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(
                            glyim_mir::Place::new(temp),
                            glyim_mir::Rvalue::Use(op),
                        ),
                        spread_expr.span,
                    );
                    Some(temp)
                } else {
                    None
                };
                // Field count: read from the ADT def. `lower`'s `AdtDef`
                // exposes fields only through `variants` (a struct is a
                // single-variant ADT at index 0).
                let declared_field_count = {
                    let def = self.ctx.adt_def(*adt_id);
                    def.variants
                        .get(*variant_idx as usize)
                        .map(|v| v.fields.len())
                        .unwrap_or(0)
                };
                let total = explicit_max.max(declared_field_count);
                let mut mir_operands: Vec<glyim_mir::Operand> = Vec::with_capacity(total);
                let mut resolved_iter = resolved.into_iter().peekable();
                for i in 0..total {
                    // Explicit field wins.
                    if let Some(&(idx, _)) = resolved_iter.peek() {
                        if idx == i {
                            let (_, op) = resolved_iter.next().expect("peeked");
                            mir_operands.push(op);
                            continue;
                        }
                    }
                    // Otherwise, take from the spread (if any).
                    if let Some(sl) = spread_local {
                        let field_place = glyim_mir::Place {
                            local: sl,
                            projection: Box::new([glyim_mir::ProjectionElem::Field(
                                glyim_type::FieldIdx::from_raw(i as u32),
                            )]),
                        };
                        mir_operands.push(glyim_mir::Operand::Copy(field_place));
                        continue;
                    }
                    // Neither explicit nor spread: keep the previous
                    // Error-const fallback (typeck already diagnosed the
                    // missing field).
                    mir_operands.push(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                        kind: glyim_mir::MirConstKind::Error,
                        ty: glyim_type::Ty::ERROR,
                        span: expr.span,
                    }));
                }
                glyim_mir::Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Adt(*adt_id, variant, *substs),
                    mir_operands,
                )
            }
            thir::ExprKind::Return { value } => {
                if let Some(val_expr) = value {
                    let rvalue = self.lower_expr_to_rvalue(val_expr);
                    let ret_place = glyim_mir::Place::new(LocalIdx::from_raw(0));
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(ret_place, rvalue),
                        expr.span,
                    );
                }
                self.terminate(glyim_mir::TerminatorKind::Return, expr.span);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Unit,
                    ty: Ty::NEVER,
                    span: expr.span,
                }))
            }
            thir::ExprKind::Break { value } => {
                // HIR-30: a `break <value>` must write the value into the
                // enclosing loop's result local; the loop expression reads it
                // back. Previously the value was evaluated and discarded.
                if let Some(val_expr) = value {
                    let rv = self.lower_expr_to_rvalue(val_expr);
                    if let Some(info) = self.loop_stack.last() {
                        let place = glyim_mir::Place::new(info.break_place);
                        self.push_stmt(glyim_mir::StatementKind::Assign(place, rv), expr.span);
                    }
                }
                let target_bb = self.loop_stack.last().map(|info| info.break_bb);
                if let Some(target) = target_bb {
                    self.terminate(glyim_mir::TerminatorKind::Goto { target }, expr.span);
                } else {
                    self.diagnostics.push(GlyimDiagnostic::type_error(
                        expr.span,
                        "break outside of loop".to_string(),
                    ));
                    self.terminate(glyim_mir::TerminatorKind::Unreachable, expr.span);
                }
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Unit,
                    ty: Ty::NEVER,
                    span: expr.span,
                }))
            }
            thir::ExprKind::Continue => {
                let target_bb = self.loop_stack.last().map(|info| info.continue_bb);
                if let Some(target) = target_bb {
                    self.terminate(glyim_mir::TerminatorKind::Goto { target }, expr.span);
                } else {
                    self.diagnostics.push(GlyimDiagnostic::type_error(
                        expr.span,
                        "continue outside of loop".to_string(),
                    ));
                    self.terminate(glyim_mir::TerminatorKind::Unreachable, expr.span);
                }
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Unit,
                    ty: Ty::NEVER,
                    span: expr.span,
                }))
            }
            thir::ExprKind::Closure {
                body: _thir_body,
                captures,
                is_move: _,
            } => {
                let (closure_id, closure_substs) = match self.ctx.ty_ctx().ty_kind(expr.ty) {
                    TyKind::Closure(id, substs) => (id, substs),
                    _ => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            expr.span,
                            "closure expression has non-closure type".to_string(),
                        ));
                        return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Error,
                                ty: expr.ty,
                                span: expr.span,
                            },
                        ));
                    }
                };
                // Build the closure's own MIR body so codegen can emit it as
                // `__glyim_fn_{closure_id}`. The captures come first, followed
                // by the closure's own parameters (see `lower_closure`).
                self.lower_closure(
                    _thir_body,
                    captures,
                    *closure_id,
                    *closure_substs,
                    expr.span,
                );
                let mut capture_operands = Vec::with_capacity(captures.len());
                for capture in captures {
                    // HIR-31: `capture.local` is a THIR `LocalVarId`, NOT a MIR
                    // `LocalIdx`. Reinterpreting it via `from_raw` captured
                    // `LocalIdx(0)` (the return place) for the common case where
                    // the variable was bound early. Resolve through
                    // `local_for_var` (which honours `local_var_map`) instead.
                    let capture_local = self.local_for_var(capture.local);
                    let operand = match capture.kind {
                        thir::CaptureKind::ByValue => {
                            glyim_mir::Operand::Move(glyim_mir::Place::new(capture_local))
                        }
                        thir::CaptureKind::ByRef(mutability) => {
                            // HIR-31: the environment field is `&T`/`&mut T`,
                            // NOT a copy of the captured value. Materialize the
                            // reference into a temp local and capture that, so
                            // the closure body aliases the enclosing binding.
                            let borrow = match mutability {
                                glyim_core::primitives::Mutability::Mut => {
                                    glyim_mir::BorrowKind::Mut {
                                        // T121-REVIEWED [BCK-1]: closure
                                        // capture is not a call argument,
                                        // so no two-phase borrow — the
                                        // reference is stored immediately,
                                        // not held for the duration of a
                                        // call.
                                        allow_two_phase_borrow: false,
                                    }
                                }
                                glyim_core::primitives::Mutability::Not => {
                                    glyim_mir::BorrowKind::Shared
                                }
                            };
                            let ref_ty = capture.ty;
                            let ref_local = self.alloc_local(
                                ref_ty,
                                glyim_core::primitives::Mutability::Not,
                                expr.span,
                            );
                            self.push_stmt(
                                glyim_mir::StatementKind::StorageLive(ref_local),
                                expr.span,
                            );
                            self.push_stmt(
                                glyim_mir::StatementKind::Assign(
                                    glyim_mir::Place::new(ref_local),
                                    glyim_mir::Rvalue::Ref(
                                        glyim_mir::Place::new(capture_local),
                                        borrow,
                                    ),
                                ),
                                expr.span,
                            );
                            glyim_mir::Operand::Move(glyim_mir::Place::new(ref_local))
                        }
                    };
                    capture_operands.push(operand);
                }
                // The closure *value* is laid out as `[Fn(def_id), captures...]`
                // (plan §12.1). The interpreter's `resolve_callee` unpacks that
                // leading `Fn` to find the callee and passes the rest as the
                // closure body's leading arguments. Emitting only the captures
                // here made every runtime closure call fail with "indirect call
                // through non-function value".
                let fn_const = glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Fn(
                        glyim_core::def_id::FnDefId::from_raw(closure_id.to_raw()),
                        *closure_substs,
                    ),
                    ty: self.ctx.ty_ctx().error_ty(),
                    span: expr.span,
                };
                let mut operands = Vec::with_capacity(capture_operands.len() + 1);
                operands.push(glyim_mir::Operand::Constant(fn_const));
                operands.extend(capture_operands);
                glyim_mir::Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Closure(*closure_id, *closure_substs),
                    operands,
                )
            }
            thir::ExprKind::Err => {
                self.diagnostics.push(GlyimDiagnostic::new(
                    glyim_diag::ErrorCode {
                        category: glyim_diag::ErrorCategory::Internal,
                        number: 0,
                    },
                    glyim_diag::DiagSeverity::Warning,
                    "Err expression in THIR during lowering".to_string(),
                    glyim_diag::MultiSpan::from_span(expr.span),
                ));
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Error,
                    ty: expr.ty,
                    span: expr.span,
                }))
            }
            thir::ExprKind::Try { expr: inner } => {
                // `expr?` — desugar to:
                //   tmp  = <inner>;                 // Result<T,E> / Option<T>
                //   disc = Discriminant(tmp);
                //   switch disc { succ => ok_bb, else => err_bb }
                //
                //   ok_bb:
                //     dest = tmp.succ_field(0);     // T
                //     goto merge_bb
                //
                //   err_bb:
                //     ret  = <ret_ty>::err_variant(tmp.err_field(0))  // or None
                //     return ret
                //
                //   merge_bb:
                //     -> dest
                //
                // Variant numbering: `Result` (Adt1011) is 0=Ok, 1=Err;
                // `Option` (Adt1010) is 0=None, 1=Some. Success and error
                // variants differ per type, so the `success_variant` is
                // derived from the operand's AdtId below. `None` has no
                // payload, so the err-extract + aggregate-build step is
                // skipped for Option; the returned value is a bare
                // `Option::None` aggregate.
                //
                // Typeck (check_expr.rs:3012) has already unified the
                // operand's error type against the enclosing function's
                // return error type, so no `From::from` coercion is
                // needed here.
                let inner_ty = inner.ty;
                let (is_result, success_variant, err_variant) =
                    match self.ctx.ty_ctx().ty_kind(inner_ty) {
                        TyKind::Adt(id, _) => match id.to_raw() {
                            1011 => (true, 0u32, 1u32), // Result
                            1010 => (false, 1u32, 0u32), // Option
                            _ => (true, 0u32, 1u32),    // defensive; typeck rejects
                        },
                        _ => (true, 0u32, 1u32),
                    };

                // Evaluate the operand into a local.
                let operand_rv = self.lower_expr_to_rvalue(inner);
                let operand_local = self.alloc_local(
                    inner_ty,
                    glyim_core::primitives::Mutability::Mut,
                    inner.span,
                );
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(operand_local),
                        operand_rv,
                    ),
                    inner.span,
                );

                // Discriminant local.
                let u8_ty = self.ctx.ty_ctx().mk_ty(TyKind::Uint(
                    glyim_core::primitives::UintTy::U8,
                ));
                let discr_local = self.alloc_local(
                    u8_ty,
                    glyim_core::primitives::Mutability::Not,
                    expr.span,
                );
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(discr_local),
                        glyim_mir::Rvalue::Discriminant(glyim_mir::Place::new(operand_local)),
                    ),
                    expr.span,
                );

                // Success value slot.
                let dest_local = self.alloc_local(
                    expr.ty,
                    glyim_core::primitives::Mutability::Mut,
                    expr.span,
                );
                let dest_place = glyim_mir::Place::new(dest_local);

                let ok_bb = self.new_block();
                let err_bb = self.new_block();
                let merge_bb = self.new_block();

                self.terminate(
                    glyim_mir::TerminatorKind::SwitchInt {
                        discr: glyim_mir::Operand::Copy(glyim_mir::Place::new(discr_local)),
                        switch_ty: u8_ty,
                        targets: glyim_mir::SwitchTargets::new(
                            Box::new([(success_variant as u128, ok_bb)]),
                            err_bb,
                        ),
                    },
                    expr.span,
                );

                // ok_bb: extract success payload, assign to dest, goto merge.
                self.current_block = Some(ok_bb);
                let succ_payload = glyim_mir::Place {
                    local: operand_local,
                    projection: vec![
                        glyim_mir::ProjectionElem::Downcast(glyim_mir::VariantIdx::from_raw(
                            success_variant,
                        )),
                        glyim_mir::ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0)),
                    ]
                    .into_boxed_slice(),
                };
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        dest_place.clone(),
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(succ_payload)),
                    ),
                    expr.span,
                );
                self.terminate(
                    glyim_mir::TerminatorKind::Goto { target: merge_bb },
                    expr.span,
                );

                // err_bb: build the return_ty's error/None variant and Return.
                self.current_block = Some(err_bb);
                let ret_ty = self.return_ty;
                let (ret_adt_id, ret_substs) = match self.ctx.ty_ctx().ty_kind(ret_ty) {
                    TyKind::Adt(id, substs) => (*id, *substs),
                    _ => {
                        // Defensive: typeck should have guaranteed Result/Option
                        // here. Fall back to the operand's Adt info so the
                        // aggregate has a valid shape (only the error slot is
                        // touched, and typeck unifies error types, so this is
                        // layout-compatible with the real return type).
                        let (id, substs) = match self.ctx.ty_ctx().ty_kind(inner_ty) {
                            TyKind::Adt(id, substs) => (*id, *substs),
                            _ => {
                                self.terminate(glyim_mir::TerminatorKind::Return, expr.span);
                                self.current_block = Some(merge_bb);
                                return glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(
                                    dest_place,
                                ));
                            }
                        };
                        (id, substs)
                    }
                };
                // Err/None payload: Result carries the error value at field 0
                // of variant 1; Option::None carries no payload.
                let agg_args: Vec<glyim_mir::Operand> = if is_result {
                    let err_payload = glyim_mir::Place {
                        local: operand_local,
                        projection: vec![
                            glyim_mir::ProjectionElem::Downcast(glyim_mir::VariantIdx::from_raw(
                                err_variant,
                            )),
                            glyim_mir::ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0)),
                        ]
                        .into_boxed_slice(),
                    };
                    vec![glyim_mir::Operand::Copy(err_payload)]
                } else {
                    vec![]
                };
                let ret_agg = glyim_mir::Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Adt(
                        ret_adt_id,
                        glyim_mir::VariantIdx::from_raw(err_variant),
                        ret_substs,
                    ),
                    agg_args,
                );
                let ret_local = self.alloc_local(
                    ret_ty,
                    glyim_core::primitives::Mutability::Mut,
                    expr.span,
                );
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(glyim_mir::Place::new(ret_local), ret_agg),
                    expr.span,
                );
                // `Return` reads local 0 (the function's return slot). Copy
                // our aggregate into local 0 before returning.
                let local0 = glyim_mir::LocalIdx::from_raw(0);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(local0),
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(
                            glyim_mir::Place::new(ret_local),
                        )),
                    ),
                    expr.span,
                );
                self.terminate(glyim_mir::TerminatorKind::Return, expr.span);

                // merge_bb: fall through with the success value.
                self.current_block = Some(merge_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(dest_place))
            }

            thir::ExprKind::DynamicCall {
                receiver: _,
                trait_def_id,
                method_name,
                args,
                self_ty,
            } => {
                // Generic-bound method call (`f.poll()` where `f: F: Trait`).
                // The concrete impl is unknown until monomorphization, so we
                // carry the trait + method identity in a `VirtualMethod`
                // constant. Monomorphization rewrites this to a direct
                // `Call` of the resolved `FnDefId`; the MIR interpreter resolves
                // it on the fly against the (concrete) receiver type.
                //
                // `args` already has the receiver as its first element
                // (typeck prepends it), so we lower them verbatim.
                let resolved_self_ty = self_ty.or_else(|| args.first().map(|a| a.ty));
                let func_const = glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::VirtualMethod {
                        trait_def_id: *trait_def_id,
                        method_name: *method_name,
                        self_ty: resolved_self_ty,
                    },
                    ty: expr.ty,
                    span: expr.span,
                };
                let mut mir_args = Vec::with_capacity(args.len());
                for arg in args {
                    mir_args.push(self.lower_expr_to_operand(arg));
                }

                let dest_local =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                let dest_place = glyim_mir::Place::new(dest_local);
                let next_bb = self.new_block();
                self.terminate(
                    glyim_mir::TerminatorKind::Call {
                        func: glyim_mir::Operand::Constant(func_const),
                        args: mir_args,
                        destination: dest_place.clone(),
                        target: Some(next_bb),
                        cleanup: None,
                    },
                    expr.span,
                );
                self.current_block = Some(next_bb);
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(dest_place))
            }

            // `TraitMethodRef` is always resolved to a concrete `FnRef` +
            // `Call` at the typeck call site (static dispatch), so it never
            // reaches MIR lowering. Kept for exhaustiveness.
            thir::ExprKind::TraitMethodRef { .. } => unreachable!(
                "TraitMethodRef should have been resolved to FnRef+Call during type-checking"
            ),
        }
    }

    // ---- Expression → Operand lowering ----
    /// lower_expr_to_operand.
    /// Resolve a THIR `VarRef(LocalVarId)` to the MIR `LocalIdx` allocated
    /// for that binding. The type-checker's `LocalVarId` space is NOT aligned
    /// with the MIR local-index space (temporaries interleave user-variable
    /// allocations), so we consult `local_var_map` (populated as bindings are
    /// lowered). The direct `LocalIdx::from_raw(var_id.to_raw())` fallback
    /// preserves compatibility with hand-built THIR unit tests that allocate
    /// locals 1:1 with `LocalVarId`.
    fn local_for_var(&self, var_id: thir::LocalVarId) -> LocalIdx {
        let hit = self.local_var_map.get(&var_id);

        if let Some(&local) = hit {
            local
        } else {
            LocalIdx::from_raw(var_id.to_raw())
        }
    }
    pub fn lower_expr_to_operand(&mut self, expr: &thir::Expr) -> glyim_mir::Operand {
        match &expr.kind {
            thir::ExprKind::Literal(_) | thir::ExprKind::FnRef(_) => {
                if let glyim_mir::Rvalue::Use(op) = self.lower_expr_to_rvalue(expr) {
                    op
                } else {
                    unreachable!()
                }
            }
            thir::ExprKind::VarRef(var_id) => {
                let local = self.local_for_var(*var_id);
                // HIR-31: a by-ref capture holds `&T`; the body reads `T`.
                let place = if self.byref_capture_vars.contains(var_id) {
                    self.place_with_projection(glyim_mir::Place::new(local), ProjectionElem::Deref)
                } else {
                    glyim_mir::Place::new(local)
                };
                glyim_mir::Operand::Copy(place)
            }
            _ => {
                let rvalue = self.lower_expr_to_rvalue(expr);
                let local =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                let place = glyim_mir::Place::new(local);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(place.clone(), rvalue),
                    expr.span,
                );
                glyim_mir::Operand::Move(place)
            }
        }
    }

    // ---- Expression → Place lowering ----
    /// lower_expr_to_place.
    pub fn lower_expr_to_place(&mut self, expr: &thir::Expr) -> glyim_mir::Place {
        match &expr.kind {
            thir::ExprKind::VarRef(var_id) => {
                let local = self.local_for_var(*var_id);
                // HIR-31: deref a by-ref capture so the place denotes `T`.
                if self.byref_capture_vars.contains(var_id) {
                    self.place_with_projection(glyim_mir::Place::new(local), ProjectionElem::Deref)
                } else {
                    glyim_mir::Place::new(local)
                }
            }
            thir::ExprKind::StaticRef(static_def_id) => {
                // B.1: `&STATIC` / `static mut` write target. Materialize
                // the static's address into a fresh temp local and return
                // `Place{temp, [Deref]}` so the caller writes THROUGH the
                // global, not into a stack temporary. Mirrors the rvalue
                // arm's two-step lowering.
                let const_alias =
                    glyim_core::def_id::ConstDefId::from_raw(static_def_id.to_raw());
                let substs = glyim_type::Substitution::empty();
                let Some(init) = self.ctx.const_value(const_alias, substs) else {
                    tracing::warn!(
                        "static {} could not be const-evaluated (StaticRef place)",
                        static_def_id.to_raw()
                    );
                    return glyim_mir::Place::new(LocalIdx::from_raw(0));
                };
                let is_mut = self.ctx.ty_ctx().static_is_mut(*static_def_id);
                let static_ref_const = glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::StaticRef {
                        def_id: *static_def_id,
                        init: Box::new(init),
                        is_mut,
                    },
                    ty: expr.ty,
                    span: expr.span,
                };
                let ptr_ty = self.ctx.ty_ctx().mk_ty(glyim_type::TyKind::RawPtr(
                    expr.ty,
                    glyim_core::primitives::Mutability::Not,
                ));
                let addr_local = self
                    .alloc_local(ptr_ty, glyim_core::primitives::Mutability::Not, expr.span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(addr_local),
                        glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(static_ref_const)),
                    ),
                    expr.span,
                );
                glyim_mir::Place {
                    local: addr_local,
                    projection: vec![glyim_mir::ProjectionElem::Deref].into_boxed_slice(),
                }
            }
            thir::ExprKind::Field {
                receiver,
                field,
                ty: _field_ty,
            } => {
                let base_place = self.lower_expr_to_place(receiver);
                // Auto-deref the receiver for field access (mirrors Rust):
                // `(&mut Counter).field` needs a `Deref` projection before the
                // `Field` projection can be applied to the pointee.
                //
                // Decision is based on the *MIR base-local's declared type*
                // (`locals[base.local].ty`), NOT the THIR `receiver.ty`. THIR
                // typeck has already stripped the `&mut` from a method `self`,
                // so `receiver.ty` would report `Counter` and skip the Deref —
                // but the MIR local still holds `&mut Counter`, and the
                // interpreter requires a `Deref` step to reach the pointee.
                let base_ty = self.locals[base_place.local].ty;
                // Only peel a reference at the *root* of the place. A by-ref
                // capture (`VarRef` to a `&T` local) already produced a `Deref`
                // projection; adding another would double-deref.
                let base_place = if base_place.projection.is_empty()
                    && matches!(self.ctx.ty_ctx().ty_kind(base_ty), TyKind::Ref(..))
                {
                    self.place_with_projection(base_place, ProjectionElem::Deref)
                } else {
                    base_place
                };
                let field_idx = self.resolve_field_index(receiver.ty, *field, expr.span);
                let field_idx = match field_idx {
                    Some(idx) => idx,
                    None => FieldIdx::from_raw(0),
                };
                self.place_with_projection(base_place, ProjectionElem::Field(field_idx))
            }
            thir::ExprKind::Index { base, index } => {
                let base_place = self.lower_expr_to_place(base);
                // Auto-deref a `&[T]`/`&mut [T]`/`&Vec<T>` receiver before
                // applying the `Index` projection (mirrors Rust indexing,
                // which peels the reference first).
                let base_decl_ty = self.locals[base_place.local].ty;
                let base_place =
                    if matches!(self.ctx.ty_ctx().ty_kind(base_decl_ty), TyKind::Ref(..)) {
                        self.place_with_projection(base_place, ProjectionElem::Deref)
                    } else {
                        base_place
                    };
                let index_local = self.alloc_local(
                    index.ty,
                    glyim_core::primitives::Mutability::Not,
                    index.span,
                );
                let index_rval = self.lower_expr_to_rvalue(index);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(index_local),
                        index_rval,
                    ),
                    index.span,
                );
                self.place_with_projection(
                    base_place,
                    glyim_mir::ProjectionElem::Index(index_local),
                )
            }
            thir::ExprKind::Ref {
                operand,
                mutability: _,
            } => self.lower_expr_to_place(operand),
            // PTR-DEREF-PLACE: `*p` on the LHS of an assignment (`*d = s`) is
            // a *place* — the store must go THROUGH the pointer, not into a
            // temporary. The previous code had no `Unary(Deref)` arm, so `*d`
            // fell into the `_` fallback, which lowers it as an rvalue (a
            // read) into a fresh local; the assignment then wrote to that
            // temp and the raw store silently no-opped. Lower the operand to
            // a place and add a `Deref` projection.
            thir::ExprKind::Unary {
                op: glyim_core::primitives::UnOp::Deref,
                operand,
            } => {
                let base_place = self.lower_expr_to_place(operand);
                self.place_with_projection(base_place, ProjectionElem::Deref)
            }
            _ => {
                let rvalue = self.lower_expr_to_rvalue(expr);
                let local =
                    self.alloc_local(expr.ty, glyim_core::primitives::Mutability::Mut, expr.span);
                let place = glyim_mir::Place::new(local);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(place.clone(), rvalue),
                    expr.span,
                );
                place
            }
        }
    }

    // ---- Pattern binding ----
    /// bind_pattern.
    ///
    /// `copy_place` is the place a by-value binding moves from (usually a
    /// temp copy of the matched value); `orig_place` is the *original*
    /// place a `ref`/`ref mut` binding borrows, so `*v = ..` writes
    /// through to the scrutinee rather than into a copy. When the two are
    /// the same (e.g. a `let` binding) callers pass the same place twice.
    pub fn bind_pattern(
        &mut self,
        pat: &thir::Pattern,
        copy_place: Option<glyim_mir::Place>,
        orig_place: Option<glyim_mir::Place>,
        span: glyim_span::Span,
    ) {
        match &pat.kind {
            thir::PatternKind::Range { .. } => {}
            thir::PatternKind::Binding {
                name,
                var_id,
                mutability,
                binding_ty,
                subpattern,
            } => {
                // A `ref`/`ref mut` binding typed `&T`: store a *borrow* of
                // the ORIGINAL place (not the temp copy) so writes through
                // `*v` reach the scrutinee.
                if let Some(bt) = binding_ty {
                    if let Some(place) = orig_place.clone() {
                        let local = self.alloc_local(*bt, *mutability, span);
                        self.var_map.insert(*name, local);
                        self.local_var_map.insert(*var_id, local);
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(local), span);
                        // Borrow kind from the reference's own mutability
                        // (`&mut T` vs `&T`), not the binding's `mutability`
                        // (`ref mut v` binds a `&mut T` with a non-mut
                        // binding).
                        let borrow = match self.ctx.ty_ctx().ty_kind(*bt) {
                            TyKind::Ref(_, _, glyim_core::primitives::Mutability::Mut) => {
                                glyim_mir::BorrowKind::Mut {
                                    allow_two_phase_borrow: false,
                                }
                            }
                            _ => glyim_mir::BorrowKind::Shared,
                        };
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(local),
                                glyim_mir::Rvalue::Ref(place, borrow),
                            ),
                            span,
                        );
                        if let Some(sub) = subpattern {
                            self.bind_pattern(
                                sub,
                                Some(glyim_mir::Place::new(local)),
                                Some(glyim_mir::Place::new(local)),
                                span,
                            );
                        }
                        return;
                    }
                }
                // Plain binding: prefer the copy place's resolved type when
                // `pat.ty` is `Ty::ERROR` (typeck leaves a variant-field
                // binding error-typed; the enclosing arm typed the copy
                // from the resolved `Downcast+Field` place).
                let local_ty = if pat.ty == Ty::ERROR {
                    match &copy_place {
                        Some(p) => p.ty(self.ctx.ty_ctx(), &self.locals),
                        None => pat.ty,
                    }
                } else {
                    pat.ty
                };
                let local = self.alloc_local(local_ty, *mutability, span);
                self.var_map.insert(*name, local);
                self.local_var_map.insert(*var_id, local);
                self.push_stmt(glyim_mir::StatementKind::StorageLive(local), span);
                // Guard on the *binding's* type, not `pat.ty`: typeck leaves
                // a variant-field binding's `pat.ty` as `Ty::ERROR` (see the
                // `local_ty` computation above), and a flag on an
                // error-typed local is never read.
                if self.ctx.ty_ctx().needs_drop(local_ty) {
                    self.register_drop_flag_init(local, span);
                }
                if let Some(p) = copy_place {
                    // The source temp is being *moved out of*. Clear its drop
                    // flag, or `elaborate_scope_drops` drops it at scope exit
                    // **in addition to** the binding it moved into — a
                    // double-drop. `let _g = G { v: 1 }` lowered to
                    // `temp = G{1}; _g = Move(temp);` and dropped both, which
                    // double-freed any owning field.
                    if self.ctx.ty_ctx().needs_drop(local_ty) {
                        self.register_partial_move(p.local, span);
                    }
                    let rvalue = glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(p));
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(glyim_mir::Place::new(local), rvalue),
                        span,
                    );
                }
                if let Some(sub) = subpattern {
                    self.bind_pattern(
                        sub,
                        Some(glyim_mir::Place::new(local)),
                        Some(glyim_mir::Place::new(local)),
                        span,
                    );
                }
            }
            thir::PatternKind::Wild => {}
            thir::PatternKind::Tuple(fields) => {
                if let (Some(init), Some(orig)) = (copy_place.clone(), orig_place.clone()) {
                    for (idx, field_pat) in fields.iter().enumerate() {
                        let field_proj = ProjectionElem::Field(FieldIdx::from_raw(idx as u32));
                        let copy_field =
                            self.place_with_projection(init.clone(), field_proj.clone());
                        let orig_field =
                            self.place_with_projection(orig.clone(), field_proj);
                        let temp_local = self.alloc_local(
                            field_pat.ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(temp_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(copy_field)),
                            ),
                            span,
                        );
                        self.bind_pattern(
                            field_pat,
                            Some(glyim_mir::Place::new(temp_local)),
                            Some(orig_field),
                            span,
                        );
                    }
                }
            }
            thir::PatternKind::Struct {
                adt_id,
                variant_idx,
                fields,
                rest: _rest,
            } => {
                if let (Some(init), Some(orig)) = (copy_place.clone(), orig_place.clone()) {
                    let init_place = init;
                    // BUG-HUNT FIX: a *tuple-style* enum variant
                    // (`enum E { A(i32,i32) }`) has all fields with an
                    // empty `Name`. `field_index_by_name` therefore
                    // returns the same `Field(0)` for every binding in
                    // `A(x, y)`, so `y` silently aliased `x` (silent
                    // wrong-value bug — `match E::A(3,4) { E::A(x,y) =>
                    // y }` returned 3). The variant's `style` is the
                    // authoritative signal: when it's `Tuple`, use the
                    // source-order position `i` as the field index.
                    let is_tuple_variant = {
                        let def = self.ctx.adt_def(*adt_id);
                        def.variants
                            .get(*variant_idx as usize)
                            .map(|v| v.style == glyim_type::adt_def::VariantStyle::Tuple)
                            .unwrap_or(false)
                    };
                    // For an enum scrutinee, insert a `Downcast` before the
                    // `Field` projection. Without it, `Place::ty()` on the
                    // field place bails with "Field projection on
                    // non-tuple/ADT type", the field local's type collapses to
                    // `Ty::ERROR`, and an arm body like `Ok(n) => written += n`
                    // ends up adding a default `1` instead of `n` (or, worse,
                    // folding to a constant) -- the reason `write_all` never
                    // advanced `written` and looped forever / panicked.
                    let is_enum = self.ctx.adt_def(*adt_id).kind == crate::lower::AdtKind::Enum;
                    let (field_base, orig_base) = if is_enum {
                        let dc = ProjectionElem::Downcast(glyim_mir::VariantIdx::from_raw(*variant_idx));
                        (
                            self.place_with_projection(init_place.clone(), dc.clone()),
                            self.place_with_projection(orig.clone(), dc),
                        )
                    } else {
                        (init_place.clone(), orig.clone())
                    };
                    for (pos, field_pat) in fields.iter().enumerate() {
                        let field_idx: Option<FieldIdx> = if is_tuple_variant {
                            Some(FieldIdx::from_raw(pos as u32))
                        } else {
                            self.ctx
                                .field_index_by_name(*adt_id, *variant_idx, field_pat.field)
                        };
                        let field_idx = match field_idx {
                            Some(idx) => idx,
                            None => continue,
                        };
                        let field_proj = ProjectionElem::Field(field_idx);
                        let field_place =
                            self.place_with_projection(field_base.clone(), field_proj);
                        // Type the field local from the *resolved place*
                        // (`Downcast(variant) + Field(idx)` -> concrete field
                        // type), not from `field_pat.pattern.ty`. Typeck
                        // leaves the latter as `Ty::ERROR` for a field inside
                        // a generic enum-variant pattern (`Ok(n)` on
                        // `Result<usize, E>`), which then propagates: an
                        // `Ok(n) => written += n` arm would build an `Add`
                        // with a `Ty::ERROR` operand and the layout computer
                        // rejects the aggregate. Fall back to
                        // `field_pat.pattern.ty` only if the resolved place
                        // type itself is error.
                        let place_field_ty = field_place.ty(self.ctx.ty_ctx(), &self.locals);
                        let field_ty = if place_field_ty == Ty::ERROR {
                            field_pat.pattern.ty
                        } else {
                            place_field_ty
                        };
                        // The corresponding field on the ORIGINAL place —
                        // a `ref`/`ref mut` sub-pattern borrows this, so
                        // `*v = ..` writes through to the scrutinee rather
                        // than into the temp copy.
                        let orig_field = self.place_with_projection(
                            orig_base.clone(),
                            ProjectionElem::Field(field_idx),
                        );
                        let temp_local = self.alloc_local(
                            field_ty,
                            glyim_core::primitives::Mutability::Not,
                            field_pat.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::StorageLive(temp_local),
                            field_pat.span,
                        );
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(temp_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(field_place)),
                            ),
                            field_pat.span,
                        );
                        self.bind_pattern(
                            &field_pat.pattern,
                            Some(glyim_mir::Place::new(temp_local)),
                            Some(orig_field),
                            field_pat.span,
                        );
                    }
                }
            }
            thir::PatternKind::Or(pats) => {
                if let Some(first_pat) = pats.first() {
                    self.bind_pattern(
                        first_pat,
                        copy_place,
                        orig_place,
                        span,
                    );
                }
            }
            // `Literal` and `ConstBlock` patterns bind no names of their
            // own -- like `Literal`, `ConstBlock` is a *refutable*
            // comparison pattern (`match x { const { A + B } => .. }`),
            // not a binding pattern. Nothing to do here; the actual
            // compile-time evaluation and comparison-value generation for
            // `ConstBlock` happens in `collect_switch_values` below, which
            // is where `PatternKind::Literal` is also turned into a
            // switch-arm value.
            thir::PatternKind::Literal(_) => {}
            thir::PatternKind::ConstBlock(_) => {}
            thir::PatternKind::Error => {}
            thir::PatternKind::Slice {
                prefix,
                slice,
                suffix,
            } => {
                if let (Some(init), Some(orig)) = (copy_place.clone(), orig_place.clone()) {
                    let init_place = init;

                    for (i, sub_pat) in prefix.iter().enumerate() {
                        let proj = ProjectionElem::ConstantIndex {
                            offset: i as u64,
                            min_length: (prefix.len() + suffix.len()) as u64,
                            from_end: false,
                        };
                        let elem_place = self.place_with_projection(init_place.clone(), proj.clone());
                        let orig_elem = self.place_with_projection(orig.clone(), proj);
                        let temp_local = self.alloc_local(
                            sub_pat.ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(temp_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(elem_place)),
                            ),
                            span,
                        );
                        self.bind_pattern(
                            sub_pat,
                            Some(glyim_mir::Place::new(temp_local)),
                            Some(orig_elem),
                            span,
                        );
                    }

                    for (i, sub_pat) in suffix.iter().enumerate() {
                        let proj = ProjectionElem::ConstantIndex {
                            offset: (suffix.len() - i) as u64,
                            min_length: (prefix.len() + suffix.len()) as u64,
                            from_end: true,
                        };
                        let elem_place = self.place_with_projection(init_place.clone(), proj.clone());
                        let orig_elem = self.place_with_projection(orig.clone(), proj);
                        let temp_local = self.alloc_local(
                            sub_pat.ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(temp_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(elem_place)),
                            ),
                            span,
                        );
                        self.bind_pattern(
                            sub_pat,
                            Some(glyim_mir::Place::new(temp_local)),
                            Some(orig_elem),
                            span,
                        );
                    }

                    if let Some(rest_pat) = slice {
                        let proj = ProjectionElem::Subslice {
                            from: prefix.len() as u64,
                            to: suffix.len() as u64,
                            from_end: true,
                        };
                        let rest_place = self.place_with_projection(init_place.clone(), proj.clone());
                        let orig_rest = self.place_with_projection(orig.clone(), proj);
                        let temp_local = self.alloc_local(
                            rest_pat.ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(
                                glyim_mir::Place::new(temp_local),
                                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(rest_place)),
                            ),
                            span,
                        );
                        self.bind_pattern(
                            rest_pat,
                            Some(glyim_mir::Place::new(temp_local)),
                            Some(orig_rest),
                            span,
                        );
                    }
                }
            }
        }
    }

    // ---- Literal lowering ----
    fn lower_literal(
        &self,
        lit: &thir::Literal,
        ty: Ty,
        span: glyim_span::Span,
    ) -> glyim_mir::MirConst {
        match lit {
            thir::Literal::Int(val, _) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Int(*val),
                ty,
                span,
            },
            thir::Literal::Uint(val, _) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Uint(*val),
                ty,
                span,
            },
            thir::Literal::FloatBits(val, _fty) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::FloatBits(*val),
                ty,
                span,
            },
            thir::Literal::Bool(val) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Bool(*val),
                ty,
                span,
            },
            thir::Literal::Char(ch) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Int(*ch as i128),
                ty,
                span,
            },
            thir::Literal::String(name) => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::String(*name),
                ty,
                span,
            },
            thir::Literal::Unit => glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Unit,
                ty,
                span,
            },
        }
    }

    // ---- Match lowering ----
    fn lower_match(
        &mut self,
        scrutinee: &thir::Expr,
        arms: &[thir::MatchArm],
        result_ty: Ty,
        span: glyim_span::Span,
    ) -> glyim_mir::Rvalue {
        let merge_bb = self.new_block();
        let dest_local = self.alloc_local(result_ty, glyim_core::primitives::Mutability::Mut, span);
        let dest_place = glyim_mir::Place::new(dest_local);

        let slice_dispatch = matches!(
            self.ctx.ty_ctx().ty_kind(scrutinee.ty),
            TyKind::Slice(_) | TyKind::Array(_, _)
        ) && arms
            .iter()
            .any(|a| matches!(&a.pat.kind, thir::PatternKind::Slice { .. }));

        // Materialize the whole scrutinee exactly once. Both the per-arm
        // test blocks (which project fields) and `bind_pattern` (which binds
        // the arm patterns' variables) read off this single local; see the
        // long comment that used to live here about not re-lowering the
        // scrutinee (side effects would run twice).
        let scrutinee_place = self.lower_expr_to_place(scrutinee);
        // Auto-deref a reference scrutinee: `match &x { Variant::A(..) => .. }`
        // and, crucially, `match self { .. }` inside an `impl` method (where
        // `self: &Self` / `&mut Self`) both have scrutinee type `&Adt`, but
        // the per-arm pattern tests and `bind_pattern` project `Field(i)`
        // off the *underlying* Adt. Without the deref, `Place::ty()` bails
        // with "Field projection on non-tuple/ADT type" and the pattern
        // tests can't resolve field types. Materialize the dereferenced
        // value into `scrut_local` (typed as the pointee) so every
        // subsequent `Downcast+Field` projection sees a real ADT.
        let (scrut_source, scrut_ty) = match self.ctx.ty_ctx().ty_kind(scrutinee.ty) {
            TyKind::Ref(_, inner, _) => (
                self.place_with_projection(scrutinee_place.clone(), ProjectionElem::Deref),
                *inner,
            ),
            _ => (scrutinee_place.clone(), scrutinee.ty),
        };
        let scrut_local = self.alloc_local(scrut_ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(scrut_local),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(scrut_source.clone())),
            ),
            span,
        );
        let scrut_place = glyim_mir::Place::new(scrut_local);

        // Per-arm blocks: `test_bbs[i]` decides whether arm `i` matches;
        // `arm_bbs[i]` binds the pattern and lowers the body; `otherwise_bb`
        // is the fail-target of the last arm's test.
        let arm_bbs: Vec<BasicBlockIdx> = (0..arms.len()).map(|_| self.new_block()).collect();
        let test_bbs: Vec<BasicBlockIdx> = (0..arms.len()).map(|_| self.new_block()).collect();
        let otherwise_bb = self.new_block();

        if arms.is_empty() {
            self.terminate(
                glyim_mir::TerminatorKind::Goto {
                    target: otherwise_bb,
                },
                span,
            );
        } else {
            self.terminate(
                glyim_mir::TerminatorKind::Goto {
                    target: test_bbs[0],
                },
                span,
            );
        }

        // Emit each arm's test block. Sequential arm testing (HANDOFF §4):
        // a pattern like `Ok(0)` needs *both* `discriminant == Ok` *and*
        // `field0 == 0`, so a discriminant-only switch silently miscompiled
        // every arm that discriminated on a payload literal. Here every arm
        // gets a full pattern test as a boolean, chained in order.
        for (i, arm) in arms.iter().enumerate() {
            self.current_block = Some(test_bbs[i]);
            let next_bb = if i + 1 < arms.len() {
                test_bbs[i + 1]
            } else {
                otherwise_bb
            };
            match self.lower_pattern_test(&arm.pat, &scrut_place, span) {
                Some(bool_local) => {
                    let bool_ty = self.ctx.ty_ctx().bool_ty();
                    self.terminate(
                        glyim_mir::TerminatorKind::SwitchInt {
                            discr: glyim_mir::Operand::Copy(glyim_mir::Place::new(bool_local)),
                            switch_ty: bool_ty,
                            targets: glyim_mir::SwitchTargets::if_switch(arm_bbs[i], next_bb),
                        },
                        span,
                    );
                }
                None => {
                    // Irrefutable pattern: jump straight to the arm body.
                    self.terminate(glyim_mir::TerminatorKind::Goto { target: arm_bbs[i] }, span);
                }
            }
        }

        // Emit each arm's body block.
        for (i, arm) in arms.iter().enumerate() {
            self.current_block = Some(arm_bbs[i]);
            // Bind the arm pattern's variables into MIR locals so that
            // `VarRef`s inside the arm body resolve correctly. Skipped for
            // slice-dispatch matches (as before) where the historical
            // behaviour was to not bind at all.
            if !slice_dispatch {
                self.bind_pattern(
                    &arm.pat,
                    Some(glyim_mir::Place::new(scrut_local)),
                    Some(scrut_source.clone()),
                    arm.pat.span,
                );
            }
            if let Some(guard) = &arm.guard {
                let guard_op = self.lower_expr_to_operand(guard);
                let arm_body_bb = self.new_block();
                let next_bb = if i + 1 < arms.len() {
                    test_bbs[i + 1]
                } else {
                    otherwise_bb
                };
                let guard_targets =
                    glyim_mir::SwitchTargets::new(Box::new([(1, arm_body_bb)]), next_bb);
                self.terminate(
                    glyim_mir::TerminatorKind::SwitchInt {
                        discr: guard_op,
                        switch_ty: guard.ty,
                        targets: guard_targets,
                    },
                    guard.span,
                );
                self.current_block = Some(arm_body_bb);
                self.lower_arm_body(arm, &dest_place, merge_bb);
            } else {
                self.lower_arm_body(arm, &dest_place, merge_bb);
            }
        }

        // `otherwise_bb` is reached only if no arm matched. Terminate it
        // explicitly so every block in the body has a terminator, whether or
        // not it happens to be entered (dead blocks get eliminated later).
        self.current_block = Some(otherwise_bb);
        self.terminate(glyim_mir::TerminatorKind::Unreachable, span);

        self.current_block = Some(merge_bb);
        glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(dest_place))
    }

    /// Compute a boolean local that is `true` iff `pat` matches the value at
    /// `scrut_place`. Returns `None` for an irrefutable pattern (the caller
    /// then jumps straight to the arm body).
    fn lower_pattern_test(
        &mut self,
        pat: &thir::Pattern,
        scrut_place: &glyim_mir::Place,
        span: glyim_span::Span,
    ) -> Option<LocalIdx> {
        match &pat.kind {
            thir::PatternKind::Wild | thir::PatternKind::Error => None,
            thir::PatternKind::Binding { subpattern, .. } => match subpattern {
                None => None,
                Some(sub) => self.lower_pattern_test(sub, scrut_place, span),
            },
            thir::PatternKind::Literal(lit) => {
                // Prefer the projected place's resolved type (the field's
                // real type after Downcast+Field). Fall back to `pat.ty`
                // only when the place type is unresolvable. `pat.ty` is
                // `Ty::ERROR` for a field literal inside an enum-variant
                // pattern (`Ok(0)` on `Result<usize, E>`), which would make
                // the constant error-typed and the comparison always fail.
                let place_ty = scrut_place.ty(self.ctx.ty_ctx(), &self.locals);
                let scrut_ty = if place_ty == Ty::ERROR {
                    pat.ty
                } else {
                    place_ty
                };
                let const_op = self.literal_to_mir_const(lit, scrut_ty, span)?;
                let cmp = glyim_mir::Rvalue::BinaryOp(
                    BinOp::Eq,
                    Box::new((
                        glyim_mir::Operand::Copy(scrut_place.clone()),
                        glyim_mir::Operand::Constant(const_op),
                    )),
                );
                let bool_ty = self.ctx.ty_ctx().bool_ty();
                let out = self.alloc_local(bool_ty, glyim_core::primitives::Mutability::Not, span);
                self.push_stmt(glyim_mir::StatementKind::StorageLive(out), span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(glyim_mir::Place::new(out), cmp),
                    span,
                );
                Some(out)
            }
            thir::PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                // See the note on the `Literal` arm above.
                let place_ty = scrut_place.ty(self.ctx.ty_ctx(), &self.locals);
                let scrut_ty = if place_ty == Ty::ERROR {
                    pat.ty
                } else {
                    place_ty
                };
                let bool_ty = self.ctx.ty_ctx().bool_ty();
                let mut acc: Option<LocalIdx> = None;
                if let Some(s) = start {
                    if let Some(c) = self.literal_to_mir_const(s, scrut_ty, span) {
                        let cmp = glyim_mir::Rvalue::BinaryOp(
                            BinOp::GtEq,
                            Box::new((
                                glyim_mir::Operand::Copy(scrut_place.clone()),
                                glyim_mir::Operand::Constant(c),
                            )),
                        );
                        let l = self.alloc_local(
                            bool_ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(l), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(glyim_mir::Place::new(l), cmp),
                            span,
                        );
                        acc = Some(l);
                    }
                }
                if let Some(e) = end {
                    if let Some(c) = self.literal_to_mir_const(e, scrut_ty, span) {
                        let op = if *inclusive { BinOp::LtEq } else { BinOp::Lt };
                        let cmp = glyim_mir::Rvalue::BinaryOp(
                            op,
                            Box::new((
                                glyim_mir::Operand::Copy(scrut_place.clone()),
                                glyim_mir::Operand::Constant(c),
                            )),
                        );
                        let l = self.alloc_local(
                            bool_ty,
                            glyim_core::primitives::Mutability::Not,
                            span,
                        );
                        self.push_stmt(glyim_mir::StatementKind::StorageLive(l), span);
                        self.push_stmt(
                            glyim_mir::StatementKind::Assign(glyim_mir::Place::new(l), cmp),
                            span,
                        );
                        acc = Some(match acc {
                            None => l,
                            Some(prev) => self.and_bool(prev, l, span),
                        });
                    }
                }
                acc
            }
            thir::PatternKind::Or(subpats) => {
                let mut acc: Option<LocalIdx> = None;
                for sub in subpats {
                    {
                        let l = self.lower_pattern_test(sub, scrut_place, span)?;
                        acc = Some(match acc {
                            None => l,
                            Some(prev) => self.or_bool(prev, l, span),
                        });
                    }
                }
                acc
            }
            thir::PatternKind::Struct {
                adt_id,
                variant_idx,
                fields,
                ..
            } => {
                let is_enum = self.ctx.adt_def(*adt_id).kind == crate::lower::AdtKind::Enum;
                let mut acc: Option<LocalIdx> = None;
                if is_enum {
                    let u8_ty = Ty::U8;
                    let discr_local =
                        self.alloc_local(u8_ty, glyim_core::primitives::Mutability::Not, span);
                    self.push_stmt(glyim_mir::StatementKind::StorageLive(discr_local), span);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(
                            glyim_mir::Place::new(discr_local),
                            glyim_mir::Rvalue::Discriminant(scrut_place.clone()),
                        ),
                        span,
                    );
                    let idx_const = glyim_mir::MirConst {
                        kind: glyim_mir::MirConstKind::Uint(*variant_idx as u128),
                        ty: u8_ty,
                        span,
                    };
                    let cmp = glyim_mir::Rvalue::BinaryOp(
                        BinOp::Eq,
                        Box::new((
                            glyim_mir::Operand::Copy(glyim_mir::Place::new(discr_local)),
                            glyim_mir::Operand::Constant(idx_const),
                        )),
                    );
                    let bool_ty = self.ctx.ty_ctx().bool_ty();
                    let discr_eq =
                        self.alloc_local(bool_ty, glyim_core::primitives::Mutability::Not, span);
                    self.push_stmt(glyim_mir::StatementKind::StorageLive(discr_eq), span);
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(glyim_mir::Place::new(discr_eq), cmp),
                        span,
                    );
                    acc = Some(discr_eq);
                }
                for field_pat in fields {
                    let field_idx =
                        self.ctx
                            .field_index_by_name(*adt_id, *variant_idx, field_pat.field);
                    let Some(field_idx) = field_idx else {
                        continue;
                    };
                    // For an enum scrutinee, insert a `Downcast` before the
                    // `Field` projection so `Place::ty()` can resolve the
                    // variant's field type (without it, `Place::ty()` bails
                    // with "Field projection on non-tuple/ADT type" and the
                    // field test is silently skipped or error-typed).
                    let field_base = if is_enum {
                        self.place_with_projection(
                            scrut_place.clone(),
                            ProjectionElem::Downcast(glyim_mir::VariantIdx::from_raw(*variant_idx)),
                        )
                    } else {
                        scrut_place.clone()
                    };
                    let field_place =
                        self.place_with_projection(field_base, ProjectionElem::Field(field_idx));
                    if let Some(fl) =
                        self.lower_pattern_test(&field_pat.pattern, &field_place, span)
                    {
                        acc = Some(match acc {
                            None => fl,
                            Some(prev) => self.and_bool(prev, fl, span),
                        });
                    }
                }
                acc
            }
            thir::PatternKind::Tuple(fields) => {
                let mut acc: Option<LocalIdx> = None;
                for (i, field_pat) in fields.iter().enumerate() {
                    let field_place = self.place_with_projection(
                        scrut_place.clone(),
                        ProjectionElem::Field(FieldIdx::from_raw(i as u32)),
                    );
                    if let Some(fl) = self.lower_pattern_test(field_pat, &field_place, span) {
                        acc = Some(match acc {
                            None => fl,
                            Some(prev) => self.and_bool(prev, fl, span),
                        });
                    }
                }
                acc
            }
            thir::PatternKind::Slice { prefix, suffix, .. } => {
                let usize_ty = Ty::USIZE;
                let len_local =
                    self.alloc_local(usize_ty, glyim_core::primitives::Mutability::Not, span);
                self.push_stmt(glyim_mir::StatementKind::StorageLive(len_local), span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(
                        glyim_mir::Place::new(len_local),
                        glyim_mir::Rvalue::Len(scrut_place.clone()),
                    ),
                    span,
                );
                let expected = (prefix.len() + suffix.len()) as u128;
                let expected_const = glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Uint(expected),
                    ty: usize_ty,
                    span,
                };
                let cmp = glyim_mir::Rvalue::BinaryOp(
                    BinOp::Eq,
                    Box::new((
                        glyim_mir::Operand::Copy(glyim_mir::Place::new(len_local)),
                        glyim_mir::Operand::Constant(expected_const),
                    )),
                );
                let bool_ty = self.ctx.ty_ctx().bool_ty();
                let out = self.alloc_local(bool_ty, glyim_core::primitives::Mutability::Not, span);
                self.push_stmt(glyim_mir::StatementKind::StorageLive(out), span);
                self.push_stmt(
                    glyim_mir::StatementKind::Assign(glyim_mir::Place::new(out), cmp),
                    span,
                );
                Some(out)
            }
            thir::PatternKind::ConstBlock(_) => {
                // Const-block patterns previously had no field tests
                // either; conservatively treat them as irrefutable so the
                // arm body executes (matching historical behaviour).
                None
            }
        }
    }

    /// `dest = lhs & rhs` on two bool locals, returning `dest`.
    fn and_bool(&mut self, lhs: LocalIdx, rhs: LocalIdx, span: glyim_span::Span) -> LocalIdx {
        let bool_ty = self.ctx.ty_ctx().bool_ty();
        let dest = self.alloc_local(bool_ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(dest), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(dest),
                glyim_mir::Rvalue::BinaryOp(
                    // `BinOp::And` (logical), not `BitAnd`: the interpreter
                    // implements bool short-circuit `And`/`Or` and rejects
                    // `BitAnd`/`BitOr` on `Bool` operands with
                    // "unsupported bool binop".
                    BinOp::And,
                    Box::new((
                        glyim_mir::Operand::Copy(glyim_mir::Place::new(lhs)),
                        glyim_mir::Operand::Copy(glyim_mir::Place::new(rhs)),
                    )),
                ),
            ),
            span,
        );
        dest
    }

    /// `dest = lhs | rhs` on two bool locals, returning `dest`.
    fn or_bool(&mut self, lhs: LocalIdx, rhs: LocalIdx, span: glyim_span::Span) -> LocalIdx {
        let bool_ty = self.ctx.ty_ctx().bool_ty();
        let dest = self.alloc_local(bool_ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(dest), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(dest),
                glyim_mir::Rvalue::BinaryOp(
                    // `BinOp::Or` (logical), not `BitOr`; see `and_bool`.
                    BinOp::Or,
                    Box::new((
                        glyim_mir::Operand::Copy(glyim_mir::Place::new(lhs)),
                        glyim_mir::Operand::Copy(glyim_mir::Place::new(rhs)),
                    )),
                ),
            ),
            span,
        );
        dest
    }

    /// Convert a THIR literal into a MIR constant, using `fallback_ty` for
    /// the integer / char cases where the literal's own width isn't carried.
    /// Bool is always `Ty::BOOL`.
    fn literal_to_mir_const(
        &self,
        lit: &thir::Literal,
        fallback_ty: Ty,
        span: glyim_span::Span,
    ) -> Option<glyim_mir::MirConst> {
        // Whether the comparison target is unsigned. `usize`/`u8`/`u16`/`u32`/
        // `u64` are represented as `Uint` at runtime; a `Literal::Int` used as
        // a pattern for an unsigned field (e.g. `Ok(0)` where the payload is
        // `usize`) must become a `Uint` constant, else the interpreter's
        // `Uint(n) == Int(0)` sees a mixed pair and rejects it.
        let target_is_unsigned = matches!(self.ctx.ty_ctx().ty_kind(fallback_ty), TyKind::Uint(_));
        match lit {
            thir::Literal::Int(v, _) => Some(glyim_mir::MirConst {
                kind: if target_is_unsigned {
                    glyim_mir::MirConstKind::Uint(*v as u128)
                } else {
                    glyim_mir::MirConstKind::Int(*v)
                },
                ty: fallback_ty,
                span,
            }),
            thir::Literal::Uint(v, _) => Some(glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Uint(*v),
                ty: fallback_ty,
                span,
            }),
            thir::Literal::Bool(b) => Some(glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Bool(*b),
                ty: Ty::BOOL,
                span,
            }),
            thir::Literal::Char(c) => Some(glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Char(*c),
                ty: fallback_ty,
                span,
            }),
            _ => None,
        }
    }

    fn lower_arm_body(
        &mut self,
        arm: &thir::MatchArm,
        dest_place: &glyim_mir::Place,
        merge_bb: BasicBlockIdx,
    ) {
        let arm_val = self.lower_expr_to_rvalue(&arm.body);
        // A diverging arm body (`return ..`, `panic!(..)`, `panic_any(..)`,
        // any call whose return type is `!`) has ALREADY terminated the
        // current block during `lower_expr_to_rvalue` -- see the
        // `ExprKind::Return` arm, which calls `self.terminate(Return)`.
        // Pushing the result `Assign` here would append a statement to a
        // terminated block (invalid MIR) and then
        // `self.terminate(Goto { merge_bb })` would OVERWRITE the `Return`
        // terminator, silently converting `Result::Ok(0) => return
        // Result::Err(..)` into a no-op `Goto(merge)`. That is exactly why
        // `write_all`'s `Ok(0)` / `Ok(n)` arms misbehaved and `unwrap`
        // panicked.
        if arm.body.ty == Ty::NEVER {
            return;
        }
        self.push_stmt(
            glyim_mir::StatementKind::Assign(dest_place.clone(), arm_val),
            arm.body.span,
        );
        self.terminate(
            glyim_mir::TerminatorKind::Goto { target: merge_bb },
            arm.body.span,
        );
    }
    // ---- Field resolution helpers ----
    fn resolve_field_index(
        &self,
        receiver_ty: Ty,
        field_name: glyim_core::interner::Name,
        _span: glyim_span::Span,
    ) -> Option<FieldIdx> {
        // Field access auto-derefs its receiver (mirrors Rust). Peel reference
        // layers so `(&mut Counter).field` / `(&Counter).field` resolve to the
        // underlying ADT/tuple's field index.
        let mut adj_ty = receiver_ty;
        while let TyKind::Ref(_, inner, _) = self.ctx.ty_ctx().ty_kind(adj_ty) {
            adj_ty = *inner;
        }
        match self.ctx.ty_ctx().ty_kind(adj_ty) {
            TyKind::Adt(adt_id, _substs) => self.ctx.field_index_by_name(*adt_id, 0, field_name),
            TyKind::Tuple(_substs) => {
                let name_str = self.ctx.ty_ctx().name_str(field_name);
                name_str.parse::<u32>().ok().map(FieldIdx::from_raw)
            }
            _ => None,
        }
    }

    // ---- Place helpers ----
    fn place_with_projection(
        &self,
        base: glyim_mir::Place,
        elem: ProjectionElem,
    ) -> glyim_mir::Place {
        let mut proj = base.projection.to_vec();
        proj.push(elem);
        glyim_mir::Place {
            local: base.local,
            projection: proj.into_boxed_slice(),
        }
    }

    // ---- Dynamic range slicing ----
    pub(crate) fn lower_dynamic_range_slice(
        &mut self,
        base_place: glyim_mir::Place,
        start_opt: Option<&thir::Expr>,
        end_opt: Option<&thir::Expr>,
        inclusive: bool,
        result_ty: Ty,
        span: glyim_span::Span,
    ) -> glyim_mir::Rvalue {
        // Auto-deref the base place for slicing (mirrors Rust indexing, which
        // peels `&`/`&mut` before applying the index operation). Required for
        // the common `buf[..n]` pattern where `buf: &mut [u8]` / `&[T]`.
        let raw_base_ty = base_place.ty(self.ctx.ty_ctx(), &self.locals);
        let base_place = if matches!(self.ctx.ty_ctx().ty_kind(raw_base_ty), TyKind::Ref(..)) {
            self.place_with_projection(base_place, ProjectionElem::Deref)
        } else {
            base_place
        };
        // Determine the element type and whether we have a slice or array.
        let base_ty = base_place.ty(self.ctx.ty_ctx(), &self.locals);
        let elem_ty = match self.ctx.ty_ctx().ty_kind(base_ty) {
            TyKind::Array(elem, _) | TyKind::Slice(elem) => *elem,
            // `Vec<T>` (builtin ADT id 1020) also supports dynamic range
            // slicing (via `Index<Range<usize>> -> [T]`). Extract `T` from
            // the substitution so the slice element type lines up.
            TyKind::Adt(adt_id, substs) if *adt_id == glyim_core::def_id::AdtId::from_raw(1020) => {
                self.ctx
                    .ty_ctx()
                    .substitution_args(*substs)
                    .first()
                    .and_then(|a| match a {
                        glyim_type::GenericArg::Ty(t) => Some(*t),
                        _ => None,
                    })
                    .unwrap_or_else(|| self.ctx.ty_ctx().error_ty())
            }
            // STR-SLICE-ELEM: `str` / `String` slice by *byte* range, element
            // type `u8`. `str` is `TyKind::String` (primitive) or `Adt(1050)`
            // (the stdlib's `String`), both lowering to the `{ptr, len}` fat
            // pointer — exactly what the slice path below builds. This is
            // what makes `&s[a..b]` (and thus `str::trim`) possible.
            TyKind::String => self
                .ctx
                .ty_ctx()
                .mk_ty(TyKind::Uint(glyim_core::UintTy::U8)),
            TyKind::Adt(adt_id, _) if *adt_id == glyim_core::def_id::AdtId::from_raw(1050) => self
                .ctx
                .ty_ctx()
                .mk_ty(TyKind::Uint(glyim_core::UintTy::U8)),
            _ => {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "dynamic range slicing requires array or slice type".to_string(),
                ));
                return Rvalue::Use(Operand::Constant(glyim_mir::MirConst {
                    kind: MirConstKind::Error,
                    ty: result_ty,
                    span,
                }));
            }
        };

        // Allocate locals for start, end, len, data_ptr, new_len.
        let start_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span);
        let end_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span);
        let len_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span);
        let data_ptr_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span); // as usize
        let new_len_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span);

        // StorageLive for all.
        self.push_stmt(StatementKind::StorageLive(start_local), span);
        self.push_stmt(StatementKind::StorageLive(end_local), span);
        self.push_stmt(StatementKind::StorageLive(len_local), span);
        self.push_stmt(StatementKind::StorageLive(data_ptr_local), span);
        self.push_stmt(StatementKind::StorageLive(new_len_local), span);

        // 1. Compute len_val = Len(base_place)
        let len_rvalue = Rvalue::Len(base_place.clone());
        self.push_stmt(
            StatementKind::Assign(Place::new(len_local), len_rvalue),
            span,
        );

        // 2. Compute start_val
        //
        // SLICE-BOUND-TYPE-FIX: the source bound (`2` in `a[2..]`) is
        // resolved by typeck as an *unsuffixed integer literal* whose
        // default type is `i32`. Storing it directly into `start_local`
        // (declared `usize`) left the high half of the 8-byte local
        // uninitialised — codegen emitted `store i32 2, ptr ...` and
        // every later `load i64` from that local pulled in adjacent
        // stack bytes, so `2 * 4` computed 8 from garbage+bias and drove
        // the pointer arithmetic out of bounds. Cast through a
        // temporary when the source type isn't already `usize`.
        let start_val = if let Some(start_expr) = start_opt {
            let needs_cast = !matches!(
                self.ctx.ty_ctx().ty_kind(start_expr.ty),
                TyKind::Uint(glyim_core::primitives::UintTy::Usize),
            );
            if needs_cast {
                let temp = self.alloc_local(start_expr.ty, Mutability::Mut, span);
                self.push_stmt(StatementKind::StorageLive(temp), span);
                let start_rvalue = self.lower_expr_to_rvalue(start_expr);
                self.push_stmt(
                    StatementKind::Assign(Place::new(temp), start_rvalue),
                    start_expr.span,
                );
                let cast = Rvalue::Cast(
                    glyim_mir::CastKind::IntToInt,
                    Operand::Copy(Place::new(temp)),
                    Ty::USIZE,
                );
                self.push_stmt(
                    StatementKind::Assign(Place::new(start_local), cast),
                    span,
                );
            } else {
                let start_rvalue = self.lower_expr_to_rvalue(start_expr);
                self.push_stmt(
                    StatementKind::Assign(Place::new(start_local), start_rvalue),
                    start_expr.span,
                );
            }
            Operand::Copy(Place::new(start_local))
        } else {
            // start = 0
            let zero = MirConst {
                kind: MirConstKind::Uint(0),
                ty: Ty::USIZE,
                span,
            };
            let op = Operand::Constant(zero);
            let rvalue = Rvalue::Use(op.clone());
            self.push_stmt(StatementKind::Assign(Place::new(start_local), rvalue), span);
            op
        };

        // 3. Compute end_val
        // (Same integer-width treatment as `start_val` above.)
        let end_val = if let Some(end_expr) = end_opt {
            let needs_cast = !matches!(
                self.ctx.ty_ctx().ty_kind(end_expr.ty),
                TyKind::Uint(glyim_core::primitives::UintTy::Usize),
            );
            if needs_cast {
                let temp = self.alloc_local(end_expr.ty, Mutability::Mut, span);
                self.push_stmt(StatementKind::StorageLive(temp), span);
                let end_rvalue = self.lower_expr_to_rvalue(end_expr);
                self.push_stmt(
                    StatementKind::Assign(Place::new(temp), end_rvalue),
                    end_expr.span,
                );
                let cast = Rvalue::Cast(
                    glyim_mir::CastKind::IntToInt,
                    Operand::Copy(Place::new(temp)),
                    Ty::USIZE,
                );
                self.push_stmt(
                    StatementKind::Assign(Place::new(end_local), cast),
                    span,
                );
            } else {
                let end_rvalue = self.lower_expr_to_rvalue(end_expr);
                self.push_stmt(
                    StatementKind::Assign(Place::new(end_local), end_rvalue),
                    end_expr.span,
                );
            }
            // For inclusive ranges (`..=`), the upper bound is `end + 1` because
            // the slice includes index `end`. Add 1 with an overflow check: if
            // `end == usize::MAX`, `end + 1` would wrap to 0, which must panic
            // like Rust's own bounds-check panics rather than silently wrapping.
            if inclusive {
                let eff_end_local = self.alloc_local(Ty::USIZE, Mutability::Mut, span);
                self.push_stmt(StatementKind::StorageLive(eff_end_local), span);
                let one = MirConst {
                    kind: MirConstKind::Uint(1),
                    ty: Ty::USIZE,
                    span,
                };
                let add_rval = Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new((Operand::Copy(Place::new(end_local)), Operand::Constant(one))),
                );
                self.push_stmt(
                    StatementKind::Assign(Place::new(eff_end_local), add_rval),
                    span,
                );
                // Overflow check: end <= usize::MAX - 1.
                let max_minus_one = MirConst {
                    kind: MirConstKind::Uint(usize::MAX as u128 - 1),
                    ty: Ty::USIZE,
                    span,
                };
                let end_le_max = Rvalue::BinaryOp(
                    BinOp::LtEq,
                    Box::new((
                        Operand::Copy(Place::new(end_local)),
                        Operand::Constant(max_minus_one),
                    )),
                );
                let end_le_max_local =
                    self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Not, span);
                self.push_stmt(StatementKind::StorageLive(end_le_max_local), span);
                self.push_stmt(
                    StatementKind::Assign(Place::new(end_le_max_local), end_le_max),
                    span,
                );
                let cond_op = Operand::Copy(Place::new(end_le_max_local));
                let overflow_bb = self.new_block();
                let cont_bb = self.new_block();
                self.terminate(
                    TerminatorKind::SwitchInt {
                        discr: cond_op,
                        switch_ty: self.ctx.ty_ctx().bool_ty(),
                        targets: SwitchTargets::if_switch(cont_bb, overflow_bb),
                    },
                    span,
                );
                // T099-PATCHED [LOW-8]: an overflow in `end + 1` for a
                // `..=end` range slice is a bounds violation, not a
                // compiler-unreachable state. Emit `Assert` with
                // `BoundsCheck` so the interpreter/codegen surface a clean
                // panic instead of `unreachable` (an ICE-shaped abort in
                // the interp, LLVM UB in codegen).
                self.current_block = Some(overflow_bb);
                self.terminate(
                    TerminatorKind::Assert {
                        cond: Operand::Constant(glyim_mir::MirConst {
                            kind: glyim_mir::MirConstKind::Bool(false),
                            ty: self.ctx.ty_ctx().bool_ty(),
                            span,
                        }),
                        expected: true,
                        target: cont_bb,
                        cleanup: None,
                        msg: glyim_mir::AssertMessage::BoundsCheck,
                    },
                    span,
                );
                self.current_block = Some(cont_bb);
                Operand::Copy(Place::new(eff_end_local))
            } else {
                Operand::Copy(Place::new(end_local))
            }
        } else {
            // end = len
            let op = Operand::Copy(Place::new(len_local));
            let rvalue = Rvalue::Use(op.clone());
            self.push_stmt(StatementKind::Assign(Place::new(end_local), rvalue), span);
            op
        };

        // 4. Bounds checks: start <= end, end <= len
        let check_start_le_end_bb = self.new_block();
        let check_end_le_len_bb = self.new_block();
        let done_bb = self.new_block();

        // Check start <= end
        let start_le_end =
            Rvalue::BinaryOp(BinOp::LtEq, Box::new((start_val.clone(), end_val.clone())));
        let start_le_end_local =
            self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Not, span);
        self.push_stmt(StatementKind::StorageLive(start_le_end_local), span);
        self.push_stmt(
            StatementKind::Assign(Place::new(start_le_end_local), start_le_end),
            span,
        );

        // Terminate current block with Assert on start_le_end.
        let cond_op = Operand::Copy(Place::new(start_le_end_local));
        let targets = SwitchTargets::if_switch(check_start_le_end_bb, check_end_le_len_bb);
        self.terminate(
            TerminatorKind::SwitchInt {
                discr: cond_op,
                switch_ty: self.ctx.ty_ctx().bool_ty(),
                targets,
            },
            span,
        );

        self.current_block = Some(check_start_le_end_bb);

        // Check end <= len
        let end_le_len = Rvalue::BinaryOp(
            BinOp::LtEq,
            Box::new((end_val.clone(), Operand::Copy(Place::new(len_local)))),
        );
        let end_le_len_local = self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Not, span);
        self.push_stmt(StatementKind::StorageLive(end_le_len_local), span);
        self.push_stmt(
            StatementKind::Assign(Place::new(end_le_len_local), end_le_len),
            span,
        );

        let cond_op2 = Operand::Copy(Place::new(end_le_len_local));
        let targets2 = SwitchTargets::if_switch(done_bb, check_end_le_len_bb);
        self.terminate(
            TerminatorKind::SwitchInt {
                discr: cond_op2,
                switch_ty: self.ctx.ty_ctx().bool_ty(),
                targets: targets2,
            },
            span,
        );

        // T099-PATCHED [LOW-8]: same for the `end <= len` failure. The
        // previous `Unreachable` terminator produced an interp Panic
        // ("reached unreachable terminator") and an LLVM `unreachable`
        // instruction (UB) for programs that used an out-of-range
        // `&arr[a..b]`.
        self.current_block = Some(check_end_le_len_bb);
        self.terminate(
            TerminatorKind::Assert {
                cond: Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Bool(false),
                    ty: self.ctx.ty_ctx().bool_ty(),
                    span,
                }),
                expected: true,
                target: done_bb,
                cleanup: None,
                msg: glyim_mir::AssertMessage::BoundsCheck,
            },
            span,
        );

        self.current_block = Some(done_bb);

        // Compute data_ptr:
        let first_elem_place = if matches!(self.ctx.ty_ctx().ty_kind(base_ty), TyKind::Array(_, _))
        {
            let mut proj = base_place.projection.to_vec();
            proj.push(ProjectionElem::ConstantIndex {
                offset: 0,
                min_length: 0,
                from_end: false,
            });
            Place {
                local: base_place.local,
                projection: proj.into_boxed_slice(),
            }
        } else {
            let mut proj = base_place.projection.to_vec();
            proj.push(ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0)));
            Place {
                local: base_place.local,
                projection: proj.into_boxed_slice(),
            }
        };

        // SLICE-INDEX-FIX: `data_ptr_ptr` is `usize` — it must hold an
        // *address*, not the first element's value. For an array base
        // (`&a[1..4]` on `a: [i32; 5]`) the natural projection is
        // `ConstantIndex(0)`, which yields the element *value* (`i32`);
        // storing that into a `usize` slot allocated `i32`-shaped was the
        // source of a 4-byte-vs-8-byte read at codegen (`load i64` from
        // a 4-byte `i32` alloca pulled adjacent stack bytes into the high
        // half and drove the pointer arithmetic out of bounds — a
        // SEGV on `s[0]`). For a slice base (`x: &[i32]`, `x[1..4]`)
        // the projection is `Field(0)` and already yields the fat
        // pointer's data pointer; that IS a pointer value, but still
        // needs `PtrToInt` so the destination `usize` slot holds an
        // integer that can be added to `byte_offset`.
        //
        // Both cases therefore become: take the *address* of the first
        // element (arrays) or the already-loaded pointer (slices),
        // `PtrToInt` to `usize`, store.
        let base_is_array =
            matches!(self.ctx.ty_ctx().ty_kind(base_ty), TyKind::Array(_, _));
        let data_ptr_ptr = self.alloc_local(Ty::USIZE, Mutability::Not, span);
        self.push_stmt(StatementKind::StorageLive(data_ptr_ptr), span);
        if base_is_array {
            // ARRAY BASE (`&a[1..4]`): `ConstantIndex(0)` yields the
            // element *value* (`i32`, 4 bytes), not its address. Take
            // `&a[0]` instead — a pointer — store it in a *pointer-typed*
            // local (NOT `usize`: `PtrToInt`'s codegen checks
            // `is_pointer_value()` on the source, so a `usize`-typed
            // local holding a `Ref` fails with "PtrToInt cast on
            // non-pointer"), then `PtrToInt` to `usize` for arithmetic.
            let ptr_ty = self.ctx.ty_ctx().mk_ty(TyKind::RawPtr(
                elem_ty,
                glyim_core::primitives::Mutability::Not,
            ));
            let ptr_local = self.alloc_local(ptr_ty, Mutability::Not, span);
            self.push_stmt(StatementKind::StorageLive(ptr_local), span);
            self.push_stmt(
                StatementKind::Assign(
                    Place::new(ptr_local),
                    Rvalue::Ref(first_elem_place, glyim_mir::BorrowKind::Shared),
                ),
                span,
            );
            let ptr_to_int = Rvalue::Cast(
                glyim_mir::CastKind::PtrToInt,
                Operand::Copy(Place::new(ptr_local)),
                Ty::USIZE,
            );
            self.push_stmt(
                StatementKind::Assign(Place::new(data_ptr_ptr), ptr_to_int),
                span,
            );
        } else {
            // SLICE BASE (`x: &[i32]`, `x[1..4]`): `Field(0)` of the fat
            // pointer already yields an 8-byte pointer-shaped value.
            // Store it directly into the `usize` slot.
            let load_data_ptr = Rvalue::Use(Operand::Copy(first_elem_place));
            self.push_stmt(
                StatementKind::Assign(Place::new(data_ptr_ptr), load_data_ptr),
                span,
            );
        }

        let layout_computer =
            glyim_layout::SimpleLayoutComputer::new(self.ctx.ty_ctx(), TargetInfo::x86_64());
        // If the element type is (or contains) a generic `Param`, its layout
        // is unknown here (`layout_of` fails). Baking a fallback size of 1
        // miscompiled every generic slice range (`&slice[1..]` advanced by 1
        // byte instead of `size_of::<T>()`). Emit a deferred
        // `MirConstKind::SizeOf(elem_ty)` instead: mono substitutes the
        // concrete type into it and the backends fold the real size.
        let elem_size_const = match layout_computer.layout_of(elem_ty) {
            Ok(l) if !contains_param(self.ctx.ty_ctx(), elem_ty) => MirConst {
                kind: MirConstKind::Uint(l.size.0.into()),
                ty: Ty::USIZE,
                span,
            },
            _ => MirConst {
                kind: MirConstKind::SizeOf(elem_ty),
                ty: Ty::USIZE,
                span,
            },
        };
        let start_op = Operand::Copy(Place::new(start_local));
        let size_op = Operand::Constant(elem_size_const);
        let byte_offset = Rvalue::BinaryOp(BinOp::Mul, Box::new((start_op, size_op)));
        let byte_offset_local = self.alloc_local(Ty::USIZE, Mutability::Not, span);
        self.push_stmt(StatementKind::StorageLive(byte_offset_local), span);
        self.push_stmt(
            StatementKind::Assign(Place::new(byte_offset_local), byte_offset),
            span,
        );

        let data_ptr_rvalue = Rvalue::BinaryOp(
            BinOp::Add,
            Box::new((
                Operand::Copy(Place::new(data_ptr_ptr)),
                Operand::Copy(Place::new(byte_offset_local)),
            )),
        );
        self.push_stmt(
            StatementKind::Assign(Place::new(data_ptr_local), data_ptr_rvalue),
            span,
        );

        let new_len_rvalue = Rvalue::BinaryOp(
            BinOp::Sub,
            Box::new((end_val, Operand::Copy(Place::new(start_local)))),
        );
        self.push_stmt(
            StatementKind::Assign(Place::new(new_len_local), new_len_rvalue),
            span,
        );

        let slice_operands = vec![
            Operand::Copy(Place::new(data_ptr_local)),
            Operand::Copy(Place::new(new_len_local)),
        ];
        Rvalue::Aggregate(glyim_mir::AggregateKind::Tuple, slice_operands)
    }
}

// ---------------------------------------------------------------------------
// T121-PATCHED-ACTIVATION [BCK-1]: precise two-phase borrow flag computation.
// ---------------------------------------------------------------------------

/// For a call's argument list, compute the two-phase-borrow flag for each
/// argument. `flags[i]` is `true` iff `args[i]` is an `&mut <local>` whose
/// target local is also read by at least one *sibling* argument — the exact
/// precondition under which rustc reserves a two-phase borrow.
///
/// The analysis is conservative in the "keep the reservation" direction: any
/// expression shape whose child set is not exhaustively modelled is treated
/// as *reading* the place, so the flag is over-set rather than under-set.
/// Over-setting admits strictly more programs (i.e. the pre-refinement
/// behavior) and is sound; the refinement only *removes* the flag where we
/// can prove no sibling reads the borrowed place.
fn compute_two_phase_flags(args: &[thir::Expr]) -> Vec<bool> {
    let mut flags = vec![false; args.len()];
    for (i, arg) in args.iter().enumerate() {
        let Some(target) = ref_mut_target_local(arg) else {
            continue;
        };
        for (j, sib) in args.iter().enumerate() {
            if i == j {
                continue;
            }
            if expr_reads_local(sib, target) {
                flags[i] = true;
                break;
            }
        }
    }
    flags
}

/// If `e` is a `&mut <local>` reference expression, return the local's id.
fn ref_mut_target_local(e: &thir::Expr) -> Option<thir::LocalVarId> {
    if let thir::ExprKind::Ref {
        mutability: Mutability::Mut,
        operand,
    } = &e.kind
    {
        if let thir::ExprKind::VarRef(id) = &operand.kind {
            return Some(*id);
        }
    }
    None
}

/// Approximate "does `e` read local `v`?" for the T121 flag. Returns `true`
/// for any expression shape whose child set is not exhaustively modelled, so
/// the flag is over-set rather than under-set.
fn expr_reads_local(e: &thir::Expr, v: thir::LocalVarId) -> bool {
    match &e.kind {
        thir::ExprKind::VarRef(id) => *id == v,
        // A mutable reborrow is not a read of the underlying local.
        thir::ExprKind::Ref {
            mutability: Mutability::Mut,
            ..
        } => false,
        // A shared reborrow is a read.
        thir::ExprKind::Ref { operand, .. } => expr_reads_local(operand, v),
        thir::ExprKind::Binary { lhs, rhs, .. } => {
            expr_reads_local(lhs, v) || expr_reads_local(rhs, v)
        }
        thir::ExprKind::Unary { operand, .. } => expr_reads_local(operand, v),
        thir::ExprKind::Call { func, args } => {
            expr_reads_local(func, v) || args.iter().any(|a| expr_reads_local(a, v))
        }
        thir::ExprKind::DynamicCall { receiver, args, .. } => {
            expr_reads_local(receiver, v) || args.iter().any(|a| expr_reads_local(a, v))
        }
        // Known leaves that cannot read a local.
        thir::ExprKind::Literal(_)
        | thir::ExprKind::FnRef(_)
        | thir::ExprKind::ConstRef(_)
        | thir::ExprKind::Err => false,
        // Anything else: over-approximate (keeps the reservation).
        _ => true,
    }
}

#[cfg(test)]
mod t121_two_phase_tests {
    use super::*;

    fn var(v: u32) -> thir::Expr {
        thir::Expr {
            kind: thir::ExprKind::VarRef(thir::LocalVarId::from_raw(v)),
            ty: Ty::ERROR,
            span: glyim_span::Span::DUMMY,
        }
    }
    fn ref_mut(v: u32) -> thir::Expr {
        thir::Expr {
            kind: thir::ExprKind::Ref {
                mutability: Mutability::Mut,
                operand: Box::new(var(v)),
            },
            ty: Ty::ERROR,
            span: glyim_span::Span::DUMMY,
        }
    }
    fn leaf() -> thir::Expr {
        thir::Expr {
            kind: thir::ExprKind::Err,
            ty: Ty::ERROR,
            span: glyim_span::Span::DUMMY,
        }
    }

    #[test]
    fn lone_mut_ref_is_not_two_phase() {
        // `f(&mut x)` — a plain mutable borrow, no reservation needed.
        let args = vec![ref_mut(0)];
        assert_eq!(compute_two_phase_flags(&args), vec![false]);
    }

    #[test]
    fn mut_ref_with_sibling_read_is_two_phase() {
        // `f(&mut x, x)` — the canonical two-phase borrow.
        let args = vec![ref_mut(0), var(0)];
        assert_eq!(compute_two_phase_flags(&args), vec![true, false]);
    }

    #[test]
    fn mut_ref_with_unrelated_sibling_is_not_two_phase() {
        // `f(&mut x, y)` — no sibling read of `x`.
        let args = vec![ref_mut(0), var(1)];
        assert_eq!(compute_two_phase_flags(&args), vec![false, false]);
    }

    #[test]
    fn read_before_reservation_still_marks_later_mut_ref() {
        // `f(x, &mut x)` — the read precedes the reservation; rustc accepts
        // and the conservative flag covers it.
        let args = vec![var(0), ref_mut(0)];
        assert_eq!(compute_two_phase_flags(&args), vec![false, true]);
    }

    #[test]
    fn mut_ref_with_leaf_sibling_is_not_two_phase() {
        // `f(&mut x, <opaque leaf>)` — a leaf that cannot read `x`.
        let args = vec![ref_mut(0), leaf()];
        assert_eq!(compute_two_phase_flags(&args), vec![false, false]);
    }
}

// ---------------------------------------------------------------------------
