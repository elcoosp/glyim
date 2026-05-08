# The Glyim Typeck V3 Specification: The Demand-Driven Metacompiler

With your `glyim-query` crate as the backbone, the typechecker transforms from a linear pipeline into a **demand-driven metacompiler**. Every phase becomes a memoized, dependency-tracked query. Metaprogramming (staging, comptime, macros, reflection) fits naturally because comptime blocks are just queries that depend on other query results.

The key insight: **invalidation replaces recompilation**. When a source file changes, the `DependencyGraph` propagates red-status transitively, and only the affected queries re-execute.

---

## 1. Architectural Vision: The Query Stack

```
┌──────────────────────────────────────────────────────────────┐
│                     USER / LSP / JIT                         │
│           Queries type_of(), reflect_meta(), etc.            │
└──────────────────────────┬───────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────┐
│                      TyDatabase                               │
│   The query trait: defines all typechecker queries.          │
│   Each method is memoized via QueryContext.                  │
│                                                              │
│   Phase 1 (Registration):                                    │
│     nominal_type(id) → Ty                                    │
│     chr_rules(impl_id) → Vec<ChrRule>                       │
│     rep_of(id) → Rep                                         │
│     reflect_meta(id) → TypeMetaSoA                          │
│                                                              │
│   Phase 1.5 (Metaprogramming):                               │
│     eval_comptime(block_id) → ComptimeResult                │
│     expand_macro(call_id) → Arc<HirItem>                    │
│                                                              │
│   Phase 2 (Elaboration + Solving):                           │
│     typecheck_module(path) → ModuleTypeckResult             │
│                                                              │
│   Phase 3 (Freezing):                                        │
│     frozen_type(ty) → HirType                               │
│     monomorphize(fn_id, args) → MonoFn                      │
└──────────────────────────┬───────────────────────────────────┘
                           │ delegates to
┌──────────────────────────▼───────────────────────────────────┐
│                   QueryContext (glyim-query)                  │
│   Caching, dependency tracking, invalidation, persistence.   │
│   All query results are Arc<dyn Any> keyed by Fingerprint.  │
└──────────────────────────┬───────────────────────────────────┘
                           │ uses
┌──────────────────────────▼───────────────────────────────────┐
│              Shared Append-Only State                         │
│   TyArena (RwLock), Interner, MetaVm (Mutex)                │
│   These survive across queries; allocation is append-only.   │
└──────────────────────────────────────────────────────────────┘
```

---

## 2. Query Infrastructure (`queries/mod.rs`)

The bridge between `TyDatabase` and `glyim-query`. The core challenge: `glyim-query` requires explicit dependencies, but queries call other queries dynamically. We solve this with a **thread-local dependency collector**.

```rust
use glyim_query::{QueryContext, Dependency, Fingerprint};
use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;

// ─── Thread-Local Dependency Collector ───

thread_local! {
    static DEP_COLLECTOR: RefCell<Option<Vec<Dependency>>> = const { RefCell::new(None) };
}

/// Enter a dependency-recording scope. Returns all recorded deps on exit.
fn with_deps<F, R>(f: F) -> (R, Vec<Dependency>)
where
    F: FnOnce() -> R,
{
    DEP_COLLECTOR.with(|cell| {
        let old = cell.borrow_mut().replace(Vec::new());
        let result = f();
        let deps = cell.borrow_mut().take().unwrap();
        *cell.borrow_mut() = old;
        (result, deps)
    })
}

/// Record a dependency (called when one query invokes another).
fn record_dep(dep: Dependency) {
    DEP_COLLECTOR.with(|cell| {
        if let Some(deps) = cell.borrow_mut().as_mut() {
            deps.push(dep);
        }
    });
}

/// Record that the current query depends on a sub-query.
fn depend_on_query(key: Fingerprint) {
    record_dep(Dependency::query(key));
}

/// Record that the current query depends on a source file.
fn depend_on_file(path: &std::path::Path, hash: Fingerprint) {
    record_dep(Dependency::file(path, hash));
}
```

---

## 3. Query Key Definitions (`queries/keys.rs`)

Every query needs a `QueryKey` implementation. These are the identities by which results are cached and invalidated.

```rust
use glyim_query::{QueryKey, Fingerprint};
use glyim_interner::Symbol;
use std::path::PathBuf;

// ─── Phase 1: Registration ───

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NominalTypeKey {
    pub item_id: Symbol, // Fully-qualified item name
}

impl QueryKey for NominalTypeKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("nominal_type"),
            Fingerprint::of_str(&glyim_interner::resolve(self.item_id)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChrRulesKey {
    pub impl_id: Symbol,
}

impl QueryKey for ChrRulesKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("chr_rules"),
            Fingerprint::of_str(&glyim_interner::resolve(self.impl_id)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepForKey {
    pub type_id: Symbol,
}

impl QueryKey for RepForKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("rep_of"),
            Fingerprint::of_str(&glyim_interner::resolve(self.type_id)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReflectMetaKey {
    pub type_id: Symbol,
}

impl QueryKey for ReflectMetaKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("reflect_meta"),
            Fingerprint::of_str(&glyim_interner::resolve(self.type_id)),
        )
    }
}

// ─── Phase 1.5: Metaprogramming ───

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EvalComptimeKey {
    pub block_id: Symbol,      // Unique ID for the comptime block
    pub module: Symbol,        // Containing module
}

impl QueryKey for EvalComptimeKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine_all(&[
            Fingerprint::of_str("eval_comptime"),
            Fingerprint::of_str(&glyim_interner::resolve(self.block_id)),
            Fingerprint::of_str(&glyim_interner::resolve(self.module)),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExpandMacroKey {
    pub call_id: Symbol,
    pub macro_name: Symbol,
}

impl QueryKey for ExpandMacroKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine_all(&[
            Fingerprint::of_str("expand_macro"),
            Fingerprint::of_str(&glyim_interner::resolve(self.call_id)),
            Fingerprint::of_str(&glyim_interner::resolve(self.macro_name)),
        ])
    }
}

// ─── Phase 2: Elaboration + Solving ───

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypecheckModuleKey {
    pub path: PathBuf,
}

impl QueryKey for TypecheckModuleKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("typecheck_module"),
            Fingerprint::of_str(&self.path.to_string_lossy()),
        )
    }
}

// ─── Phase 3: Freezing ───

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrozenTypeKey {
    pub ty_id: u32, // The Ty arena index
}

impl QueryKey for FrozenTypeKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::combine(
            Fingerprint::of_str("frozen_type"),
            Fingerprint::of(&self.ty_id.to_le_bytes()),
        )
    }
}
```

---

## 4. The Database (`db.rs`)

The `TyDatabase` holds shared state and implements all queries by delegating to `QueryContext`.

```rust
use crate::queries::keys::*;
use crate::ty::{Ty, TyKind, TyArena};
use crate::chr::{ChrRule, Goal, ChrStore};
use crate::rep::Rep;
use crate::meta_vm::MetaVm;
use crate::freeze::{ModuleTypeckResult, HirType};
use glyim_query::{QueryContext, Fingerprint, Dependency, IncrementalState};
use glyim_interner::Interner;
use std::sync::{Arc, RwLock, Mutex};

pub struct TyDatabase {
    /// The query engine from glyim-query.
    ctx: QueryContext,
    /// Incremental state (wraps QueryContext + source tracking + persistence).
    incremental: IncrementalState,
    /// Append-only type arena. Shared across all queries.
    arena: RwLock<TyArena>,
    /// Symbol interner.
    interner: Interner,
    /// Metaprogramming VM. Mutex-protected because eval is stateful.
    vm: Mutex<MetaVm>,
    /// The parsed/lowered HIR, keyed by module path.
    hir_store: RwLock<Vec<Arc<crate::hir::Module>>>,
}

impl TyDatabase {
    pub fn new(cache_dir: &std::path::Path) -> Self {
        Self {
            ctx: QueryContext::new(),
            incremental: IncrementalState::load_or_create(cache_dir),
            arena: RwLock::new(TyArena::new()),
            interner: Interner::new(),
            vm: Mutex::new(MetaVm::new()),
            hir_store: RwLock::new(Vec::new()),
        }
    }

    // ═══════════════════════════════════════════════════════════
    //  Phase 1: Registration Queries
    // ═══════════════════════════════════════════════════════════

    /// Allocate a nominal type for the given item.
    /// Pure: just creates a Ty::Named or Ty::App in the arena.
    pub fn nominal_type(&self, key: NominalTypeKey) -> Ty {
        let fp = key.fingerprint();
        depend_on_query(fp);

        self.ctx.query(fp, || {
            let name = self.interner.resolve(key.item_id);
            let ty = self.arena.write().unwrap().alloc(TyKind::Named(key.item_id));
            Arc::new(ty) as Arc<dyn Any + Send + Sync>
        }, self.fingerprint_ty_result, vec![])
    }

    /// Extract CHR rules from an impl block.
    pub fn chr_rules(&self, key: ChrRulesKey) -> Arc<Vec<ChrRule>> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        self.ctx.query(fp, || {
            // Lower the impl to CHR rules (pure transformation)
            let rules = self.extract_chr_rules(key.impl_id);
            Arc::new(rules) as Arc<dyn Any + Send + Sync>
        }, /* value_fp */, vec![])
    }

    /// Build the generic representation (Rep) for a reflectable type.
    pub fn rep_of(&self, key: RepForKey) -> Arc<Rep> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        self.ctx.query(fp, || {
            let rep = self.build_rep(key.type_id);
            Arc::new(rep) as Arc<dyn Any + Send + Sync>
        }, /* value_fp */, vec![])
    }

    /// Generate SoA reflection metadata for a type.
    pub fn reflect_meta(&self, key: ReflectMetaKey) -> Arc<crate::reflect::TypeMetaSoA> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        // Depends on rep_of
        let rep_key = RepForKey { type_id: key.type_id };
        let rep = self.rep_of(rep_key);

        self.ctx.query(fp, || {
            let meta = crate::reflect::generate_soa_metadata(&rep, &self.interner);
            Arc::new(meta) as Arc<dyn Any + Send + Sync>
        }, /* value_fp */, vec![Dependency::query(RepForKey { type_id: key.type_id }.fingerprint())])
    }

    // ═══════════════════════════════════════════════════════════
    //  Phase 1.5: Metaprogramming Queries
    // ═══════════════════════════════════════════════════════════

    /// Evaluate a comptime block in the MetaVm.
    pub fn eval_comptime(&self, key: EvalComptimeKey) -> Arc<crate::meta_vm::ComptimeResult> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        // The comptime block depends on the types it references.
        // The VM will call back into the database for type info,
        // recording dependencies automatically via with_deps.
        let (result, deps) = with_deps(|| {
            let mut vm = self.vm.lock().unwrap();
            vm.eval_block(key.block_id, key.module, self)
        });

        let value_fp = Fingerprint::of_str(&format!("{:?}", result));
        self.ctx.insert(fp, Arc::new(result), value_fp, deps);

        // Retrieve from cache for the return value
        self.ctx.get(&fp).unwrap().value.downcast_ref::<Arc<crate::meta_vm::ComptimeResult>>().unwrap().clone()
    }

    /// Expand a macro call.
    pub fn expand_macro(&self, key: ExpandMacroKey) -> Arc<crate::hir::HirItem> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        let (result, deps) = with_deps(|| {
            let mut vm = self.vm.lock().unwrap();
            vm.expand_macro(key.macro_name, key.call_id, self)
        });

        let value_fp = Fingerprint::of_str(&format!("{:?}", result));
        self.ctx.insert(fp, Arc::new(result), value_fp, deps);
        self.ctx.get(&fp).unwrap().value.downcast_ref::<Arc<crate::hir::HirItem>>().unwrap().clone()
    }

    // ═══════════════════════════════════════════════════════════
    //  Phase 2: Elaboration + Solving
    // ═══════════════════════════════════════════════════════════

    /// Typecheck an entire module. This is the main orchestrator.
    /// It creates local UnificationTable and ChrStore, elaborates
    /// all items, solves goals, and returns the final types.
    pub fn typecheck_module(&self, key: TypecheckModuleKey) -> Arc<ModuleTypeckResult> {
        let fp = key.fingerprint();
        depend_on_query(fp);

        let (result, deps) = with_deps(|| {
            // 1. Parse & lower (sub-queries, dependencies recorded automatically)
            let hir = self.parse_and_lower(&key.path);

            // 2. Register types and CHR rules (sub-queries)
            let mut type_map = std::collections::HashMap::new();
            let mut chr_rules = Vec::new();
            for item in &hir.items {
                match item {
                    crate::hir::HirItem::Struct(def) => {
                        let nkey = NominalTypeKey { item_id: def.name };
                        let ty = self.nominal_type(nkey);
                        type_map.insert(def.name, ty);
                    }
                    crate::hir::HirItem::Impl(def) => {
                        let rkey = ChrRulesKey { impl_id: def.name };
                        let rules = self.chr_rules(rkey);
                        chr_rules.extend(rules.iter().cloned());
                    }
                    _ => {}
                }
            }

            // 3. Create local solver state
            let mut unification = crate::unify::UnificationTable::new();
            let mut chr_store = crate::chr::ChrStore::new(chr_rules);

            // 4. Elaborate all items (including comptime blocks)
            let mut elab = crate::elab::ElabContext::new(
                self, &hir, &mut unification, &mut chr_store,
            );

            for item in &hir.items {
                elab.elaborate_item(item);
            }

            // 5. Solve all pending goals
            chr_store.solve(&mut *self.vm.lock().unwrap(), &self.arena.read().unwrap())?;

            // 6. Freeze
            let result = crate::freeze::freeze_module(
                &self.arena.read().unwrap(),
                &unification,
                &chr_store,
                &elab.expr_types,
            );

            Ok(result)
        });

        match result {
            Ok(module_result) => {
                let value_fp = Fingerprint::of_str(&format!("{:?}", module_result));
                self.ctx.insert(fp, Arc::new(module_result.clone()), value_fp, deps);
                Arc::new(module_result)
            }
            Err(e) => {
                // Store error result
                self.ctx.insert(fp, Arc::new(e), Fingerprint::ZERO, deps);
                Arc::new(ModuleTypeckResult::with_errors())
            }
        }
    }

    // ═══════════════════════════════════════════════════════════
    //  Phase 3: Freezing
    // ═══════════════════════════════════════════════════════════

    pub fn frozen_type(&self, key: FrozenTypeKey) -> HirType {
        let fp = key.fingerprint();
        depend_on_query(fp);

        self.ctx.query(fp, || {
            let arena = self.arena.read().unwrap();
            let ty = Ty(key.ty_id);
            let hir_type = crate::freeze::resolve_ty(&arena, ty);
            Arc::new(hir_type) as Arc<dyn Any + Send + Sync>
        }, /* value_fp */, vec![])
    }

    // ═══════════════════════════════════════════════════════════
    //  Incremental: Source Change Processing
    // ═══════════════════════════════════════════════════════════

    /// Called when source files change. Invalidates affected queries.
    pub fn apply_source_changes(&self, changes: &[(&str, Fingerprint)]) {
        let report = self.incremental.apply_changes(changes);
        tracing::info!(
            "Invalidation: {} red, {} green",
            report.red_count(),
            report.green_count()
        );
    }

    /// Persist incremental state to disk.
    pub fn save_incremental(&self) -> Result<(), String> {
        self.incremental.save()
    }

    // ─── Helpers ───

    fn fingerprint_ty_result(&self, ty: &Ty) -> Fingerprint {
        let arena = self.arena.read().unwrap();
        crate::fingerprint_ty(&arena, *ty)
    }
}

/// Compute a Fingerprint for a Ty (for caching query results).
fn fingerprint_ty(arena: &TyArena, ty: Ty) -> Fingerprint {
    match arena.get(ty) {
        TyKind::Int => Fingerprint::of_str("Int"),
        TyKind::Float => Fingerprint::of_str("Float"),
        TyKind::Bool => Fingerprint::of_str("Bool"),
        TyKind::Named(sym) => Fingerprint::combine(
            Fingerprint::of_str("Named"),
            Fingerprint::of_str(&glyim_interner::resolve(*sym)),
        ),
        TyKind::App(sym, args) => {
            let mut fps = vec![
                Fingerprint::of_str("App"),
                Fingerprint::of_str(&glyim_interner::resolve(*sym)),
            ];
            for arg in args {
                fps.push(fingerprint_ty(arena, *arg));
            }
            Fingerprint::combine_all(&fps)
        }
        TyKind::Code(inner) => Fingerprint::combine(
            Fingerprint::of_str("Code"),
            fingerprint_ty(arena, *inner),
        ),
        TyKind::Const(ty, val_id) => Fingerprint::combine_all(&[
            Fingerprint::of_str("Const"),
            fingerprint_ty(arena, *ty),
            Fingerprint::of(&val_id.0.to_le_bytes()),
        ]),
        TyKind::Fn(params, ret) => {
            let mut fps = vec![Fingerprint::of_str("Fn")];
            for p in params {
                fps.push(fingerprint_ty(arena, *p));
            }
            fps.push(fingerprint_ty(arena, *ret));
            Fingerprint::combine_all(&fps)
        }
        _ => Fingerprint::of_str(&format!("{:?}", arena.get(ty))),
    }
}
```

---

## 5. Core Data Layer (`ty.rs`)

Extended for staging, const generics, and effects.

```rust
use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ty(pub usize);

/// Reference to a comptime value in the MetaVm heap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValueId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyKind {
    // ─── Primitives ───
    Int, Float, Bool, Str, Unit, Never,
    Error, Infer,

    // ─── Nominal Types ───
    Named(Symbol),
    App(Symbol, Vec<Ty>),
    Fn(Vec<Ty>, Ty),
    RawPtr(Ty),

    // ─── V3: Staging ───
    /// Code<T>: A quoted expression of type T at a future stage.
    Code(Ty),

    // ─── V3: Const Generics ───
    /// Const<T, V>: A const value V of type T.
    /// e.g., Array<i64, Const(usize, 16)>
    Const(Ty, ValueId),

    // ─── V3: Effects ───
    /// EffectFn(params, ret, effects): A function with tracked effects.
    EffectFn(Vec<Ty>, Ty, EffectRow),

    // ─── V3: Reflection ───
    /// The top type for dynamic reflection.
    Any,
    /// TypeInfo<T>: Reified type descriptor.
    TypeInfo(Ty),
}

/// Effect row: tracks which algebraic effects a function may perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffectRow {
    Empty,
    Extend(Symbol, Box<EffectRow>),
    /// Unification variable for effects (like Ty::Infer, but for rows).
    Var(u32),
}

pub struct TyArena {
    kinds: Vec<TyKind>,
    infer_spans: Vec<Span>,
}

impl TyArena {
    pub fn new() -> Self {
        Self {
            kinds: Vec::new(),
            infer_spans: Vec::new(),
        }
    }

    pub fn alloc(&mut self, kind: TyKind) -> Ty {
        let id = self.kinds.len();
        self.kinds.push(kind);
        Ty(id)
    }

    pub fn fresh_infer(&mut self, span: Span) -> Ty {
        let id = self.kinds.len();
        self.infer_spans.push(span);
        self.kinds.push(TyKind::Infer);
        Ty(id)
    }

    pub fn get(&self, ty: Ty) -> &TyKind {
        &self.kinds[ty.0]
    }

    pub fn get_infer_span(&self, ty: Ty) -> Option<Span> {
        if matches!(self.get(ty), TyKind::Infer) {
            self.infer_spans.get(ty.0).copied()
        } else {
            None
        }
    }
}
```

---

## 6. The CHR Solver (`chr.rs`)

Extended for metaprogramming goals. The solver now interacts with the `MetaVm` for const evaluation and macro expansion.

```rust
use crate::ty::{Ty, ValueId};
use crate::meta_vm::MetaVm;
use crate::ty::TyArena;
use glyim_interner::Symbol;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Goal {
    // ─── V2 Goals ───
    TraitImpl(Symbol, Vec<Ty>),
    StateTransition(Symbol, Ty, Ty),
    Reflectable(Ty),
    HasField(Ty, Symbol),
    IsoCoerce(Ty, Ty),

    // ─── V3: Const Generics ───
    /// Prove that two const values are equal.
    ConstEq(ValueId, ValueId),

    // ─── V3: Effects ───
    /// Prove that an effect row is satisfied.
    SatisfiesEffect(Ty, crate::ty::EffectRow),

    // ─── V3: Metaprogramming ───
    /// Prove that a macro can be expanded (triggers VM).
    MacroExpand(Symbol, Vec<ValueId>),

    /// Prove that a comptime computation succeeds.
    ComptimeSucceeds(Symbol),
}

#[derive(Clone, Debug)]
pub enum ChrRule {
    Simplify { goal: Goal, premises: Vec<Goal> },
    Propagate { goal: Goal, premises: Vec<Goal>, new_goals: Vec<Goal> },
}

pub struct ChrStore {
    rules: Vec<ChrRule>,
    pending_goals: Vec<Goal>,
    proven_goals: HashSet<Goal>,
}

impl ChrStore {
    pub fn new(rules: Vec<ChrRule>) -> Self {
        Self {
            rules,
            pending_goals: Vec::new(),
            proven_goals: HashSet::new(),
        }
    }

    /// Solve to fixed point. The VM reference allows the solver
    /// to evaluate comptime expressions during solving.
    pub fn solve(
        &mut self,
        vm: &mut MetaVm,
        arena: &TyArena,
    ) -> Result<(), ErrorGuaranteed> {
        while let Some(goal) = self.pending_goals.pop() {
            if self.proven_goals.contains(&goal) {
                continue;
            }

            let mut rule_matched = false;
            for rule in &self.rules {
                if rule.matches(&goal) {
                    rule_matched = true;
                    // Apply rule: mark proven or push premises
                    match rule {
                        ChrRule::Simplify { premises, .. } if premises.iter().all(|p| self.proven_goals.contains(p)) => {
                            self.proven_goals.insert(goal.clone());
                        }
                        ChrRule::Simplify { premises, .. } => {
                            self.pending_goals.push(goal.clone());
                            self.pending_goals.extend(premises.iter().cloned());
                        }
                        ChrRule::Propagate { new_goals, premises, .. } if premises.iter().all(|p| self.proven_goals.contains(p)) => {
                            self.proven_goals.insert(goal.clone());
                            self.pending_goals.extend(new_goals.iter().cloned());
                        }
                        ChrRule::Propagate { premises, .. } => {
                            self.pending_goals.push(goal.clone());
                            self.pending_goals.extend(premises.iter().cloned());
                        }
                    }
                    break;
                }
            }

            // Special handling for goals that require VM execution
            if !rule_matched {
                match &goal {
                    Goal::ConstEq(a, b) => {
                        if vm.values_equal(*a, *b) {
                            self.proven_goals.insert(goal);
                            rule_matched = true;
                        }
                    }
                    Goal::MacroExpand(name, args) => {
                        // The VM will expand the macro; we just need to
                        // verify it doesn't fail.
                        self.proven_goals.insert(goal);
                        rule_matched = true;
                    }
                    Goal::ComptimeSucceeds(block_id) => {
                        // The comptime block either succeeds or fails.
                        // If we got here, it succeeded.
                        self.proven_goals.insert(goal);
                        rule_matched = true;
                    }
                    _ => {}
                }
            }

            if !rule_matched {
                // No rule fired. Error.
                return Err(ErrorGuaranteed::new());
            }
        }
        Ok(())
    }
}
```

---

## 7. The Elaborator (`elab/mod.rs`)

Now query-aware. When it encounters a comptime block, it calls `db.eval_comptime()`, which is a cached query.

```rust
use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::chr::{ChrStore, Goal};
use crate::meta_vm::ComptimeResult;
use crate::staging::Level;
use crate::db::TyDatabase;
use crate::hir::HirExpr;
use glyim_diag::Span;
use glyim_interner::Symbol;

pub struct ElabContext<'a> {
    db: &'a TyDatabase,
    hir: &'a crate::hir::Module,
    arena: &'a TyArena,
    unification: &'a mut UnificationTable,
    chr_store: &'a mut ChrStore,
    /// Current staging level (tracked for phase consistency).
    current_level: Level,
    /// Types assigned to each expression.
    pub expr_types: Vec<Ty>,
    /// Errors accumulated during elaboration.
    pub errors: Vec<crate::diagnostics::TypeError>,
    /// Generated items from comptime blocks, awaiting elaboration.
    pub generated_items: Vec<crate::hir::HirItem>,
}

impl<'a> ElabContext<'a> {
    pub fn new(
        db: &'a TyDatabase,
        hir: &'a crate::hir::Module,
        unification: &'a mut UnificationTable,
        chr_store: &'a mut ChrStore,
    ) -> Self {
        Self {
            db,
            hir,
            arena,
            unification,
            chr_store,
            current_level: Level::Runtime,
            expr_types: Vec::new(),
            errors: Vec::new(),
            generated_items: Vec::new(),
        }
    }

    /// Elaborate a single item.
    pub fn elaborate_item(&mut self, item: &crate::hir::HirItem) {
        match item {
            crate::hir::HirItem::Fn(def) => {
                let body_ty = self.synth_expr(&def.body);
                // Unify with declared return type
                // ...
            }
            crate::hir::HirItem::ComptimeBlock(block) => {
                self.elaborate_comptime(block);
            }
            _ => {}
        }
    }

    /// Elaborate a comptime block by calling the database query.
    fn elaborate_comptime(&mut self, block: &crate::hir::ComptimeBlock) {
        use crate::queries::keys::EvalComptimeKey;

        let key = EvalComptimeKey {
            block_id: block.id,
            module: self.hir.name,
        };

        // This is a query! If the block's dependencies haven't changed,
        // the cached result is returned instantly.
        let result = self.db.eval_comptime(key);

        match result.as_ref() {
            ComptimeResult::Success { generated_items } => {
                // Inject generated items for further elaboration
                for item in generated_items {
                    self.generated_items.push(item.clone());
                }
            }
            ComptimeResult::Error(err) => {
                self.errors.push(err.clone());
            }
        }
    }

    /// Synth mode: figure out the type from the expression.
    pub fn synth_expr(&mut self, expr: &HirExpr) -> Ty {
        match expr {
            // ─── V3: Staging ───
            HirExpr::Quote { body, span } => {
                let prev_level = self.current_level;
                self.current_level = Level::next(prev_level);
                let inner_ty = self.synth_expr(body);
                self.current_level = prev_level;
                self.arena.alloc(TyKind::Code(inner_ty))
            }

            HirExpr::Splice { expr, span } => {
                let code_ty = self.synth_expr(expr);
                match self.arena.get(code_ty) {
                    TyKind::Code(inner_ty) => *inner_ty,
                    _ => {
                        self.errors.push(crate::diagnostics::TypeError::PhaseViolation {
                            span: *span,
                            used_at: self.current_level,
                            defined_at: self.current_level,
                        });
                        self.arena.alloc(TyKind::Error)
                    }
                }
            }

            // ─── V3: Const Generics ───
            HirExpr::ConstGeneric { value, ty, span } => {
                // Evaluate the value in the VM
                let val_id = self.db.vm.lock().unwrap().eval_const(value);
                self.arena.alloc(TyKind::Const(*ty, val_id))
            }

            // ─── V3: Reflection ───
            HirExpr::TypeOf { expr, span } => {
                let inner_ty = self.synth_expr(expr);
                self.chr_store.pending_goals.push(Goal::Reflectable(inner_ty));
                self.arena.alloc(TyKind::TypeInfo(inner_ty))
            }

            HirExpr::ReflectGet { receiver, field_name, span } => {
                let receiver_ty = self.synth_expr(receiver)?;
                let result_ty = self.unification.new_var(self.arena, *span);
                self.chr_store.pending_goals.push(Goal::HasField(receiver_ty, *field_name));

                // If receiver type is concretely known, synthesize result type
                if let TyKind::App(sym, _) = self.arena.get(receiver_ty) {
                    if let Some(field_ty) = self.lookup_field_type(*sym, *field_name) {
                        self.unification.unify(
                            self.arena, result_ty, field_ty, *span,
                            &mut |e| self.errors.push(e),
                        ).ok();
                    }
                }
                result_ty
            }

            // ─── Standard expressions (V2) ───
            HirExpr::IntLit { .. } => self.arena.alloc(TyKind::Int),
            HirExpr::MethodCall { receiver, method_name, args, span, .. } => {
                let receiver_ty = self.synth_expr(receiver)?;
                // Type-state transition (V2)
                if let TyKind::App(_, _) = self.arena.get(receiver_ty) {
                    let transition_goal = Goal::StateTransition(
                        *method_name,
                        receiver_ty,
                        self.unification.new_var(self.arena, *span),
                    );
                    self.chr_store.pending_goals.push(transition_goal);
                }
                // ... standard method lookup ...
                todo!()
            }

            _ => todo!(),
        }
    }
}
```

---

## 8. The Metaprogramming VM (`meta_vm.rs`)

The bytecode interpreter that executes comptime blocks and macro expansions. It calls back into `TyDatabase` for type information, creating query dependencies automatically.

```rust
use crate::ty::{Ty, TyKind, ValueId};
use crate::db::TyDatabase;
use crate::queries::keys::*;
use glyim_interner::Symbol;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of a comptime block evaluation.
#[derive(Clone, Debug)]
pub enum ComptimeResult {
    Success {
        generated_items: Vec<crate::hir::HirItem>,
    },
    Error(crate::diagnostics::TypeError),
}

/// Values in the VM heap.
#[derive(Clone, Debug)]
pub enum MetaValue {
    Integer(i64),
    Bool(bool),
    String(String),
    Type(Ty),
    AstFragment(Arc<crate::hir::HirItem>),
    Rep(Arc<crate::rep::Rep>),
    Optic(Arc<crate::reflect::Optic>),
    Unit,
}

pub struct MetaVm {
    /// Heap of comptime values.
    heap: Vec<MetaValue>,
    /// Content-addressable cache: InputFingerprint → ValueId.
    cache: HashMap<glyim_query::Fingerprint, ValueId>,
}

impl MetaVm {
    pub fn new() -> Self {
        Self {
            heap: Vec::new(),
            cache: HashMap::new(),
        }
    }

    /// Evaluate a comptime block. This is called from the
    /// eval_comptime query in TyDatabase.
    pub fn eval_block(
        &mut self,
        block_id: Symbol,
        module: Symbol,
        db: &TyDatabase,
    ) -> ComptimeResult {
        // 1. Get the HIR for the comptime block
        //    (This records a query dependency automatically)
        let hir = db.lower_comptime_block(block_id, module);

        // 2. Execute the block in the interpreter
        let mut interp = VmInterpreter::new(self, db);
        match interp.exec_block(&hir) {
            Ok(result) => result,
            Err(e) => ComptimeResult::Error(e),
        }
    }

    /// Expand a macro call.
    pub fn expand_macro(
        &mut self,
        macro_name: Symbol,
        call_id: Symbol,
        db: &TyDatabase,
    ) -> Arc<crate::hir::HirItem> {
        // Similar to eval_block but returns a single AST item
        todo!()
    }

    /// Evaluate a const expression to a ValueId.
    pub fn eval_const(&mut self, expr: &crate::hir::HirExpr) -> ValueId {
        // Evaluate pure const expressions (no IO, no side effects)
        todo!()
    }

    /// Check if two comptime values are equal.
    pub fn values_equal(&self, a: ValueId, b: ValueId) -> bool {
        self.heap.get(a.0) == self.heap.get(b.0)
    }

    /// Allocate a value on the heap.
    pub fn alloc(&mut self, value: MetaValue) -> ValueId {
        let id = ValueId(self.heap.len());
        self.heap.push(value);
        id
    }
}

/// The interpreter that walks comptime bytecode.
/// Holds a reference to the database so it can query type info
/// (recording dependencies automatically).
struct VmInterpreter<'a> {
    vm: &'a mut MetaVm,
    db: &'a TyDatabase,
}

impl<'a> VmInterpreter<'a> {
    fn exec_block(&mut self, block: &crate::hir::ComptimeBlock) -> Result<ComptimeResult, crate::diagnostics::TypeError> {
        for stmt in &block.stmts {
            match stmt {
                crate::hir::ComptimeStmt::Reflect { type_name } => {
                    // Query the database for Rep. This creates a dependency!
                    let rep_key = RepForKey { type_id: *type_name };
                    let rep = self.db.rep_of(rep_key);
                    // ... use rep ...
                }
                crate::hir::ComptimeStmt::Generate { template } => {
                    // Generate HIR items from the template
                    // ...
                }
                crate::hir::ComptimeStmt::Emit { item } => {
                    // Emit a generated item
                    // ...
                }
            }
        }
        Ok(ComptimeResult::Success {
            generated_items: vec![], // populated during execution
        })
    }
}
```

---

## 9. The Phase Checker (`staging.rs`)

Enforces level consistency so you never use a runtime value where a compile-time one is required.

```rust
use glyim_diag::Span;

/// The stage at which an expression exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Level 0: Pure compile-time evaluation.
    Comptime,
    /// Level 1: Build-time (hermetic IO).
    Buildtime,
    /// Level 2: Normal runtime execution.
    Runtime,
}

impl Level {
    /// Return the next (higher) stage.
    pub fn next(self) -> Self {
        match self {
            Level::Comptime => Level::Buildtime,
            Level::Buildtime => Level::Runtime,
            Level::Runtime => Level::Runtime,
        }
    }
}

/// Check phase consistency: an expression at level L can only
/// reference values at level ≤ L.
pub fn check_phase_consistency(
    use_level: Level,
    def_level: Level,
    span: Span,
) -> Result<(), crate::diagnostics::TypeError> {
    if def_level > use_level {
        Err(crate::diagnostics::TypeError::PhaseViolation {
            span,
            used_at: use_level,
            defined_at: def_level,
        })
    } else {
        Ok(())
    }
}
```

---

## 10. Diagnostics (`diagnostics/mod.rs`)

Extended with phase violations and const mismatch errors.

```rust
use glyim_diag::Span;
use crate::staging::Level;

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum TypeError {
    // ─── V2 Errors ───
    #[error("infinite type detected")]
    #[diagnostic(code(glyim::infinite_type))]
    InfiniteType { #[label] span: Span },

    #[error("type mismatch")]
    #[diagnostic(code(glyim::mismatch))]
    MismatchedTypes {
        #[label("expected {expected}")]
        expected_span: Span,
        #[label("found {found}")]
        found_span: Span,
        expected: String,
        found: String,
        #[help]
        diff_path: Option<String>,
        autofix: Option<AutoFix>,
    },

    // ─── V3: Staging Errors ───
    #[error("phase violation: cannot use {defined_at:?} value at {used_at:?} stage")]
    #[diagnostic(code(glyim::phase_violation))]
    #[diagnostic(help("values can only be used at the same stage or a later stage"))]
    PhaseViolation {
        #[label]
        span: Span,
        used_at: Level,
        defined_at: Level,
    },

    // ─── V3: Const Generic Errors ───
    #[error("const generic mismatch: expected {expected}, found {found}")]
    #[diagnostic(code(glyim::const_mismatch))]
    ConstMismatch {
        #[label]
        span: Span,
        expected: String,
        found: String,
    },

    // ─── V3: Comptime Errors ───
    #[error("comptime evaluation failed: {message}")]
    #[diagnostic(code(glyim::comptime_error))]
    ComptimeError {
        #[label]
        span: Span,
        message: String,
    },

    // ─── V3: Macro Errors ───
    #[error("macro expansion failed: {message}")]
    #[diagnostic(code(glyim::macro_error))]
    MacroError {
        #[label]
        span: Span,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub enum AutoFix {
    WrapWithOptions(Span),
    WrapWithOk(Span),
    TakeAddress(Span),
    InsertSplice(Span),  // V3: Wrap expression in $(...)
    LiftToComptime(Span), // V3: Mark expression as comptime
}
```

---

## 11. The Freeze Phase (`freeze.rs`)

Extended for specialization: only emit metadata for fields that are actually used reflectively.

```rust
use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::chr::{ChrStore, Goal};
use crate::reflect::TypeMetaSoA;
use std::collections::HashMap;

pub struct ModuleTypeckResult {
    pub expr_types: HashMap<u32, HirType>,
    pub reflect_descriptors: HashMap<Symbol, TypeMetaSoA>,
    pub optic_tables: Vec<OpticDispatchTable>,
    pub errors: Vec<crate::diagnostics::TypeError>,
    /// Cache manifest: fingerprints of all comptime blocks that
    /// succeeded, for incremental recompilation.
    pub meta_cache_manifest: HashMap<glyim_query::Fingerprint, Symbol>,
    /// Specialized reflection metadata (only fields actually used).
    pub specialized_reflect: HashMap<Symbol, TypeMetaSoA>,
}

impl ModuleTypeckResult {
    pub fn with_errors() -> Self {
        Self {
            expr_types: HashMap::new(),
            reflect_descriptors: HashMap::new(),
            optic_tables: Vec::new(),
            errors: Vec::new(),
            meta_cache_manifest: HashMap::new(),
            specialized_reflect: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HirType {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Never,
    Error,
    Named(String),
    Generic(String, Vec<HirType>),
    Fn(Vec<HirType>, Box<HirType>),
    RawPtr(Box<HirType>),
    Code(Box<HirType>),
    Any,
}

/// Freeze a module: resolve all inference variables and produce
/// the final output for monomorphization.
pub fn freeze_module(
    arena: &TyArena,
    unification: &UnificationTable,
    chr_store: &ChrStore,
    expr_types: &[Ty],
) -> ModuleTypeckResult {
    // 1. Resolve all expression types
    let frozen_expr_types: HashMap<u32, HirType> = expr_types
        .iter()
        .enumerate()
        .map(|(i, ty)| (i as u32, resolve_ty(arena, unification, *ty)))
        .collect();

    // 2. Analyze which reflective operations were actually used
    let reflect_usage = analyze_reflect_usage(chr_store);

    // 3. Generate specialized metadata (only for used fields)
    let specialized_reflect = generate_specialized_metadata(
        arena, &reflect_usage, chr_store,
    );

    ModuleTypeckResult {
        expr_types: frozen_expr_types,
        reflect_descriptors: HashMap::new(), // populated from Phase 1 queries
        optic_tables: Vec::new(),
        errors: Vec::new(),
        meta_cache_manifest: HashMap::new(),
        specialized_reflect,
    }
}

/// Resolve a Ty to a concrete HirType.
pub fn resolve_ty(arena: &TyArena, unification: &UnificationTable, ty: Ty) -> HirType {
    let ty = unification.find(arena, ty);
    match arena.get(ty) {
        TyKind::Int => HirType::Int,
        TyKind::Float => HirType::Float,
        TyKind::Bool => HirType::Bool,
        TyKind::Str => HirType::Str,
        TyKind::Unit => HirType::Unit,
        TyKind::Never => HirType::Never,
        TyKind::Error => HirType::Error,
        TyKind::Infer => HirType::Error, // Unresolved hole
        TyKind::Named(sym) => HirType::Named(glyim_interner::resolve(*sym).to_string()),
        TyKind::App(sym, args) => HirType::Generic(
            glyim_interner::resolve(*sym).to_string(),
            args.iter().map(|a| resolve_ty(arena, unification, *a)).collect(),
        ),
        TyKind::Code(inner) => HirType::Code(Box::new(resolve_ty(arena, unification, *inner))),
        TyKind::Any => HirType::Any,
        TyKind::Fn(params, ret) => HirType::Fn(
            params.iter().map(|p| resolve_ty(arena, unification, *p)).collect(),
            Box::new(resolve_ty(arena, unification, *ret)),
        ),
        TyKind::RawPtr(inner) => HirType::RawPtr(Box::new(resolve_ty(arena, unification, *inner))),
        TyKind::Const(_, _) => HirType::Error, // Should be resolved by now
        TyKind::TypeInfo(inner) => resolve_ty(arena, unification, *inner),
        TyKind::EffectFn(params, ret, _) => HirType::Fn(
            params.iter().map(|p| resolve_ty(arena, unification, *p)).collect(),
            Box::new(resolve_ty(arena, unification, *ret)),
        ),
    }
}

/// Resolve without unification (for query results where unification is done).
pub fn resolve_ty_simple(arena: &TyArena, ty: Ty) -> HirType {
    match arena.get(ty) {
        TyKind::Int => HirType::Int,
        TyKind::Named(sym) => HirType::Named(glyim_interner::resolve(*sym).to_string()),
        TyKind::App(sym, args) => HirType::Generic(
            glyim_interner::resolve(*sym).to_string(),
            args.iter().map(|a| resolve_ty_simple(arena, *a)).collect(),
        ),
        TyKind::Code(inner) => HirType::Code(Box::new(resolve_ty_simple(arena, *inner))),
        TyKind::Any => HirType::Any,
        _ => HirType::Error,
    }
}

/// Analyze which reflective goals were proven to determine
/// what metadata we actually need to emit.
fn analyze_reflect_usage(chr_store: &ChrStore) -> HashMap<Symbol, HashSet<Symbol>> {
    let mut usage: HashMap<Symbol, HashSet<Symbol>> = HashMap::new();
    for goal in &chr_store.proven_goals {
        match goal {
            Goal::HasField(ty, field_name) => {
                if let TyKind::Named(sym) | TyKind::App(sym, _) = arena.get(*ty) {
                    usage.entry(*sym).or_default().insert(*field_name);
                }
            }
            Goal::Reflectable(ty) => {
                if let TyKind::Named(sym) | TyKind::App(sym, _) = arena.get(*ty) {
                    usage.entry(*sym).or_default();
                }
            }
            _ => {}
        }
    }
    usage
}
```

---

## 12. Reflection (`reflect/mod.rs`)

SoA metadata and optics generation, driven by `Rep` and query results.

```rust
use crate::rep::Rep;
use crate::ty::Ty;
use glyim_interner::{Interner, Symbol};
use std::collections::HashSet;

/// SoA-style type descriptor. Stored in .rodata.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TypeMetaSoA {
    pub type_id: u32,
    pub field_count: u32,
    pub name_hashes: Vec<u64>,
    pub offsets: Vec<usize>,
    pub type_ids: Vec<u32>,
    pub getters: Vec<usize>,  // Function pointers (offsets into code section)
    pub mph_seed: u64,        // Seed for minimal perfect hash
}

/// Monomorphized optic (function pointer table).
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Optic<S, A> {
    pub get: unsafe fn(*const S) -> *const A,
    pub set: unsafe fn(*mut S, A),
}

/// A dispatch table for optics, stored in .rodata.
#[derive(Debug, Clone)]
pub struct OpticDispatchTable {
    pub type_name: Symbol,
    pub optics: Vec<(Symbol, usize)>, // (field_name, offset to Optic<S,A>)
}

/// Generate SoA metadata from a Rep structure.
pub fn generate_soa_metadata(rep: &Rep, interner: &Interner) -> TypeMetaSoA {
    let fields = rep.collect_fields();
    let field_count = fields.len() as u32;

    TypeMetaSoA {
        type_id: rep.type_id(),
        field_count,
        name_hashes: fields.iter().map(|f| f.name_hash(interner)).collect(),
        offsets: fields.iter().map(|f| f.byte_offset()).collect(),
        type_ids: fields.iter().map(|f| f.type_id()).collect(),
        getters: fields.iter().map(|f| f.getter_offset()).collect(),
        mph_seed: compute_mph_seed(&fields, interner),
    }
}

/// Generate specialized metadata that only includes the fields
/// that are actually used reflectively.
pub fn generate_specialized_metadata(
    full_meta: &TypeMetaSoA,
    used_fields: &HashSet<Symbol>,
    interner: &Interner,
) -> TypeMetaSoA {
    // Filter to only include used fields
    let mut filtered_hashes = Vec::new();
    let mut filtered_offsets = Vec::new();
    let mut filtered_type_ids = Vec::new();
    let mut filtered_getters = Vec::new();

    for (i, field_name) in full_meta.field_names(interner).iter().enumerate() {
        let sym = interner.intern(field_name);
        if used_fields.contains(&sym) {
            filtered_hashes.push(full_meta.name_hashes[i]);
            filtered_offsets.push(full_meta.offsets[i]);
            filtered_type_ids.push(full_meta.type_ids[i]);
            filtered_getters.push(full_meta.getters[i]);
        }
    }

    TypeMetaSoA {
        field_count: filtered_hashes.len() as u32,
        name_hashes: filtered_hashes,
        offsets: filtered_offsets,
        type_ids: filtered_type_ids,
        getters: filtered_getters,
        mph_seed: compute_mph_seed_from_hashes(&filtered_hashes),
        ..*full_meta
    }
}

fn compute_mph_seed(fields: &[FieldInfo], interner: &Interner) -> u64 {
    // Use CHD or Brz algorithm to find a seed that produces
    // a minimal perfect hash over the field name hashes.
    todo!()
}

fn compute_mph_seed_from_hashes(hashes: &[u64]) -> u64 {
    todo!()
}
```

---

## 13. Generic Representation (`rep.rs`)

GHC-style generic representation, the core IR for reflection.

```rust
use crate::ty::Ty;
use glyim_interner::Symbol;

/// The generic representation of a type.
/// Any @reflectable type is automatically converted to this.
#[derive(Debug, Clone)]
pub enum Rep {
    /// Datatype metadata (name, module, annotations)
    Meta(RepMeta, Box<Rep>),
    /// Sum type (choice between constructors)
    Sum(Box<Rep>, Box<Rep>),
    /// Product type (multiple fields)
    Product(Box<Rep>, Box<Rep>),
    /// Constructor metadata + contents
    Constructor(RepMeta, Box<Rep>),
    /// Field metadata + type
    Field(RepMeta, Ty),
    /// No fields (unit constructor)
    Unit,
}

#[derive(Debug, Clone)]
pub struct RepMeta {
    pub name: Symbol,
    pub annotations: Vec<Symbol>,
}

impl Rep {
    /// Collect all fields from this Rep (flattened).
    pub fn collect_fields(&self) -> Vec<FieldInfo> {
        let mut fields = Vec::new();
        self.collect_fields_into(&mut fields);
        fields
    }

    fn collect_fields_into(&self, fields: &mut Vec<FieldInfo>) {
        match self {
            Rep::Meta(_, inner) => inner.collect_fields_into(fields),
            Rep::Sum(l, r) => {
                l.collect_fields_into(fields);
                r.collect_fields_into(fields);
            }
            Rep::Product(l, r) => {
                l.collect_fields_into(fields);
                r.collect_fields_into(fields);
            }
            Rep::Constructor(_, inner) => inner.collect_fields_into(fields),
            Rep::Field(meta, ty) => {
                fields.push(FieldInfo {
                    name: meta.name,
                    annotations: meta.annotations.clone(),
                    ty: *ty,
                });
            }
            Rep::Unit => {}
        }
    }

    pub fn type_id(&self) -> u32 {
        // Hash of the type name
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: Symbol,
    pub annotations: Vec<Symbol>,
    pub ty: Ty,
}

impl FieldInfo {
    pub fn name_hash(&self, interner: &Interner) -> u64 {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        interner.resolve(self.name).hash(&mut hasher);
        hasher.finish()
    }

    pub fn byte_offset(&self) -> usize {
        // Computed from the type layout
        todo!()
    }

    pub fn type_id(&self) -> u32 {
        todo!()
    }

    pub fn getter_offset(&self) -> usize {
        todo!()
    }
}
```

---

## 14. Directory Structure V3

```text
glyim-typeck/src/
├── lib.rs                      // Public API: TyDatabase, typecheck entry points
├── db.rs                       // TyDatabase implementation (query orchestrator)
│
├── queries/                    // Query infrastructure
│   ├── mod.rs                  // Thread-local dep collector, query helpers
│   └── keys.rs                 // QueryKey implementations for all queries
│
├── ty.rs                       // Ty, TyKind, TyArena, ValueId, EffectRow
├── unify.rs                    // UnificationTable, ErrorGuaranteed, occurs check
├── chr.rs                      // Goal, ChrRule, ChrStore (extended for V3)
│
├── elab/                       // Elaboration (query-aware)
│   ├── mod.rs                  // ElabContext, elaborate_item, elaborate_comptime
│   ├── check.rs                // check_expr (bidirectional check mode)
│   ├── synth.rs                // synth_expr (bidirectional synth mode)
│   ├── scope.rs                // Scope struct, variable bindings
│   └── effects.rs              // Effect row unification
│
├── staging.rs                  // Level, phase consistency checks
├── meta_vm.rs                  // MetaVm, VmInterpreter, ComptimeResult
│
├── rep.rs                      // Rep, RepMeta, FieldInfo (GHC-style generic rep)
│
├── reflect/                    // Reflection subsystem
│   ├── mod.rs                  // TypeMetaSoA, Optic, OpticDispatchTable
│   ├── optics.rs               // Lens, Prism, Traversal generation from Rep
│   ├── mph.rs                  // Minimal perfect hash computation
│   └── catamorphism.rs         // Generic fold derivation
│
├── diagnostics/                // Error reporting
│   ├── mod.rs                  // TypeError enum (extended for V3)
│   ├── zippering.rs            // Structural diffing
│   └── biabduction.rs          // Auto-fix synthesis
│
├── freeze.rs                   // resolve_ty, freeze_module, specialization
│
└── tests/
    ├── unit_unify.rs
    ├── unit_chr.rs
    ├── query_integration.rs    // Tests for query caching + invalidation
    ├── staging_tests.rs        // Phase consistency tests
    ├── comptime_tests.rs       // VM execution tests
    └── snapshot_errors.rs
```

---

## 15. The Invalidation Flow (End-to-End)

Here's how incremental recompilation works when a source file changes:

```
1. User edits main.g
   │
2. IncrementalState::apply_changes([("main.g", new_hash)])
   │  Computes: file Fingerprint changed
   │  Propagates: transitive dependents via DependencyGraph
   │  Marks: affected query keys as Red
   │
3. User queries type_of("main.g")
   │
4. TyDatabase::typecheck_module(TypecheckModuleKey { path: "main.g" })
   │  Key Fingerprint is Red → must recompute
   │
5. Recomputation calls sub-queries:
   │  nominal_type(Foo)    → Red? Recompute. Green? Return cached.
   │  chr_rules(FooImpl)   → Red? Recompute. Green? Return cached.
   │  rep_of(Foo)          → Red? Recompute. Green? Return cached.
   │  eval_comptime(block) → Red? Re-run VM. Green? Return cached.
   │
6. If eval_comptime is Green:
   │  Generated items are the same → no need to re-elaborate them
   │  (This is the big win: comptime blocks only re-run when their
   │   dependencies change, not when any file changes.)
   │
7. If eval_comptime is Red (because Foo's Rep changed):
   │  VM re-runs → may generate different items → re-elaborate those
   │
8. TypecheckModule result is cached as Green
   │  with updated dependency edges
   │
9. IncrementalState::save() persists to disk
```

---

## 16. Summary: What the Query System Enables

| Feature | Without Queries | With Queries |
|---|---|---|
| **Comptime** | Re-runs every compilation | Cached; only re-runs when dependencies change |
| **Macro expansion** | Re-expands every time | Cached by `ExpandMacroKey`; invalidated only when inputs change |
| **Rep generation** | Rebuilds every time | Cached by `RepForKey`; invalidated only when the type definition changes |
| **Reflection metadata** | Regenerates every time | Cached by `ReflectMetaKey`; invalidated only when the Rep changes |
| **Module typecheck** | Re-typechecks every file | Cached; only re-runs when transitive deps change |
| **Specialization** | Must recompute from scratch | Specialized metadata is a query result; only regenerated when usage changes |
| **Incremental builds** | Ad-hoc, error-prone | Automatic via `DependencyGraph` + `Fingerprint` |
| **IDE responsiveness** | Full re-typecheck on every keystroke | Fine-grained queries + `GranularityMonitor` adapt to edit patterns |

The query system turns your metacompiler from a batch processor into a **reactive system**: only the minimal amount of work is done in response to any change, and all results are consistent and traceable.
