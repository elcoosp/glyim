use glyim_core::arena::IndexVec;
use glyim_core::interner::Name;
use glyim_core::primitives::Mutability;
use glyim_diag::GlyimDiagnostic;
use glyim_mir::{BasicBlockIdx, LocalIdx};
use glyim_span::Span;
use glyim_type::*;
use glyim_typeck::thir;

use crate::lower::LowerCtx;

/// Information about an enclosing loop, used for break/continue targeting.
pub(crate) struct LoopInfo {
    pub(crate) continue_bb: BasicBlockIdx,
    pub(crate) break_bb: BasicBlockIdx,
    /// The local that a `break <value>` writes and the loop expression reads
    /// back as its result (HIR-30). For a `while`/`for` (unit result) it holds
    /// `()`; for `loop` it holds the break value's type.
    pub(crate) break_place: LocalIdx,
}

/// The MIR Builder.
///
/// Accumulates locals, basic blocks, statements, and terminators as THIR
/// expressions are recursively lowered to MIR.
pub struct MirBuilder<'a> {
    pub(crate) ctx: &'a dyn LowerCtx,
    pub(crate) locals: IndexVec<LocalIdx, glyim_mir::LocalDecl>,
    pub(crate) basic_blocks: IndexVec<BasicBlockIdx, glyim_mir::BasicBlockData>,
    pub(crate) arg_count: usize,
    pub(crate) return_ty: Ty,
    pub(crate) owner: glyim_core::def_id::DefId,
    pub(crate) span: Span,
    pub(crate) diagnostics: Vec<GlyimDiagnostic>,
    pub(crate) closure_bodies: Vec<(
        glyim_core::def_id::ClosureId,
        glyim_type::Substitution,
        glyim_mir::Body,
    )>,
    pub(crate) var_map: std::collections::HashMap<Name, LocalIdx>,
    pub(crate) capture_map: std::collections::HashMap<thir::LocalVarId, LocalIdx>,
    /// HIR-31: captures whose environment field holds a *reference* into the
    /// enclosing frame (`ByRef`/`ByRef(Mut)`). A closure-body `VarRef` to one
    /// of these must be auto-deref'd: the capture local is typed `&T`/`&mut T`
    /// while the body's THIR types the variable as `T`.
    pub(crate) byref_capture_vars: std::collections::HashSet<thir::LocalVarId>,
    pub(crate) param_map: std::collections::HashMap<thir::LocalVarId, LocalIdx>,
    /// Maps a type-checker `LocalVarId` (carried by `VarRef` and by
    /// `PatternKind::Binding::var_id`) to the MIR `LocalIdx` this builder
    /// allocated for that binding. The type-checker's `LocalVarId` space and
    /// the MIR local-index space are NOT aligned (temporaries interleave
    /// between user-variable allocations), so `VarRef(local_var_id)` must be
    /// resolved through this map rather than `LocalIdx::from_raw(var_id)`.
    pub(crate) local_var_map: std::collections::HashMap<thir::LocalVarId, LocalIdx>,
    pub(crate) current_block: Option<BasicBlockIdx>,
    /// Stack of enclosing loops for break/continue resolution.
    pub(crate) loop_stack: Vec<LoopInfo>,
    /// Maps a droppable local to its shadow boolean drop-flag local.
    ///
    /// **Tier 1.8 (partial-move drop elaboration) infrastructure.** When a
    /// local is partially moved (e.g. `let y = x.field;` where `field` is not
    /// `Copy`), its whole-struct scope-exit `Drop` must be guarded so it only
    /// fires for the still-initialized fields. This map backs that guard:
    /// `elaborate_scope_drops` wraps each `Drop` in a `SwitchInt` on the flag,
    /// dropping only when the flag is still `true`.
    ///
    /// Population of this map (allocating a `bool` flag per droppable local
    /// and **clearing** it at each move) is gated on landing an explicit
    /// Copy/Move distinction in lowering. Today THIR has no `Move` node and
    /// field access lowers as `Operand::Copy`, so moves are not yet modeled
    /// at this layer — the double-drop scenario the plan targets is
    /// therefore not currently reachable, and the independent
    /// `glyim-borrowck` move analysis already tracks per-field move paths
    /// separately. Until move-semantics land, the map stays empty and the
    /// guard degrades to the pre-existing unconditional-drop behavior (always
    /// sound). See `docs/plans/v0.1.0/feature-gaps/part2.md` §1.8.
    pub(crate) drop_flags: std::collections::HashMap<LocalIdx, LocalIdx>,
}

impl<'a> MirBuilder<'a> {
    /// Create a new MIR builder for the given THIR body.
    pub fn new(ctx: &'a dyn LowerCtx, thir: &thir::Body) -> Self {
        let mut locals = IndexVec::new();
        // _0 is return place
        locals.push(glyim_mir::LocalDecl {
            ty: thir.return_ty,
            mutability: Mutability::Mut,
            source_info: glyim_mir::SourceInfo::new(thir.span),
        });

        Self {
            ctx,
            locals,
            basic_blocks: IndexVec::new(),
            arg_count: thir.params.len(),
            return_ty: thir.return_ty,
            owner: thir.owner,
            span: thir.span,
            diagnostics: Vec::new(),
            closure_bodies: Vec::new(),
            var_map: std::collections::HashMap::new(),
            capture_map: std::collections::HashMap::new(),
            byref_capture_vars: std::collections::HashSet::new(),
            param_map: std::collections::HashMap::new(),
            local_var_map: std::collections::HashMap::new(),
            current_block: None,
            loop_stack: Vec::new(),
            drop_flags: std::collections::HashMap::new(),
        }
    }

    /// Allocate a new basic block and return its index.
    pub fn new_block(&mut self) -> BasicBlockIdx {
        self.basic_blocks.push(glyim_mir::BasicBlockData {
            statements: Vec::new(),
            terminator: glyim_mir::Terminator {
                kind: glyim_mir::TerminatorKind::Unreachable,
                source_info: glyim_mir::SourceInfo::new(self.span),
            },
            is_cleanup: false,
        })
    }

    /// Allocate a new local variable and return its index.
    pub fn alloc_local(&mut self, ty: Ty, mutability: Mutability, span: Span) -> LocalIdx {
        self.locals.push(glyim_mir::LocalDecl {
            ty,
            mutability,
            source_info: glyim_mir::SourceInfo::new(span),
        })
    }

    /// Push a statement onto the current basic block.
    pub fn push_stmt(&mut self, stmt: glyim_mir::StatementKind, span: Span) {
        if let Some(bb) = self.current_block {
            self.basic_blocks[bb].statements.push(glyim_mir::Statement {
                kind: stmt,
                source_info: glyim_mir::SourceInfo::new(span),
            });
        }
    }

    /// Lower a THIR body into this builder, populating locals, blocks, etc.
    pub fn lower_body(&mut self, thir: &thir::Body) {
        let entry = self.new_block();
        self.current_block = Some(entry);

        for param in &thir.params {
            let local = self.alloc_local(param.ty, Mutability::Not, param.span);
            self.push_stmt(glyim_mir::StatementKind::StorageLive(local), param.span);
            match &param.pat.kind {
                thir::PatternKind::Binding {
                    name,
                    var_id,
                    mutability: _,
                    subpattern,
                } => {
                    self.var_map.insert(*name, local);
                    self.local_var_map.insert(*var_id, local);
                    if let Some(sub) = subpattern {
                        self.bind_pattern(sub, Some(local), param.span);
                    }
                }
                thir::PatternKind::Wild => {}
                _ => {
                    self.bind_pattern(&param.pat, Some(local), param.span);
                }
            }
        }

        for (i, stmt) in thir.stmts.iter().enumerate() {
            let is_last = i + 1 == thir.stmts.len();
            if is_last {
                // The function body's tail expression (a `Stmt::Expr` pushed
                // by the type-checker as the final statement) is the function's
                // return value. Route it into the return slot (local 0) instead
                // of a discarded temporary, otherwise a non-`()` function that
                // ends in a bare expression returns an uninitialized slot
                // (interpreter panics "read from uninitialized local 0"). For
                // `()` tails this also writes a valid `Unit` so the `Return`
                // terminator reads a populated slot.
                if let thir::Stmt::Expr { expr } = stmt {
                    let rvalue = self.lower_expr_to_rvalue(expr);
                    let ret_place = glyim_mir::Place::new(LocalIdx::from_raw(0));
                    self.push_stmt(
                        glyim_mir::StatementKind::Assign(ret_place, rvalue),
                        expr.span,
                    );
                    continue;
                }
            }
            self.lower_stmt(stmt);
        }

        if self.current_block.is_some() {
            // Elaborate drop terminators for every non-Copy local at scope
            // exit, in reverse declaration order, immediately before the
            // `Return`. (Tier 1.6 — top-level / whole-value drop elaboration;
            // the per-projection partial-move guard from Tier 1.8 is wired in
            // `elaborate_scope_drops` via `drop_flags` but stays inert until
            // move-semantics land — see the field's doc comment and
            // `docs/plans/v0.1.0/feature-gaps/part2.md` §1.8.)
            //
            // This mirrors the previous `terminate(Return)` behavior: it only
            // runs when control fell straight through to the *current* block
            // (i.e. `current_block.is_some()`). For `if`/`match`/`while`/`loop`
            // the lowering redirected control flow into other blocks and set
            // `current_block = None`, so we must not inject drops there (the
            // real terminators of those constructs are untouched).
            let fall_through = self.current_block.unwrap();
            self.elaborate_scope_drops(fall_through, thir.span);
        }
    }

    /// Insert a chain of `Drop` terminators for every local whose type needs a
    /// destructor, executed in reverse declaration order at the end of the
    /// function body, immediately before a `Return` terminator.
    ///
    /// The return place (`_0`) is never dropped here. Parameters are also
    /// skipped: in this minimal model they are handled by the caller's scope
    /// and dropping them here would duplicate the drop glue.
    pub(crate) fn elaborate_scope_drops(&mut self, fall_through: BasicBlockIdx, span: Span) {
        // Collect non-Copy locals (excluding the return place) that need a
        // destructor, in declaration order.
        let mut to_drop: Vec<LocalIdx> = Vec::new();
        for (idx, decl) in self.locals.iter_enumerated() {
            if idx.to_raw() == 0 {
                continue; // return place
            }
            if (idx.to_raw() as usize) <= self.arg_count {
                continue; // parameters are dropped by the caller's scope
            }
            if self.needs_drop(decl.ty) {
                to_drop.push(idx);
            }
        }
        // Reverse so the highest-indexed (most recently declared) local is
        // dropped first, matching Rust's reverse declaration-order rule.
        to_drop.reverse();

        if to_drop.is_empty() {
            self.basic_blocks[fall_through].terminator = glyim_mir::Terminator {
                kind: glyim_mir::TerminatorKind::Return,
                source_info: glyim_mir::SourceInfo::new(span),
            };
            self.current_block = None;
            return;
        }

        // Build a dedicated return block and chain the drop terminators so
        // that each `Drop` targets the next drop (or the return block).
        let return_bb = self.new_block();
        self.basic_blocks[return_bb].terminator = glyim_mir::Terminator {
            kind: glyim_mir::TerminatorKind::Return,
            source_info: glyim_mir::SourceInfo::new(span),
        };

        let mut target = return_bb;
        for local in to_drop {
            let drop_bb = self.new_block();
            self.basic_blocks[drop_bb].terminator = glyim_mir::Terminator {
                kind: glyim_mir::TerminatorKind::Drop {
                    place: glyim_mir::Place::new(local),
                    target,
                    cleanup: None,
                },
                source_info: glyim_mir::SourceInfo::new(span),
            };
            // Tier 1.8: if a drop-flag was allocated for this local, guard the
            // `Drop` behind a `SwitchInt` on the flag so a partially-moved
            // local only drops its still-initialized fields. When no flag is
            // registered (the current state — move-semantics not yet landed),
            // this degrades to the pre-existing unconditional `Drop`, which is
            // always sound.
            let next_target = if let Some(&flag) = self.drop_flags.get(&local) {
                let check_bb = self.new_block();
                self.basic_blocks[check_bb].terminator = glyim_mir::Terminator {
                    kind: glyim_mir::TerminatorKind::SwitchInt {
                        discr: glyim_mir::Operand::Copy(glyim_mir::Place::new(flag)),
                        targets: glyim_mir::SwitchTargets::new(
                            Box::new([(1u128, drop_bb)]),
                            target,
                        ),
                        switch_ty: self.ctx.ty_ctx().bool_ty(),
                    },
                    source_info: glyim_mir::SourceInfo::new(span),
                };
                check_bb
            } else {
                drop_bb
            };
            target = next_target;
        }

        self.basic_blocks[fall_through].terminator = glyim_mir::Terminator {
            kind: glyim_mir::TerminatorKind::Goto { target },
            source_info: glyim_mir::SourceInfo::new(span),
        };
        self.current_block = None;
    }

    /// Whether a type needs a destructor call at the end of its scope.
    ///
    /// Delegates to the single authoritative `TyCtx::needs_drop`, which both
    /// this crate and `glyim-opt` now share (de-stubbing plan §8.2/§12.3).
    /// Having one implementation removes the soundness risk where MIR building
    /// and optimization disagreed on drop-carrying-ness for an identical type.
    fn needs_drop(&self, ty: Ty) -> bool {
        self.ctx.ty_ctx().needs_drop(ty)
    }

    /// Phase 4 (GLYIM_DESTUB_PLAN): when a droppable `let`-bound local is
    /// declared, pre-allocate its drop-flag, `StorageLive` it, and initialize
    /// it to `true` (fully initialized) at the declaration site — which
    /// dominates every later path, so the flag is always defined before
    /// `elaborate_scope_drops` reads it at scope exit. A later partial move
    /// (`register_partial_move`) clears it to `false`.
    ///
    /// Idempotent: a local declared once gets exactly one flag, regardless of
    /// how many times this is called.
    pub(crate) fn register_drop_flag_init(&mut self, local: LocalIdx, span: Span) {
        if self.drop_flags.contains_key(&local) {
            return;
        }
        let flag = self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Mut, span);
        self.drop_flags.insert(local, flag);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(flag), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(flag),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Bool(true),
                    ty: self.ctx.ty_ctx().bool_ty(),
                    span,
                })),
            ),
            span,
        );
    }

    /// Phase 4 (GLYIM_DESTUB_PLAN): a field of a non-`Copy` parent was just
    /// moved out of (read as an rvalue operand in the `Field` arm of
    /// `lower_expr_to_rvalue`). Ensure the parent local has a drop-flag, and
    /// emit the statement that clears it — marking the whole value as "no
    /// longer fully initialized" so its scope-exit `Drop`, guarded by
    /// `elaborate_scope_drops`, is skipped.
    ///
    /// The flag models "should the parent's `Drop` run at all," not per-field
    /// state (fine-grained per-field drop guards are a natural v2). Disabling
    /// the parent's `Drop` entirely on any partial move is always sound — it
    /// only over-retains (leaks) the still-initialized sibling fields, never
    /// double-frees.
    pub(crate) fn register_partial_move(&mut self, local: LocalIdx, span: Span) {
        let flag = if let Some(&flag) = self.drop_flags.get(&local) {
            flag
        } else {
            let f = self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Mut, span);
            self.drop_flags.insert(local, f);
            f
        };
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(flag),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                    kind: glyim_mir::MirConstKind::Bool(false),
                    ty: self.ctx.ty_ctx().bool_ty(),
                    span,
                })),
            ),
            span,
        );
    }

    /// Lower a closure expression: generate its MIR body and return an aggregate.
    #[allow(dead_code)]
    pub(crate) fn lower_closure(
        &mut self,
        thir_body: &thir::Body,
        captures: &[thir::Capture],
        closure_id: glyim_core::def_id::ClosureId,
        substs: glyim_type::Substitution,
        span: glyim_span::Span,
    ) -> glyim_mir::Rvalue {
        use glyim_core::def_id::{CrateId, DefId, LocalDefId};
        use glyim_mir::Rvalue;
        use glyim_typeck::thir;

        // Create DefId for the closure.
        let def_id = DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(closure_id.to_raw()),
        );

        // Build a new MIR body for the closure using a fresh builder.
        let mut builder = MirBuilder::new(self.ctx, thir_body);

        // Allocate locals for captures and populate capture_map.
        for capture in captures {
            let mutability = match capture.kind {
                thir::CaptureKind::ByValue => glyim_core::primitives::Mutability::Not,
                thir::CaptureKind::ByRef(m) => m,
            };
            let local = builder.alloc_local(capture.ty, mutability, span);
            builder.capture_map.insert(capture.local, local);
            builder.local_var_map.insert(capture.local, local);
            if matches!(capture.kind, thir::CaptureKind::ByRef(_)) {
                builder.byref_capture_vars.insert(capture.local);
            }
        }

        // Allocate locals for parameters and populate param_map.
        // Map each parameter's real LocalVarId (from the THIR Param) to its MIR
        // local so the body's VarRefs resolve correctly.
        for param in thir_body.params.iter() {
            let mutability = match &param.pat.kind {
                thir::PatternKind::Binding { mutability, .. } => *mutability,
                _ => glyim_core::primitives::Mutability::Not,
            };
            let local = builder.alloc_local(param.ty, mutability, param.span);
            builder.param_map.insert(param.local, local);
            builder.local_var_map.insert(param.local, local);
        }

        // Set arg_count to include captures + original params.
        builder.arg_count = captures.len() + thir_body.params.len();

        // Lower the THIR body into the builder.
        builder.lower_body(thir_body);

        // Extract the built body.
        let mut closure_body = glyim_mir::Body::dummy(def_id);
        closure_body.basic_blocks = builder.basic_blocks;
        closure_body.locals = builder.locals;
        closure_body.arg_count = builder.arg_count;
        closure_body.return_ty = builder.return_ty;
        closure_body.span = builder.span;

        // Store the closure body.
        self.closure_bodies.push((closure_id, substs, closure_body));

        // Build the aggregate: function pointer + captures.
        let fn_const = glyim_mir::MirConst {
            kind: glyim_mir::MirConstKind::Fn(
                glyim_core::def_id::FnDefId::from_raw(closure_id.to_raw()),
                substs,
            ),
            ty: self.ctx.ty_ctx().error_ty(),
            span,
        };
        let fn_operand = glyim_mir::Operand::Constant(fn_const);
        let mut operands = vec![fn_operand];
        for capture in captures {
            let local = glyim_mir::LocalIdx::from_raw(capture.local.to_raw());
            let operand = match capture.kind {
                thir::CaptureKind::ByValue => {
                    glyim_mir::Operand::Move(glyim_mir::Place::new(local))
                }
                thir::CaptureKind::ByRef(_) => {
                    glyim_mir::Operand::Copy(glyim_mir::Place::new(local))
                }
            };
            operands.push(operand);
        }
        Rvalue::Aggregate(
            glyim_mir::AggregateKind::Closure(closure_id, substs),
            operands,
        )
    }
}
