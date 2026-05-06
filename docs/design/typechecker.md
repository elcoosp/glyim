This is the definitive architectural spec for `glyim-typeck v2`. It is designed from the ground up using the exact patterns used by `rustc` and the `roc` compiler. 

The goal is **Zero-Abstraction Leakage**: AST traversal, type algebra, unification, and error reporting are completely decoupled. 

---

# The Glyim Typeck V2 Architecture Spec

## 1. Core Architectural Principles
1. **Arena Allocation**: All types are allocated in a central `TyArena` and referenced by `Copy` IDs. Zero deep cloning, `O(1)` hashing, `O(1)` equality checks.
2. **ErrorGuaranteed Pattern**: The type checker **never panics** on invalid code. It emits an error, returns a poison type (`Ty::Error`), and returns an `ErrorGuaranteed` token. Callers can safely ignore this token.
3. **Strict Phase Separation**:
   - *Phase 1 (Traversal)*: Walk HIR, emit constraints, return `Ty`.
   - *Phase 2 (Solving)*: Run Union-Find.
   - *Phase 3 (Freezing)*: Resolve inference vars for monomorphization.
4. **100% Span Coverage**: Every `Ty::Infer` created must carry a `Span`. Every error must carry a `Span`. No `(0, 0)` fallbacks.

---

## 2. The Data Layer (`ty.rs`)
*Replaces direct usage of `HirType` inside the typechecker.*

We stop using `HirType` for inference because it lacks indirection. We implement a generational arena.

```rust
use std::marker::PhantomData;
use glyim_diag::Span;

// The actual variations of types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyKind {
    Int, Float, Bool, Str, Unit, Never,
    
    /// Poison type. Subsumes all other types to prevent cascading errors.
    Error,
    
    /// A nominal type (e.g., a struct/enum name with no generic args applied yet)
    Named(Symbol),
    
    /// A fully applied generic type (e.g., `Vec<i64>`)
    App(Symbol, Vec<Ty>),
    
    /// Function signature
    Fn(Vec<Ty>, Ty),
    
    /// Raw pointers
    RawPtr(Ty),
}

/// A reference to a type in the arena. Completely `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ty(usize);

/// The Arena. Holds all types so we don't clone them.
pub struct TyArena {
    kinds: Vec<TyKind>,
    /// For debuggability: maps Ty::Infer IDs to the span that created them
    infer_spans: Vec<Span>,
}

impl TyArena {
    pub fn alloc(&mut self, kind: TyKind) -> Ty {
        let id = self.kinds.len();
        self.kinds.len();
        self.kinds.push(kind);
        Ty(id)
    }

    pub fn alloc_infer(&mut self, span: Span) -> Ty {
        let id = self.kinds.len();
        self.infer_spans.push(span);
        self.kinds.push(TyKind::Error); // Placeholder, mutated by UnificationTable
        Ty(id)
    }

    pub fn get(&self, ty: Ty) -> &TyKind {
        &self.kinds[ty.0]
    }
}
```

---

## 3. The Unification Engine (`unify.rs`)
*Replaces your `HashMap` substitution logic.*

This is a pure state machine. It knows nothing about HIR, `miette`, or your struct definitions. It only knows about `TyKind`.

```rust
/// Rustc pattern: A token proving an error was reported.
/// Returning this means "I already told the user it's wrong, don't panic."
#[derive(Clone, Copy, Debug)]
pub struct ErrorGuaranteed(#[kanid id=0] std::convert::Infallible);

pub struct UnificationTable {
    /// Union-Find parent links. parent[i] = j means i is linked to j.
    parents: Vec<usize>,
    /// Ranks for Union-Find balancing
    ranks: Vec<u8>,
}

impl UnificationTable {
    /// Create a fresh inference variable in the arena and table
    pub fn new_var(&mut self, arena: &mut TyArena, span: Span) -> Ty {
        let ty = arena.alloc_infer(span);
        self.parents.push(ty.0); // Points to itself
        self.ranks.push(0);
        ty
    }

    /// Find the root of a variable with path compression
    pub fn find(&self, arena: &TyArena, ty: Ty) -> Ty {
        match arena.get(ty) {
            TyKind::Error => ty, // Error types are their own root
            _ if self.parents[ty.0] == ty.0 => ty,
            _ => {
                // Path compression (simplified)
                let root = self.find(arena, Ty(self.parents[ty.0]));
                self.parents[ty.0] = root.0;
                root
            }
        }
    }

    /// The core unification algorithm
    pub fn unify(
        &mut self, 
        arena: &mut TyArena, 
        a: Ty, 
        b: Ty, 
        span: Span, 
        emit_err: &mut dyn FnMut(TypeError)
    ) -> Result<(), ErrorGuaranteed> {
        let a = self.find(arena, a);
        let b = self.find(arena, b);

        if a == b { return Ok(()); }
        if matches!(arena.get(a), TyKind::Error) || matches!(arena.get(b), TyKind::Error) {
            return Ok(()); // Error poisoning
        }

        // Occurs check: prevent ?0 = Vec<?0>
        if self.occurs(arena, a, b) || self.occurs(arena, b, a) {
            let origin_span = arena.get_infer_span(a).or_else(|| arena.get_infer_span(b));
            emit_err(TypeError::InfiniteType { span: origin_span.unwrap_or(span) });
            arena.kinds[a.0] = TyKind::Error;
            return Err(ErrorGuaranteed::new());
        }

        // If 'a' is an inference variable, link it to 'b'
        if self.is_infer(arena, a) {
            self.union(arena, a, b);
            return Ok(());
        }
        if self.is_infer(arena, b) {
            self.union(arena, b, a);
            return Ok(());
        }

        // Structural unification for concrete types
        self.unify_structural(arena, a, b, span, emit_err)
    }
}
```

---

## 4. The Context (`context.rs`)
*The central orchestrator. Replaces `mod.rs`, `scope.rs`.*

Uses the Rust alias pattern (`type Cx<'a>`) so you don't have to pass 5 generic parameters everywhere.

```rust
use glyim_hir::node::Hir;

/// Convenient context alias used everywhere in the typechecker.
pub type Cx<'a, 'b> = TypeckContext<'a, 'b>;

pub struct TypeckContext<'hir, 'diag> {
    pub hir: &'hir Hir,
    pub interner: &'hir Interner,
    pub arena: &'diag mut TyArena,
    pub unification: &'diag mut UnificationTable,
    pub scopes: Vec<Scope>,
    
    /// Cached struct/enum info from HIR
    pub struct_infos: HashMap<Symbol, StructInfo>,
    pub enum_infos: HashMap<Symbol, EnumInfo>,
    
    /// Output for the rest of the compiler
    pub expr_types: Vec<Ty>,
    pub call_type_args: HashMap<ExprId, Vec<Ty>>,
    
    /// Error buffer
    pub errors: Vec<TypeError>,
}

impl<'hir, 'diag> Cx<'hir, 'diag> {
    /// The primary entry point for checking an expression.
    /// Implements Bidirectional Type Checking.
    pub fn check_expr(&mut self, expr: &HirExpr, expected: Ty) -> Result<Ty, ErrorGuaranteed> {
        match expr {
            // CHECK mode: We know the target type
            HirExpr::Call { callee, args, id, span } => {
                let callee_ty = self.synth_expr(callee)?;
                
                // Create fresh inference vars for params and return
                let param_tys: Vec<Ty> = args.iter().map(|_| self.unification.new_var(self.arena, *span)).collect();
                let ret_ty = self.unification.new_var(self.arena, *span);
                
                // Constrain callee to match the function signature
                let fn_ty = self.arena.alloc(TyKind::Fn(param_tys.clone(), ret_ty));
                self.unification.unify(self.arena, callee_ty, fn_ty, *span, &mut |e| self.errors.push(e))?;
                
                // Check arguments against inferred param types
                for (arg, param_ty) in args.iter().zip(param_tys) {
                    self.check_expr(arg, param_ty)?;
                }
                
                // Constrain the inferred return type to match the expected type
                self.unification.unify(self.arena, ret_ty, expected, *span, &mut |e| self.errors.push(e))?;
                
                self.set_type(*id, expected);
                Ok(expected)
            }
            
            // Default: Synthesize, then Unify with expected
            _ => {
                let inferred = self.synth_expr(expr)?;
                self.unification.unify(self.arena, inferred, expected, expr.get_span(), &mut |e| self.errors.push(e))?;
                Ok(expected)
            }
        }
    }

    /// SYNTH mode: Figure out the type from the expression itself.
    pub fn synth_expr(&mut self, expr: &HirExpr) -> Result<Ty, ErrorGuaranteed> {
        let err_ty = self.arena.alloc(TyKind::Error);
        match expr {
            HirExpr::IntLit { id, .. } => {
                let ty = self.arena.alloc(TyKind::Int);
                self.set_type(*id, ty);
                Ok(ty)
            }
            HirExpr::Ident { name, span, id } => {
                match self.lookup_binding(name) {
                    Some(ty) => { self.set_type(*id, ty); Ok(ty) }
                    None => {
                        self.errors.push(TypeError::UnresolvedName { 
                            name: self.interner.resolve(*name).to_string(), 
                            span: *span 
                        });
                        self.set_type(*id, err_ty);
                        Err(ErrorGuaranteed::new())
                    }
                }
            }
            // ... other synth cases
            _ => Ok(err_ty)
        }
    }
}
```

---

## 5. Developer Experience: The Error Layer (`diagnostics.rs`)
*Replaces `error.rs`.*

By using `miette` correctly and `ErrorGuaranteed`, we guarantee no panics, and beautiful terminal output.

```rust
use miette::{Diagnostic, SourceCode};
use glyim_diag::Span;

#[derive(Debug, thiserror::Error)]
#[error("Type Error")]
pub enum TypeError {
    #[error("variable `{name}` not found in this scope")]
    #[diagnostic(code(glyim::unresolved_name))]
    UnresolvedName {
        name: String,
        #[label("not found here")]
        span: Span,
    },

    #[error("infinite type detected")]
    #[diagnostic(code(glyim::infinite_type))]
    InfiniteType {
        #[label("recursive type inferred here")]
        span: Span,
        #[help("Ensure your type does not contain itself directly or indirectly.")]
        help: String,
    },

    #[error("expected `{expected}`, found `{found}`")]
    #[diagnostic(code(glyim::mismatch))]
    MismatchedTypes {
        expected: String,
        found: String,
        #[label("expected {expected} because of return type")]
        expected_span: Span,
        #[label("found {found}")]
        found_span: Span,
    }
}

// Helper to format Ty for DX
impl TypeError {
    pub fn mismatch(expected: Ty, found: Ty, cx: &Cx) -> Self {
        Self::MismatchedTypes {
            expected: cx.format_ty(expected),
            found: cx.format_ty(found),
            // Spans are fetched from the TyArena's infer_spans if they are inference vars!
            expected_span: cx.arena.get_infer_span(expected).unwrap_or_default(),
            found_span: cx.arena.get_infer_span(found).unwrap_or_default(),
        }
    }
}
```

---

## 6. Debuggability Infrastructure (`debug.rs`)
*No other language has this built directly into the typechecker core.*

Because all types are in an `Arena` and all inference variables are in a `UnificationTable`, dumping the entire state of the compiler at the exact moment of an error is trivial.

```rust
impl<'hir, 'diag> Cx<'hir, 'diag> {
    /// Call this inside `#[tracing::instrument]` on error.
    pub fn dump_state_to_stderr(&self) {
        eprintln!("=== GLYIM TYPECK STATE DUMP ===\n");
        
        eprintln!("--- TyArena ({} types) ---", self.arena.kinds.len());
        for (i, kind) in self.arena.kinds.iter().enumerate() {
            eprintln!("  [{}] {:?}", i, kind);
        }
        
        eprintln!("\n--- Unification Table ---");
        for (i, parent) in self.unification.parents.iter().enumerate() {
            if i == *parent { 
                eprintln!("  ?{} = Unbound (created at {:?})", i, self.arena.infer_spans.get(i)); 
            } else { 
                eprintln!("  ?{} -> [{}]", i, parent); 
            }
        }

        eprintln!("\n--- Scopes ---");
        for (level, scope) in self.scopes.iter().enumerate() {
            eprintln!("  Level {}:", level);
            for (name, binding) in &scope.bindings {
                eprintln!("    {} : [{}]", self.interner.resolve(*name), binding.ty.0);
            }
        }
    }
}
```

---

## 7. The Monomorphization Hook (`freeze.rs`)
*How V2 talks to the rest of the compiler.*

In V1, you mutated `call_type_args` with `HirType` during traversal. In V2, traversal creates inference variables. You must "freeze" them at the end.

```rust
impl TypeChecker {
    pub fn finish(self) -> (Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
        let mut frozen_expr_types = Vec::with_capacity(self.arena.kinds.len());
        
        for ty_id in self.expr_types {
            // Resolve inference variables to their final concrete forms
            let resolved_ty = self.unification.find(&self.arena, ty_id);
            let hir_ty = self.convert_ty_to_hir(resolved_ty); // Map TyKind -> HirType
            frozen_expr_types.push(hir_ty);
        }

        let mut frozen_call_args = HashMap::new();
        for (expr_id, args) in self.call_type_args {
            let frozen_args: Vec<HirType> = args.iter()
                .map(|ty| {
                    let resolved = self.unification.find(&self.arena, *ty);
                    self.convert_ty_to_hir(resolved)
                })
                .collect();
            frozen_call_args.insert(expr_id, frozen_args);
        }

        (frozen_expr_types, frozen_call_args)
    }
}
```

---

## 8. Directory Structure of V2

```text
glyim-typeck/src/typeck/
├── mod.rs          // Public interface: TypeChecker::new(), check(), finish()
├── context.rs      // The Cx<'a> alias and TypeckContext struct
├── ty.rs           // Ty, TyKind, TyArena (The Data)
├── unify.rs        // UnificationTable, ErrorGuaranteed (The Math)
├── check.rs        // impl Cx { fn check_expr, fn check_stmt } (Bidirectional Checking)
├── synth.rs        // impl Cx { fn synth_expr } (Synthesizing)
├── scope.rs        // Scope struct (pure data, no logic)
├── freeze.rs       // impl TypeChecker { fn finish() } (Output translation)
├── diagnostics.rs  // TypeError enum, miette integration (The DX)
├── debug.rs        // State dumping utilities (The Debuggability)
└── tests.rs        // Unit tests for the new modules
```

## Why this is the ultimate Rust pattern:
1. **`Copy` everywhere**: By using `Ty(usize)`, you never fight the borrow checker with deep `HirType` trees.
2. **No `unwrap()` in compiler code**: `ErrorGuaranteed` ensures that if a user writes garbage, your compiler gracefully degrades to returning `Ty::Error`, outputs a beautiful `miette` error, and exits with code 1, rather than triggering a `panic!` deep in the unifier.
3. **Testability**: You can write a unit test that just does `let mut table = UnificationTable::new(); table.unify(...)`. You don't need to parse a string to test your unification logic.
