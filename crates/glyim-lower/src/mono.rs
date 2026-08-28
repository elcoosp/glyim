//! Monomorphization: instantiate generic MIR bodies with concrete types.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::{ConstDefId, CrateId, DefId, FnDefId, LocalDefId, StaticDefId};
use glyim_mir::{self, MirConstKind, Operand, Rvalue, StatementKind, TerminatorKind};
use glyim_type::*;
use std::sync::Arc;

glyim_core::define_idx!(MonoItemId);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// MonoItem.
pub enum MonoItem {
/// Variant.
    Fn {
/// Struct.
        def_id: FnDefId,
/// Struct.
        substs: Substitution,
    },
/// Variant.
    Const {
/// Struct.
        def_id: ConstDefId,
/// Struct.
        substs: Substitution,
    },
/// Variant.
    Static {
/// Struct.
        def_id: StaticDefId,
    },
/// Variant.
    DropGlue {
/// Struct.
        ty: Ty,
    },
}

#[derive(Clone, Debug)]
/// MonoItemData.
pub struct MonoItemData {
/// Struct.
    pub item: MonoItem,
/// Struct.
    pub body: Arc<glyim_mir::Body>,
/// Struct.
    pub symbol: String,
/// Struct.
    pub source_module: u32,
}

/// MonoCtx.
pub struct MonoCtx<'a> {
    items: IndexVec<MonoItemId, MonoItemData>,
    queue: std::collections::VecDeque<MonoItem>,
    seen: std::collections::HashSet<MonoItem>,
    cache: std::collections::HashMap<MonoItem, MonoItemId>,
    drop_locals: Vec<glyim_mir::LocalIdx>,
    /// Optional type context, used to derive generic substitutions from call
    /// argument types during monomorphization (closes the single-await
    /// `TyKind::Error` codegen gap for generic `Future::Output`/`block_on`).
    ty_ctx: Option<&'a TyCtx>,
}

impl<'a> MonoCtx<'a> {
/// new.
    pub fn new() -> Self {
        Self {
            items: IndexVec::new(),
            queue: std::collections::VecDeque::new(),
            seen: std::collections::HashSet::new(),
            cache: std::collections::HashMap::new(),
            drop_locals: Vec::new(),
            ty_ctx: None,
        }
    }

    /// Attach a type context so generic call sites can be instantiated from
    /// their concrete argument types (rather than the often-empty substitution
    /// carried by the callee constant).
    pub fn with_ty_ctx(&mut self, ty_ctx: &'a TyCtx) {
        self.ty_ctx = Some(ty_ctx);
    }

    fn enqueue(&mut self, item: MonoItem) {
        if !self.seen.contains(&item) && !self.cache.contains_key(&item) {
            self.queue.push_back(item);
        }
    }

/// collect.
    pub fn collect(
        &mut self,
        start: &[MonoItem],
        mir_bodies: &dyn Fn(DefId, &Substitution) -> Arc<glyim_mir::Body>,
        drop_glue_body: &dyn Fn(Ty) -> Arc<glyim_mir::Body>,
    ) {
        for item in start {
            if self.cache.contains_key(item) || self.seen.contains(item) {
                continue;
            }
            self.queue.push_back(item.clone());
        }

        while let Some(item) = self.queue.pop_front() {
            if self.seen.contains(&item) || self.cache.contains_key(&item) {
                continue;
            }
            self.seen.insert(item.clone());

            let body = match &item {
                MonoItem::Fn { def_id, substs } => mir_bodies(
                    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(def_id.to_raw())),
                    substs,
                ),
                MonoItem::Const { def_id, substs } => mir_bodies(
                    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(def_id.to_raw())),
                    substs,
                ),
                MonoItem::Static { .. } => Arc::new(glyim_mir::Body::dummy(DefId::new(
                    CrateId::from_raw(0),
                    LocalDefId::from_raw(0),
                ))),
                MonoItem::DropGlue { ty } => drop_glue_body(*ty),
            };

            // Devirtualize generic-bound trait method calls
            // (`f.poll()` where `f: F: Trait`) against the *concrete* receiver
            // types that monomorphization has substituted. Each
            // `VirtualMethod` constant is rewritten in place to a direct
            // `Fn(def_id)` constant so the existing `scan_terminator` `Fn` arm
            // enqueues the impl and codegen sees a static call.
            let mut body = body;
            self.devirtualize(&mut body);

            self.scan_body_for_refs(&body);
            let symbol = format!("{:?}", item);
            let id = self.items.push(MonoItemData {
                item: item.clone(),
                body,
                symbol,
                source_module: 0,
            });
            self.cache.insert(item, id);
        }
    }

    fn scan_body_for_refs(&mut self, body: &glyim_mir::Body) {
        self.drop_locals.clear();

        for block in body.basic_blocks.iter() {
            for stmt in &block.statements {
                if let StatementKind::Assign(_, ref rvalue) = stmt.kind {
                    self.scan_rvalue(rvalue);
                }
            }
            self.scan_terminator(&block.terminator.kind, body);
        }

        let pending_drops: Vec<_> = self.drop_locals.drain(..).collect();
        for local_idx in pending_drops {
            let local_raw = local_idx.to_raw() as usize;
            if let Some(local_decl) = body
                .locals
                .get(glyim_mir::LocalIdx::from_raw(local_raw as u32))
            {
                let drop_ty = local_decl.ty;
                self.enqueue(MonoItem::DropGlue { ty: drop_ty });
            }
        }
    }

    /// Rewrite every `VirtualMethod` callee constant in `body` to a direct
    /// `Fn(def_id)` constant, resolving the impl function against the
    /// monomorphized (concrete) receiver type of the call's first argument.
    /// No-op when no type context is attached or when a receiver type cannot
    /// be recovered. After rewriting, `scan_terminator`'s `Fn` arm enqueues
    /// the resolved function automatically.
    fn devirtualize(&self, body: &mut Arc<glyim_mir::Body>) {
        let Some(ty_ctx) = self.ty_ctx else {
            return;
        };
        let body_mut = Arc::make_mut(body);
        for block in body_mut.basic_blocks.iter_mut() {
            if let TerminatorKind::Call { func, args, .. } = &mut block.terminator.kind {
                let recv_ty = args.first().and_then(|a| match a {
                    Operand::Constant(c) => Some(c.ty),
                    Operand::Copy(p) | Operand::Move(p) => {
                        body_mut.locals.get(p.local).map(|d| d.ty)
                    }
                });
                if let Operand::Constant(c) = func {
                    if let MirConstKind::VirtualMethod {
                        trait_def_id,
                        method_name,
                    } = c.kind
                    {
                        if let Some(fn_def_id) = recv_ty.and_then(|rt| {
                            ty_ctx.resolve_trait_method(trait_def_id, rt, method_name)
                        }) {
                            // Pass the *concrete* receiver `Self` type as the
                            // method's substitution instead of an empty one.
                            // The body is already monomorphized (the MIR
                            // provider substitutes generic params before this
                            // runs), so the receiver type here is concrete.
                            // Without this, the resolved method was enqueued
                            // with empty substs and its own body kept a
                            // `Param`/`Self` type, which then ICEs at codegen
                            // (e.g. `f.poll()` inside `block_on<F>` leaving
                            // `Poll<Self::Output>` unresolved).
                            let self_ty = recv_ty.map(|rt| match ty_ctx.ty_kind(rt) {
                                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                                _ => rt,
                            });
                            let substs = match self_ty {
                                Some(st) => {
                                    ty_ctx.intern_substitution(vec![GenericArg::Ty(st)])
                                }
                                None => Substitution::empty(),
                            };
                            let fn_ty = ty_ctx.mk_ty(TyKind::FnDef(fn_def_id, substs));
                            *c = glyim_mir::MirConst {
                                kind: MirConstKind::Fn(fn_def_id, substs),
                                ty: fn_ty,
                                span: c.span,
                            };
                        }
                    }
                }
            }
        }
    }

    fn scan_rvalue(&mut self, rvalue: &Rvalue) {
        match rvalue {
            Rvalue::Use(operand) => self.scan_operand(operand),
            Rvalue::BinaryOp(_, operands) => {
                let (lhs, rhs) = operands.as_ref();
                self.scan_operand(lhs);
                self.scan_operand(rhs);
            }
            Rvalue::UnaryOp(_, operand) => self.scan_operand(operand),
            Rvalue::Aggregate(_, operands) => {
                for operand in operands {
                    self.scan_operand(operand);
                }
            }
            Rvalue::Cast(_, operand, _) => self.scan_operand(operand),
            Rvalue::Repeat(operand, _) => self.scan_operand(operand),
            _ => {}
        }
    }

    fn scan_operand(&mut self, operand: &Operand) {
        if let Operand::Constant(mir_const) = operand {
            self.scan_const(mir_const);
        }
    }

    fn scan_const(&mut self, mir_const: &glyim_mir::MirConst) {
        match &mir_const.kind {
            MirConstKind::Fn(def_id, substs) => {
                // When a type context is present, generic calls are
                // instantiated from their argument types at the `Call`
                // terminator (see `scan_terminator`); the callee constant
                // frequently carries an empty substitution, so we skip it here
                // to avoid enqueuing an un-instantiated (-polymorphic) body
                // that codegen cannot lower. When no context is attached
                // (e.g. unit tests), fall back to the constant's own substs.
                if self.ty_ctx.is_none() {
                    self.enqueue(MonoItem::Fn {
                        def_id: *def_id,
                        substs: *substs,
                    });
                }
            }
            // ConstRef constants are materialized by the backend as zero-
            // initialized globals (`__glyim_const_{id}`); full const
            // evaluation/lowering is a follow-up, so we do not enqueue a
            // `MonoItem::Const` (which would require a lowered const body)
            // for value-namespace const references yet.
            MirConstKind::ConstRef(_, _) => {}
            _ => {}
        }
    }

    fn scan_terminator(&mut self, kind: &TerminatorKind, body: &glyim_mir::Body) {
        match kind {
            TerminatorKind::Call { func, args, .. } => {
                // Instantiate the callee from the substitution carried by the
                // callee constant's `FnDef` type. That substitution is produced
                // by the type-checker's generic-call instantiation, which sets
                // the `FnRef` node's type to e.g. `FnDef(id, [i32])`; it is
                // therefore already a correctly-interned `Substitution` shared
                // with the type context. Reading it here (rather than from the
                // callee constant's own, usually-empty `substs` field, or from
                // a freshly-interned substitution) is what lets generic
                // functions such as `id<T>` / `block_on<F: Future>` be
                // monomorphized, closing the single-await `TyKind::Error`
                // codegen gap.
                if let Operand::Constant(mir_const) = func {
                    if let MirConstKind::Fn(def_id, _) = &mir_const.kind {
                        let substs = match self.ty_ctx {
                            Some(ty_ctx) => match ty_ctx.ty_kind(mir_const.ty) {
                                TyKind::FnDef(_, s) => {
                                    if let Some(Some(arg0)) = args.first().map(|a| match a {
                                        Operand::Constant(c) => Some(c.ty),
                                        Operand::Copy(p) | Operand::Move(p) => body.locals.get(p.local).map(|d| d.ty),
                                    }) {
                                        let _ = ty_ctx.ty_kind(arg0);
                                    }
                                    // Guard against un-instantiated generic
                                    // calls: if the callee's `FnDef` substitution
                                    // still contains a `Param` (the type-checker
                                    // did not instantiate the call, e.g. an
                                    // `async fn` whose future type was left
                                    // generic), feeding it into monomorphization
                                    // corrupts the body with a leftover `Param`
                                    // that ICEs at codegen ("TyKind::Param
                                    // reached LLVM codegen"). Treat such calls
                                    // as monomorphic (empty substs) instead.
                                    let has_param = ty_ctx
                                        .substitution_args(*s)
                                        .iter()
                                        .any(|arg| {
                                            matches!(
                                                arg,
                                                &glyim_type::GenericArg::Ty(t)
                                                    if matches!(*ty_ctx.ty_kind(t), TyKind::Param(_))
                                            )
                                        });
                                    if has_param {
                                        Substitution::empty()
                                    } else {
                                        *s
                                    }
                                }
                                _ => Substitution::empty(),
                            },
                            // No type context (unit tests): `scan_const` below
                            // already enqueues `Fn` items with the constant's
                            // own (empty for monomorphic fns) substitution, so
                            // fall back to empty here.
                            None => Substitution::empty(),
                        };
                        self.enqueue(MonoItem::Fn { def_id: *def_id, substs });
                    }
                }
                self.scan_operand(func);
                for arg in args {
                    self.scan_operand(arg);
                }
            }
            TerminatorKind::Drop { place, .. } => {
                self.drop_locals.push(place.local);
            }
            TerminatorKind::SwitchInt { discr, .. } => self.scan_operand(discr),
            TerminatorKind::Assert { cond, .. } => self.scan_operand(cond),
            _ => {}
        }
    }

/// items.
    pub fn items(&self) -> &[MonoItemData] {
        self.items.as_slice()
    }

/// item_count.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

/// cache_len.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

/// lookup.
    pub fn lookup(&self, item: &MonoItem) -> Option<MonoItemId> {
        self.cache.get(item).copied()
    }

    /// Apply polymorphization to all collected mono items and deduplicate identical instantiations.
    ///
    /// This should be called after `collect` finishes and before codegen. It uses the
    /// `polymorphize` module (already present in `glyim-lower`) to analyse which generic
    /// parameters are actually used by each MIR body, shrink substitutions accordingly, and
    /// merge duplicate items so that codegen only sees unique instantiations.
    pub fn polymorphize_and_deduplicate(&mut self, ctx: &mut TyCtxMut) {
        if self.items.is_empty() {
            return;
        }
        let deduped = crate::polymorphize::deduplicate(ctx, self.items.as_slice());
        self.items = IndexVec::new();
        self.cache.clear();
        for data in deduped {
            let item = data.item.clone();
            let id = self.items.push(data);
            self.cache.insert(item, id);
        }
    }
}

impl<'a> Default for MonoCtx<'a> {
    fn default() -> Self {
        Self::new()
    }
}
