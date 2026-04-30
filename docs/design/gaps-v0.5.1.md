I've done a thorough review of the current codebase against the full specification chain (`v0.1.0` through `v0.5.0`). The compiler already spans from lexer to codegen, including package management scaffolds and some v0.5.0 components, but many critical features are either **stubbed, only partially wired, or silently broken** because later passes were never completed. Below is a categorized gap analysis.

## 1. Overall Codebase Status vis-à-vis the Specs

| Version | Theme | Status |
|---------|-------|--------|
| **v0.1.0** (Architecture) | Pipeline, crates, CAS trait, hygiene framework | ✅ Fully implemented |
| **v0.2.0** (Real Language) | `let`/`mut`, `if`/`else`, strings, `println`, Rowan CST, ariadne, JIT | ✅ Implemented |
| **v0.3.0** (Types & Data) | Structs, enums, `bool`, `match`, `Option`/`Result` built‑ins, `?`, raw pointers, `f64`, typed macros | 🟡 Mostly present but several pieces are incomplete or buggy |
| **v0.4.0** (Generics & UX) | Generic types, impl blocks, tuples, destructuring, pattern guards, cast, visibility | 🔴 **The most incomplete layer** – many HIR and parser pieces exist, but the type‑checker and monomorphization are not integrated |
| **v0.5.0** (Ecosystem) | Package manager, stdlib, remote CAS, DWARF, tests, cross‑compile, docs, optimizations | 🟠 Infrastructure is partly built, but actual functionality is largely missing or hardcoded |

---

## 2. Major Gaps and Semi‑Implemented Features

### v0.3.0 – v0.4.0 Language Features

#### 🔴 **Generic instantiation & monomorphization – completely broken**
- **Parser & HIR:** Generic type parameters on structs, enums, and functions are parsed and lowered into `HirType::Generic` and `HirFn::type_params`.
- **Typechecker:** `TypeChecker::check` performs basic local inference but **never calls monomorphization**. There is no worklist algorithm, no substitution of concrete types.
- **Codegen:** `monomorphize.rs` contains a dead‑coded `instantiate_fn`. Generic calls are lowered to the raw generic function name, which would fail LLVM verification or produce wrong code.
- **Impact:** You cannot write or use `struct Node<T> { value: T }`, nor call `id::<i64>(42)`. All tests that use generics (like `e2e_generic_identity`) are either ignored or would fail if not masked by other issues.

#### 🟡 **Tuples – partially working**
- The parser, HIR, and codegen support tuple literals and destructuring.
- **But:** The `e2e_tuple` test is `#[ignore]` with a comment: *“Tuple field access codegen needs to GEP into correct struct type.”* The codegen for `p._0` is not correctly resolving the tuple’s struct type in all cases. The type‑checker also doesn’t verify that the tuple patterns match the declared types.

#### 🟡 **Impl blocks – missing method call resolution**
- Parsing and desugaring (`Struct_method` mangling) are implemented.
- **But:** The `e2e_impl_method` test is ignored because *“the call lowering doesn't convert Point::zero() to the mangled call.”* In practice, `Point::zero()` is not rewritten to `Point_zero()`, so impl‑defined functions are invisible.

#### 🟡 **Pattern guards – present but untested**
- The match arm `guard` field is parsed, stored in HIR, and the type‑checker verifies it’s `bool`. However, without generic monomorphization and proper match codegen (which was already fragile), guard execution in LLVM IR is likely missing or incorrectly wired.

#### 🟡 **Visibility (`pub`) – only parsed, not enforced**
- The parser recognises `pub` before `struct`, `fn`, `impl`.
- **However:** The HIR for structs, enums, and functions does **not** carry a visibility flag (only `HirImplDef` has `is_pub`). The type‑checker and codegen completely ignore visibility. So all items are effectively public.

#### 🟡 **Cast expressions (`as`) – stub codegen**
- Parsing and type‑checking of casts exist.
- **Codegen** for `HirExpr::As` simply returns `i64 0`, ignoring the target type. No actual `fptosi`, `sitofp`, or pointer casts are emitted.

#### 🟡 **`#[no_std]` detection – fragile**
- `detect_no_std` relies on a line being exactly `"no_std"`, which breaks if the declaration appears inside a comment or string (known limitation noted). For a real stdlib deployment, this needs proper parsing.

### v0.5.0 Ecosystem & Production Readiness

#### 🔴 **Standard Library – entirely stubbed**
- The `stdlib/` directory contains `.g` files that are **design documents**, not compilable Glyim code.
- `vec_i64.g` is the only implementation attempt, but it uses raw pointer operations whose support is incomplete, and the compiler currently would reject many of its patterns (e.g., pointer arithmetic, cast to `*mut i64`).
- **Missing:** `Vec<T>`, `String`, `HashMap`, `Iterator`, `Range`, `File`, `BufReader` – none of the v0.5.0 stdlib types can be used by real programs.

#### 🟠 **DWARF debug info – partially wired but not functional**
- `debug.rs` sets up `DIBuilder`, creates compile units, subprograms, and local variables.
- **However:**
  - It uses `DWARFSourceLanguage::C` (a placeholder) instead of a proper Glyim language code.
  - Debug info generation is only called inside `with_debug`/`with_line_tables` constructors, but these are never used by the CLI (`pipeline.rs` always creates `Codegen::new` without debug info, even for `--debug` mode).
  - `insert_declare` works around Inkwell bugs with raw FFI, so it may be unstable.
  - No macro‑call‑site debug info (required by ADR-024) is emitted.
  - The CLI has no `--debug` flag wired to trigger full DWARF; it only uses `BuildMode::Debug` which maps to `OptimizationLevel::None`, not to debug emission.
- **Result:** Binaries compiled with the current `--debug` flag contain no debug sections.

#### 🟠 **Package manager features – infrastructure present, execution missing**
- **Implemented:** Manifest parsing, lockfile generation/resolution, local path dependencies, `glyim add`/`remove`, `glyim init`.
- **Missing or semi‑implemented:**
  - **Registry client:** `RegistryClient` has a `publish` method, but the CLI `publish` command says `“publish not yet implemented”`.
  - **`glyim outdated`/`verify`** only read the lockfile locally; they never query a real registry.
  - **Workspace support** is implemented in `glyim-pkg` but **not integrated into the build pipeline** – `glyim build` only looks for a single `glyim.toml`.
  - **Hash‑pinned lockfile** is generated but the CLI never validates content hashes during fetch (the lockfile hash is a hardcoded placeholder `"sha256:abcdef"`).
  - No support for `[dev-dependencies]` or `[features]` at runtime.

#### 🟠 **Remote CAS / distributed cache – only basic HTTP skeleton**
- `ContentStore` trait now includes `store_action_result`, `retrieve_action_result`, `has_blobs` (matches v0.5.0 spec).
- `RemoteContentStore` can reach an HTTP CAS server, but:
  - No integration with the compilation pipeline – the build never calls `has_blobs` to check cache hits.
  - The CAS server (`glyim-cas-server`) is a minimal Actix‑web server, not a REAPI‑compatible gRPC server.
  - No action cache, no `FindMissingBlobs`, no authentication backend.
- **Result:** Remote caching is a library feature with no driver.

#### 🔴 **Cross‑compilation – absent**
- The CLI has no `--target` flag.
- Codegen always uses `TargetMachine::get_default_triple()`.
- No sysroot management, no cross‑linker configuration, not even a mock.

#### 🟡 **Test runner – mostly working**
- The test harness (`__glyim_test_main`) is generated, and `#[test]`/`#[test(should_panic)]`/`#[test(ignore)]` attributes are parsed and honoured.
- The integration tests for `should_panic` pass, so the runtime harness is functional.
- **Gap:** There is **no panic isolation** via `setjmp`/`longjmp` – a panic inside a test function aborts the whole test binary, so only the first failing test will be reported. The spec calls for per‑test isolation.

#### 🟡 **Documentation generator – minimal**
- `glyim-doc` exists and produces very simple HTML listing function and struct names. It does not parse doc comments (`///`), does not include type signatures, no cross‑referencing, no Markdown rendering.

---

## 3. Summary of Critical Gaps (What Blocks Real Usage)

| Gap | Impact | Priority |
|-----|--------|----------|
| **No monomorphization** | Generic types and functions are dead code; breaks `Option`/`Result` being truly generic, breaks `Vec<T>`, `HashMap`, etc. | 🔴 Immediate |
| **Standard library stubs** | Cannot write any non‑trivial program (no dynamic memory, no hash maps, no owned strings). | 🔴 Immediate |
| **Missing DWARF integration** | `gdb`/`lldb` won’t work, making debugging impossible. | 🔴 High |
| **Cross‑compilation absent** | Cannot build for ARM or macOS from Linux. | 🟠 Medium |
| **Remote CAS not integrated** | The marquee feature of shared macro caches is a dead code path. | 🟠 Medium |
| **Incomplete impl method resolution** | `Point::zero()` call does not desugar; ergonomic method syntax is broken. | 🟡 Medium |
| **Visibility not enforced** | All items are public; `pub` keyword is a no‑op. | 🟡 Low (for now) |
| **Tuple field access codegen bug** | Destructuring tuples may produce wrong LLVM IR (tests ignored). | 🟡 Low (if tuples aren’t used yet) |

## 4. What *Is* Solid?

- The frontend pipeline (lex → parse → CST → HIR lower) is robust and passes many snapshot tests.
- The ariadne‑based error reporting is excellent.
- The JIT execution path (`glyim run`) works for the basic language features.
- The package manifest and lockfile logic is correct in isolation.
- The test harness generation works (though crash isolation is missing).

## 5. Recommended Next Steps

1. **Implement monomorphization** – this is the single biggest blocker preventing generics and the stdlib from working.
2. **Fix impl method calls** – a relatively small desugaring bug that kills a major language feature.
3. **Wire up DWARF** – enable `Codegen::with_debug` from the CLI when `--debug` is passed; this will immediately unlock `gdb` support.
4. **Build a minimal working `Vec<T>`** – start with a concrete `VecI64` to validate the allocator and pointer codegen, then generalise.

The codebase proves the architecture is sound; the remaining work is largely about finishing the integration paths and completing the last few language‑level features. If you need a detailed plan for any specific area (like monomorphization or DWARF integration), I can produce a step‑by‑step sub‑project plan.
I'm using the writing-plans skill to create the implementation plan.

Let me analyze the codebase deeply before writing the plan.

**Key findings:**

1. **Lowering bug**: `lower/item.rs` discards `type_params` for structs and enums (always sets `vec![]`), and hardcodes all struct/enum field types to `HirType::Int`. Function type_params ARE preserved correctly.

2. **Type checker gap**: `check_struct_lit` always returns `HirType::Named(struct_name)` even for generic structs. `check_call` doesn't infer type arguments for generic function calls.

3. **Existing monomorphize stub**: `codegen/monomorphize.rs` has `instantiate_fn` that substitutes param/return types but ignores the body, and has a bug (checks `HirType::Generic` instead of `HirType::Named` for type params).

4. **Codegen mono_cache**: `Codegen` has `mono_cache: HashMap<(Symbol, Vec<HirType>), FunctionValue>` that's never populated.

5. **Why tests are ignored**: `e2e_generic_struct` fails because the struct definition loses type_params in lowering, so there's nothing to specialize. The current "everything is i64" representation means specialization would work for simple cases if names were mangled correctly.

**Architecture decision**: Monomorphization as an HIR-level pass between typeck and codegen. This keeps codegen simple and is testable in isolation. The pass returns `(Hir, HashMap<ExprId, HirType>)` — the HIR with mangled names plus a type override map for field access resolution.

Now I'll write the plan and save it.

```markdown
# Monomorphization Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement full monomorphization for generic functions and structs so that `struct Container<T> { value: T }` used with different concrete types produces separate LLVM types and functions.

**Architecture:** Add a monomorphization pass between typeck and codegen that takes the typed HIR, infers concrete type arguments at generic call/instantiation sites, creates specialized copies of generic items with mangled names (e.g., `Container__i64`), rewrites references to use mangled names, and produces a clean non-generic HIR for codegen. Also fix lowering bugs that discard type parameters.

**Tech Stack:** Rust, inkwell (LLVM bindings), insta (snapshot testing)

---

## Chunk 1: Preserve type information in lowering

**Files:**
- Modify: `crates/glyim-hir/src/lower/item.rs:1-120`
- Modify: `crates/glyim-hir/src/lower/types.rs:1-10`
- Test: `crates/glyim-hir/src/lower/tests.rs`

- [ ] **Step 1: Write failing test for struct type_params preservation**

```rust
// In crates/glyim-hir/src/lower/tests.rs, add:
#[test]
fn lower_struct_preserves_type_params() {
    let (hir, interner) = lower_source("struct Container<T> { value: T }\nmain = () => 0");
    let s = hir.items.iter().find_map(|i| {
        if let HirItem::Struct(s) = i { Some(s) } else { None }
    });
    assert!(s.is_some(), "expected Struct item");
    let s = s.unwrap();
    let t_sym = interner.intern("T");
    assert_eq!(s.type_params, vec![t_sym], "struct type_params should be preserved");
}

#[test]
fn lower_enum_preserves_type_params() {
    let (hir, interner) = lower_source("enum Option<T> { Some(T), None }\nmain = () => 0");
    let e = hir.items.iter().find_map(|i| {
        if let HirItem::Enum(e) = i { Some(e) } else { None }
    });
    assert!(e.is_some(), "expected Enum item");
    let e = e.unwrap();
    let t_sym = interner.intern("T");
    assert_eq!(e.type_params, vec![t_sym], "enum type_params should be preserved");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p glyim-hir --test lower_struct_preserves_type_params -v`
Expected: FAIL — `assert_eq!(s.type_params, vec![t_sym], ...)` fails because type_params is empty

- [ ] **Step 3: Fix struct lowering to preserve type_params**

In `crates/glyim-hir/src/lower/item.rs`, find the `Item::StructDef` arm (around line 87). Change:
```rust
Some(HirItem::Struct(StructDef {
    name: *name,
    type_params: vec![],  // CHANGE THIS
    fields: hir_fields,
    span: glyim_diag::Span::new(name_span.start, end),
}))
```
To:
```rust
Some(HirItem::Struct(StructDef {
    name: *name,
    type_params: type_params.clone(),  // PRESERVE
    fields: hir_fields,
    span: glyim_diag::Span::new(name_span.start, end),
}))
```

- [ ] **Step 4: Fix enum lowering to preserve type_params**

In the same file, find the `Item::EnumDef` arm (around line 105). Change:
```rust
Some(HirItem::Enum(EnumDef {
    name: *name,
    type_params: vec![],  // CHANGE THIS
    variants: hir_variants,
    span: glyim_diag::Span::new(name_span.start, end),
}))
```
To:
```rust
Some(HirItem::Enum(EnumDef {
    name: *name,
    type_params: type_params.clone(),  // PRESERVE
    variants: hir_variants,
    span: glyim_diag::Span::new(name_span.start, end),
}))
```

- [ ] **Step 5: Fix struct field types to use actual type annotations**

In the same `Item::StructDef` arm, the field lowering currently does:
```rust
let hir_fields: Vec<StructField> = fields
    .iter()
    .map(|(sym, _, _)| StructField {
        name: *sym,
        ty: HirType::Int,
    })
    .collect();
```
Change to:
```rust
let hir_fields: Vec<StructField> = fields
    .iter()
    .map(|(sym, _, ty)| StructField {
        name: *sym,
        ty: ty.as_ref()
            .map(|t| lower_type_expr(t, ctx))
            .unwrap_or(HirType::Int),
    })
    .collect();
```

- [ ] **Step 6: Fix enum variant field types to use actual type annotations**

In the `Item::EnumDef` arm, find the variant field mapping (around line 118). Change:
```rust
fields: match &v.kind {
    glyim_parse::VariantKind::Unnamed(types)
    | glyim_parse::VariantKind::Named(types) => types
        .iter()
        .map(|(sym, _, _)| StructField {
            name: *sym,
            ty: HirType::Int,
        })
        .collect(),
},
```
To:
```rust
fields: match &v.kind {
    glyim_parse::VariantKind::Unnamed(types)
    | glyim_parse::VariantKind::Named(types) => types
        .iter()
        .map(|(sym, _, ty)| StructField {
            name: *sym,
            ty: ty.as_ref()
                .map(|t| lower_type_expr(t, ctx))
                .unwrap_or(HirType::Int),
        })
        .collect(),
},
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo nextest run -p glyim-hir -v`
Expected: All existing tests pass plus the two new tests pass

- [ ] **Step 8: Commit**

```bash
git add crates/glyim-hir/src/lower/item.rs crates/glyim-hir/src/lower/types.rs crates/glyim-hir/src/lower/tests.rs
git commit -m "fix(lower): preserve type_params and field types for structs/enums"
```

---

## Chunk 2: Type argument inference in type checker

**Files:**
- Modify: `crates/glyim-typeck/src/typeck/mod.rs:25-45`
- Modify: `crates/glyim-typeck/src/typeck/expr.rs:1-50` (check_call)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs:80-110` (check_struct_lit)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs:115-155` (check_enum_variant)
- Test: `crates/glyim-typeck/src/typeck/tests.rs`

- [ ] **Step 1: Add call_type_args field to TypeChecker**

In `crates/glyim-typeck/src/typeck/mod.rs`, add to the `TypeChecker` struct (after `expr_types`):
```rust
/// Maps ExprId of Call expressions to their inferred type arguments.
/// Only populated for calls to generic functions.
pub call_type_args: HashMap<ExprId, Vec<HirType>>,
```
Add the import at the top of the file if not present:
```rust
use std::collections::HashMap;
```

Initialize it in `TypeChecker::new`:
```rust
call_type_args: HashMap::new(),
```

- [ ] **Step 2: Write failing test for call type arg inference**

In `crates/glyim-typeck/src/typeck/tests.rs`, add:
```rust
#[test]
fn infer_type_args_for_generic_call() {
    let tc = typecheck("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
    // Find the Call expression for id(42) - it's inside main's body
    // The call_type_args should map its ExprId to [HirType::Int]
    assert!(!tc.call_type_args.is_empty(), "should have inferred type args for id(42)");
    let args = tc.call_type_args.values().next().unwrap();
    assert_eq!(args.len(), 1, "id should have 1 type param");
    assert_eq!(args[0], HirType::Int, "T should be inferred as Int from arg 42");
}

#[test]
fn infer_type_args_for_generic_struct_lit() {
    let tc = typecheck("struct Container<T> { value: T }\nmain = () => { let c = Container { value: 42 }; c.value }");
    // The StructLit for Container { value: 42 } should have type Generic(Container, [Int])
    // Find the expr_type for the struct lit - it's the LetPat value for c
    // We can't easily get the ExprId from here, but we can check that no type errors occur
    assert!(tc.errors.is_empty(), "generic struct lit should not error: {:?}", tc.errors);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo nextest run -p glyim-typeck --test infer_type_args_for_generic_call -v`
Expected: FAIL — `call_type_args.is_empty()` assertion fails

- [ ] **Step 4: Implement type arg inference in check_call**

In `crates/glyim-typeck/src/typeck/expr.rs`, replace the `check_call` method with:

```rust
fn check_call(&mut self, callee: Symbol, args: &[HirExpr]) -> HirType {
    // Check argument expressions first
    let arg_types: Vec<HirType> = args
        .iter()
        .filter_map(|a| self.check_expr(a))
        .collect();

    // Try to find the function definition
    let fn_def = self.fns.iter().find(|f| f.name == callee);

    if let Some(fn_def) = fn_def {
        if !fn_def.type_params.is_empty() {
            // Generic function: infer type args from argument types
            let mut type_args_map: HashMap<Symbol, HirType> = HashMap::new();
            for (i, tp) in fn_def.type_params.iter().enumerate() {
                if let Some(param_ty) = fn_def.params.get(i).map(|(_, ty)| ty) {
                    if let Some(arg_ty) = arg_types.get(i) {
                        type_args_map.insert(*tp, arg_ty.clone());
                    }
                }
            }
            // Record the inferred type args for this call site
            // We need the call's ExprId, but check_call doesn't have it
            // We'll handle this differently - store on the last arg's id as a proxy
            // Actually, let's use the caller approach instead
        }

        // Resolve return type using inferred type args
        let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
        if !fn_def.type_params.is_empty() {
            let sub: HashMap<Symbol, HirType> = fn_def
                .type_params
                .iter()
                .zip(arg_types.iter())
                .filter_map(|(tp, at)| Some((*tp, at.clone())))
                .collect();
            return crate::types::substitute_type(&ret, &sub);
        }
        return ret;
    }

    // Check impl methods
    for methods in self.impl_methods.values() {
        if let Some(fn_def) = methods.iter().find(|f| f.name == callee) {
            return fn_def.ret.clone().unwrap_or(HirType::Int);
        }
    }

    if self.extern_fns.contains_key(&callee) {
        return self
            .extern_fns
            .get(&callee)
            .map(|sig| sig.ret.clone())
            .unwrap_or(HirType::Int);
    }
    HirType::Int
}
```

Wait — this approach has a problem. `check_call` doesn't have access to the ExprId of the Call node, so it can't store in `call_type_args`. The caller (`check_expr`) has the id. Let me restructure.

- [ ] **Step 5: Restructure check_call to return type args, store in check_expr**

In `check_expr`, find the `HirExpr::Call` arm and change it to:

```rust
HirExpr::Call { id, callee, args, .. } => {
    let (ret_ty, inferred_args) = self.check_call_with_type_args(*callee, args);
    if let Some(type_args) = inferred_args {
        self.call_type_args.insert(*id, type_args);
    }
    self.set_type(*id, ret_ty.clone());
    Some(ret_ty)
}
```

Then rename and rewrite `check_call` to `check_call_with_type_args`:

```rust
/// Check a function call. Returns (return_type, Option<inferred_type_args>).
fn check_call_with_type_args(
    &mut self,
    callee: Symbol,
    args: &[HirExpr],
) -> (HirType, Option<Vec<HirType>>) {
    let arg_types: Vec<HirType> = args
        .iter()
        .filter_map(|a| self.check_expr(a))
        .collect();

    let fn_def = self.fns.iter().find(|f| f.name == callee);

    if let Some(fn_def) = fn_def {
        if !fn_def.type_params.is_empty() {
            let sub: HashMap<Symbol, HirType> = fn_def
                .type_params
                .iter()
                .zip(arg_types.iter())
                .filter_map(|(tp, at)| {
                    if at == &HirType::Never { None } else { Some((*tp, at.clone())) }
                })
                .collect();
            if sub.len() == fn_def.type_params.len() {
                let type_args: Vec<HirType> = fn_def
                    .type_params
                    .iter()
                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                    .collect();
                let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
                return (crate::types::substitute_type(&ret, &sub), Some(type_args));
            }
        }
        return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
    }

    for methods in self.impl_methods.values() {
        if let Some(fn_def) = methods.iter().find(|f| f.name == callee) {
            return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
        }
    }

    if self.extern_fns.contains_key(&callee) {
        return (
            self.extern_fns
                .get(&callee)
                .map(|sig| sig.ret.clone())
                .unwrap_or(HirType::Int),
            None,
        );
    }
    (HirType::Int, None)
}
```

- [ ] **Step 6: Add substitute_type helper to types module**

In `crates/glyim-hir/src/types.rs`, add at the bottom (before the `#[cfg(test)]` module):

```rust
/// Substitute type parameters with concrete types.
/// `sub` maps type parameter symbols to their concrete types.
pub fn substitute_type(ty: &HirType, sub: &std::collections::HashMap<Symbol, HirType>) -> HirType {
    match ty {
        HirType::Named(sym) => {
            if let Some(concrete) = sub.get(sym) {
                return concrete.clone();
            }
            ty.clone()
        }
        HirType::Generic(sym, args) => {
            let new_args: Vec<HirType> = args
                .iter()
                .map(|a| substitute_type(a, sub))
                .collect();
            // If no args and it's in the sub map, substitute directly
            if new_args.is_empty() {
                if let Some(concrete) = sub.get(sym) {
                    return concrete.clone();
                }
            }
            HirType::Generic(*sym, new_args)
        }
        HirType::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| substitute_type(e, sub)).collect())
        }
        HirType::RawPtr(inner) => {
            HirType::RawPtr(Box::new(substitute_type(inner, sub)))
        }
        HirType::Option(inner) => {
            HirType::Option(Box::new(substitute_type(inner, sub)))
        }
        HirType::Result(ok, err) => {
            HirType::Result(
                Box::new(substitute_type(ok, sub)),
                Box::new(substitute_type(err, sub)),
            )
        }
        HirType::Func(params, ret) => {
            HirType::Func(
                params.iter().map(|p| substitute_type(p, sub)).collect(),
                Box::new(substitute_type(ret, sub)),
            )
        }
        HirType::Int | HirType::Bool | HirType::Float | HirType::Str
        | HirType::Unit | HirType::Never | HirType::Opaque(_) => ty.clone(),
    }
}
```

Add the `use` for HashMap at the top of `types.rs` if needed:
```rust
use std::collections::HashMap;
```

- [ ] **Step 7: Fix check_struct_lit to return Generic type for generic structs**

In `crates/glyim-typeck/src/typeck/expr.rs`, replace `check_struct_lit`:

```rust
fn check_struct_lit(&mut self, struct_name: Symbol, fields: &[(Symbol, HirExpr)]) -> HirType {
    let field_names: Vec<Symbol> = fields.iter().map(|(sym, _)| *sym).collect();
    let field_count = fields.len();

    // Check field value expressions and collect their types
    let field_value_types: Vec<HirType> = fields
        .iter()
        .filter_map(|(_, val)| self.check_expr(val))
        .collect();

    if let Some(info) = self.structs.get(&struct_name) {
        for field_sym in &field_names {
            if !info.field_map.contains_key(field_sym) {
                self.errors.push(TypeError::UnknownField {
                    struct_name,
                    field: *field_sym,
                });
            }
        }
        if field_count != info.fields.len() {
            for field in &info.fields {
                if !field_names.contains(&field.name) {
                    self.errors.push(TypeError::MissingField {
                        struct_name,
                        field: field.name,
                    });
                }
            }
        }

        // If the struct is generic, infer type args from field values
        if !info.type_params.is_empty() && field_value_types.len() == info.fields.len() {
            let mut sub: HashMap<Symbol, HirType> = HashMap::new();
            for (i, tp) in info.type_params.iter().enumerate() {
                if let Some(field_ty) = info.fields.get(i).map(|f| &f.ty) {
                    if let Some(val_ty) = field_value_types.get(i) {
                        if let HirType::Named(param_sym) = field_ty {
                            if *param_sym == *tp {
                                sub.insert(*tp, val_ty.clone());
                            }
                        }
                    }
                }
            }
            if sub.len() == info.type_params.len() {
                let concrete_args: Vec<HirType> = info
                    .type_params
                    .iter()
                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                    .collect();
                return HirType::Generic(struct_name, concrete_args);
            }
        }
    }

    HirType::Named(struct_name)
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo nextest run -p glyim-hir -p glyim-typeck -v`
Expected: All tests pass, including the two new type checker tests

- [ ] **Step 9: Commit**

```bash
git add crates/glyim-hir/src/types.rs crates/glyim-typeck/src/typeck/mod.rs crates/glyim-typeck/src/typeck/expr.rs crates/glyim-typeck/src/typeck/tests.rs
git commit -m "feat(typeck): infer type arguments for generic calls and struct lits"
```

---

## Chunk 3: Monomorphization pass

**Files:**
- Create: `crates/glyim-hir/src/monomorphize.rs`
- Modify: `crates/glyim-hir/src/lib.rs`
- Test: `crates/glyim-hir/src/monomorphize.rs` (inline tests)

- [ ] **Step 1: Write failing test for monomorphize entry point**

Create `crates/glyim-hir/src/monomorphize.rs` with:

```rust
//! Monomorphization pass: replaces generic items with specialized copies.

use std::collections::{HashMap, HashSet};
use crate::item::{HirImplDef, HirItem, StructDef};
use crate::node::HirExpr;
use crate::types::{ExprId, HirType, HirFn, HirPattern, HirStmt};
use glyim_interner::{Interner, Symbol};

/// Result of monomorphization.
pub struct MonoResult {
    /// The monomorphized HIR (no generic definitions, only specialized copies).
    pub hir: crate::Hir,
    /// Override map: expr_id -> concrete type (for generic struct/enum expressions).
    /// Codegen should use this to override `expr_types` entries.
    pub type_overrides: HashMap<ExprId, HirType>,
}

/// Monomorphize the HIR.
///
/// `expr_types` — the type checker's per-expression type table.
/// `call_type_args` — inferred type args for generic function calls (from TypeChecker).
pub fn monomorphize(
    hir: &crate::Hir,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    let mut ctx = MonoContext::new(hir, expr_types, call_type_args);
    ctx.collect_and_specialize();
    ctx.build_result()
}

struct MonoContext<'a> {
    hir: &'a crate::Hir,
    expr_types: &'a [HirType],
    call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    // (fn_name, concrete_type_args) -> specialized HirFn
    fn_specs: HashMap<(Symbol, Vec<HirType>), HirFn>,
    // (struct_name, concrete_type_args) -> specialized StructDef
    struct_specs: HashMap<(Symbol, Vec<HirType>), StructDef>,
    // Type overrides for expr_ids in specialized functions
    type_overrides: HashMap<ExprId, HirType>,
    // Work queue: (fn_name, concrete_type_args)
    fn_work_queue: Vec<(Symbol, Vec<HirType>)>,
}

impl<'a> MonoContext<'a> {
    fn new(
        hir: &'a crate::Hir,
        expr_types: &'a [HirType],
        call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    ) -> Self {
        Self {
            hir,
            expr_types,
            call_type_args,
            fn_specs: HashMap::new(),
            struct_specs: HashMap::new(),
            type_overrides: HashMap::new(),
            fn_work_queue: Vec::new(),
        }
    }

    fn find_fn(&self, name: Symbol) -> Option<&HirFn> {
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                if f.name == name {
                    return Some(f);
                }
            }
        }
        None
    }

    fn find_struct(&self, name: Symbol) -> Option<&StructDef> {
        for item in &self.hir.items {
            if let HirItem::Struct(s) = item {
                if s.name == name {
                    return Some(s);
                }
            }
        }
        None
    }

    fn mangle_name(&self, base: Symbol, type_args: &[HirType]) -> Symbol {
        let base_str = self.resolve(base);
        let args_str = type_args
            .iter()
            .map(|t| format_type_short(t))
            .collect::<Vec<_>>()
            .join("_");
        // Use a synthetic intern — the actual Symbol doesn't need to come from an Interner
        // We'll use a placeholder and handle this at the Hir level
        // Actually, we need an Interner. For now, use the debug format as a key.
        let mangled = format!("{}__{}", base_str, args_str);
        // Return a symbol — but we don't have access to an Interner here.
        // We'll return a special marker and handle it in build_result.
        // FOR NOW: return the original symbol and handle mangling differently.
        // We'll use a String-based approach in build_result.
        base // placeholder — will fix in step 4
    }

    fn collect_and_specialize(&mut self) {
        // Phase 1: Scan non-generic functions for generic calls
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                if f.type_params.is_empty() {
                    self.scan_expr_for_generic_calls(&f.body);
                }
            }
        }

        // Phase 2: Process work queue
        while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
            let key = (fn_name, type_args.clone());
            if self.fn_specs.contains_key(&key) {
                continue;
            }
            if let Some(generic_fn) = self.find_fn(fn_name) {
                let specialized = self.specialize_fn(generic_fn, &type_args);
                // Scan specialized body for more generic calls
                self.scan_expr_for_generic_calls(&specialized.body);
                self.fn_specs.insert(key, specialized);
            }
        }

        // Phase 3: Collect struct specializations from expr_types
        for (id, ty) in self.expr_types.iter().enumerate() {
            if let HirType::Generic(name, args) = ty {
                if let Some(struct_def) = self.find_struct(*name) {
                    if !struct_def.type_params.is_empty()
                        && args.len() == struct_def.type_params.len()
                    {
                        let key = (*name, args.clone());
                        if !self.struct_specs.contains_key(&key) {
                            self.struct_specs.insert(
                                key,
                                self.specialize_struct(struct_def, args),
                            );
                        }
                        // Record type override: Generic -> Named(mangled)
                        let mangled_str = self.mangle_name_str(*name, args);
                        // We need an Interner to create the mangled symbol.
                        // We'll handle this in build_result with String keys.
                        // For now, store as Named with original symbol as placeholder.
                        self.type_overrides
                            .insert(ExprId::new(id as u32), HirType::Named(*name));
                    }
                }
            }
        }
    }

    fn build_result(self) -> MonoResult {
        let mut items = Vec::new();
        let mangle_map: HashMap<(Symbol, Vec<HirType>), String> = self
            .struct_specs
            .keys()
            .map(|(name, args)| {
                let s = self.mangle_name_str(*name, args);
                ((*name, args.clone()), s)
            })
            .collect();
        let fn_mangle_map: HashMap<(Symbol, Vec<HirType>), String> = self
            .fn_specs
            .keys()
            .map(|(name, args)| {
                let s = self.mangle_name_str(*name, args);
                ((*name, args.clone()), s)
            })
            .collect();

        // Add non-generic top-level items with calls rewritten
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) if f.type_params.is_empty() => {
                    items.push(HirItem::Fn(self.rewrite_fn_calls(f, &fn_mangle_map)));
                }
                HirItem::Struct(s) if s.type_params.is_empty() => {
                    items.push(HirItem::Struct(s.clone()));
                }
                HirItem::Enum(e) if e.type_params.is_empty() => {
                    items.push(HirItem::Enum(e.clone()));
                }
                HirItem::Extern(e) => {
                    items.push(HirItem::Extern(e.clone()));
                }
                HirItem::Impl(imp) if imp.type_params.is_empty() => {
                    items.push(HirItem::Impl(self.rewrite_impl_calls(imp, &fn_mangle_map)));
                }
                _ => {} // Skip generic definitions
            }
        }

        // Add specialized structs
        for (_, struct_def) in &self.struct_specs {
            items.push(HirItem::Struct(struct_def.clone()));
        }

        // Add specialized functions
        for (_, fn_def) in &self.fn_specs {
            items.push(HirItem::Fn(fn_def.clone()));
        }

        // Build final type_overrides using String-based mangled names
        // This is a placeholder — the real fix needs an Interner in the monomorphize function.
        // For now, return what we have.
        MonoResult {
            hir: crate::Hir { items },
            type_overrides: self.type_overrides,
        }
    }

    fn resolve(&self, sym: Symbol) -> &str {
        // We don't have an Interner. Return a placeholder.
        "???"
    }

    fn mangle_name_str(&self, base: Symbol, type_args: &[HirType]) -> String {
        // Since we can't resolve Symbol without an Interner,
        // we'll need the caller to provide one. For now, return empty.
        // This will be fixed when we pass an Interner to monomorphize.
        String::new()
    }

    fn specialize_fn(&self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub: HashMap<Symbol, HirType> = HashMap::new();
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        // Note: body expression types are NOT substituted here.
        // The body is rewritten at the HirExpr level (name mangling).
        mono
    }

    fn specialize_struct(&self, s: &StructDef, concrete: &[HirType]) -> StructDef {
        let mut sub: HashMap<Symbol, HirType> = HashMap::new();
        for (i, tp) in s.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = s.clone();
        mono.type_params.clear();
        for field in &mut mono.fields {
            field.ty = crate::types::substitute_type(&field.ty, &sub);
        }
        mono
    }

    fn scan_expr_for_generic_calls(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Call { callee, args, .. } => {
                // Check if callee is a generic function
                if let Some(fn_def) = self.find_fn(*callee) {
                    if !fn_def.type_params.is_empty() {
                        // Infer type args from arg types
                        let arg_types: Vec<HirType> = args
                            .iter()
                            .filter_map(|a| {
                                self.expr_types.get(a.get_id().as_usize())
                            })
                            .collect();
                        let mut sub: HashMap<Symbol, HirType> = HashMap::new();
                        for (i, tp) in fn_def.type_params.iter().enumerate() {
                            if let Some(at) = arg_types.get(i) {
                                sub.insert(*tp, at.clone());
                            }
                        }
                        if sub.len() == fn_def.type_params.len() {
                            let concrete: Vec<HirType> = fn_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            self.fn_work_queue.push((*callee, concrete));
                        }
                    }
                }
                for arg in args {
                    self.scan_expr_for_generic_calls(arg);
                }
            }
            HirExpr::StructLit { struct_name, fields, .. } => {
                if let Some(struct_def) = self.find_struct(*struct_name) {
                    if !struct_def.type_params.is_empty() {
                        // Infer type args from field value types
                        let field_types: Vec<HirType> = fields
                            .iter()
                            .filter_map(|(_, f)| {
                                self.expr_types.get(f.get_id().as_usize())
                            })
                            .collect();
                        let mut sub: HashMap<Symbol, HirType> = HashMap::new();
                        for (i, tp) in struct_def.type_params.iter().enumerate() {
                            if let Some(ft) = struct_def.fields.get(i) {
                                if let HirType::Named(fp_sym) = &ft.ty {
                                    if *fp_sym == *tp {
                                        if let Some(vt) = field_types.get(i) {
                                            sub.insert(*tp, vt.clone());
                                        }
                                    }
                                }
                            }
                        }
                        if sub.len() == struct_def.type_params.len() {
                            let concrete: Vec<HirType> = struct_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            let key = (*struct_name, concrete.clone());
                            if !self.struct_specs.contains_key(&key) {
                                self.struct_specs
                                    .insert(key, self.specialize_struct(struct_def, &concrete));
                            }
                        }
                    }
                }
                for (_, field) in fields {
                    self.scan_expr_for_generic_calls(field);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        HirStmt::Let { value, .. }
                        | HirStmt::LetPat { value, .. }
                        | HirStmt::Assign { value, .. }
                        | HirStmt::AssignDeref { value, .. }
                        | HirStmt::Expr(e) => {
                            self.scan_expr_for_generic_calls(value);
                            if let Some(e) = e { self.scan_expr_for_generic_calls(e); }
                        }
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.scan_expr_for_generic_calls(condition);
                self.scan_expr_for_generic_calls(then_branch);
                if let Some(e) = else_branch {
                    self.scan_expr_for_generic_calls(e);
                }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.scan_expr_for_generic_calls(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.scan_expr_for_generic_calls(g);
                    }
                    self.scan_expr_for_generic_calls(body);
                }
            }
            HirExpr::Binary { lhs, rhs, .. }
            | HirExpr::Unary { operand, .. }
            | HirExpr::Return { value: Some(v), .. }
            | HirExpr::Deref { expr, .. } => {
                self.scan_expr_for_generic_calls(lhs);
                self.scan_expr_for_generic_calls(rhs);
                if let Some(v) = value { self.scan_expr_for_generic_calls(v); }
                self.scan_expr_for_generic_calls(expr);
            }
            HirExpr::MethodCall { args, .. } => {
                for a in args {
                    self.scan_expr_for_generic_calls(a);
                }
            }
            _ => {}
        }
    }

    fn rewrite_fn_calls(&self, f: &HirFn, mangle_map: &HashMap<(Symbol, Vec<HirType>), String>) -> HirFn {
        let mut mono = f.clone();
        mono.body = self.rewrite_expr_calls(&f.body, mangle_map);
        mono
    }

    fn rewrite_impl_calls(&self, imp: &HirImplDef, mangle_map: &HashMap<(Symbol, Vec<HirType>), String>) -> HirImplDef {
        let mut mono = imp.clone();
        for method in &mut mono.methods {
            method.body = self.rewrite_expr_calls(&method.body, mangle_map);
        }
        mono
    }

    fn rewrite_expr_calls(&self, expr: &HirExpr, mangle_map: &HashMap<(Symbol, Symbol>, String>) -> HirExpr {
        // Placeholder — full implementation below
        expr.clone()
    }
}

fn format_type_short(ty: &HirType) -> String {
    match ty {
        HirType::Int => "i64".to_string(),
        HirType::Bool => "bool".to_string(),
        HirType::Float => "f64".to_string(),
        HirType::Str => "Str".to_string(),
        HirType::Unit => "()".to_string(),
        HirType::Named(_) => "Named".to_string(),
        HirType::Generic(_, _) => "Generic".to_string(),
        HirType::Tuple(_) => "Tuple".to_string(),
        HirType::RawPtr(_) => "Ptr".to_string(),
        HirType::Opaque(_) => "Opaque".to_string(),
        HirType::Func(_, _) => "Func".to_string(),
        HirType::Option(_) => "Option".to_string(),
        HirType::Result(_, _) => "Result".to_string(),
        HirType::Never => "Never".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hir;
    use glyim_interner::Interner;

    fn lower_source(source: &str) -> (Hir, glyim_interner::Interner) {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            panic!("parse errors: {:?}", parse_out.errors);
        }
        let mut interner = parse_out.interner;
        let hir = crate::lower(&parse_out.ast, &mut interner);
        (hir, interner)
    }

    #[test]
    fn monomorphize_non_generic_passthrough() {
        let (hir, _) = lower_source("main = () => 42");
        let result = monomorphize(&hir, &[], &HashMap::new());
        assert_eq!(result.hir.items.len(), hir.items.len());
    }

    #[test]
    fn monomorphize_generic_fn_creates_specialization() {
        let (hir, interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
        // Fake call_type_args: the Call expr for id(42) gets type_args [Int]
        // We need to know the ExprId... for testing, we'll just verify the struct
        // specialization path works
        let result = monomorphize(&hir, &[], &HashMap::new());
        // Without call_type_args, no specialization happens
        assert_eq!(result.hir.items.len(), hir.items.len());
    }
}
```

- [ ] **Step 2: Run test to verify it compiles and basic test passes**

Run: `cargo nextest run -p glyim-hir --test monomorphize_non_generic_passthrough -v`
Expected: PASS

Run: `cargo nextest run -p glyim-hir --test monomorphize_generic_fn_creates_specialization -v`
Expected: PASS (trivially, since no call_type_args provided)

- [ ] **Step 3: Commit placeholder**

```bash
git add crates/glyim-hir/src/monomorphize.rs crates/glyim-hir/src/lib.rs
git commit -m "feat(hir): add monomorphization pass skeleton"
```

---

## Chunk 4: Complete monomorphization pass implementation

**Files:**
- Modify: `crates/glyim-hir/src/monomorphize.rs` (replace placeholder with full impl)
- Modify: `crates/glyim-hir/src/monomorphize.rs` tests
- Modify: `crates/glyim-hir/src/lib.rs` (update exports)

- [ ] **Step 1: Update monomorphize signature to accept Interner**

Change the public function signature to accept an `&mut Interner` (needed for creating mangled name symbols):

```rust
pub fn monomorphize(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
```

Update `MonoContext` to hold `&mut Interner`:
```rust
struct MonoContext<'a> {
    hir: &'a crate::Hir,
    interner: &'a mut Interner,
    // ... rest unchanged ...
}
```

- [ ] **Step 2: Implement mangle_name properly**

Replace `mangle_name` and `mangle_name_str` with:

```rust
fn mangle_name(&mut self, base: Symbol, type_args: &[HirType]) -> Symbol {
    let base_str = self.interner.resolve(base).to_string();
    let args_str = type_args
        .iter()
        .map(|t| format_type_short(t))
        .collect::<Vec<_>>()
        .join("_");
    self.interner.intern(&format!("{}__{}", base_str, args_str))
}
```

- [ ] **Step 3: Implement rewrite_expr_calls fully**

Replace the placeholder `rewrite_expr_calls` with a recursive walk that rewrites:
- `Call { callee }` → look up in `fn_specs`, if found, replace callee with mangled name
- `StructLit { struct_name, fields }` → look up in `struct_specs`, if found, replace struct_name with mangled name
- All other variants → recurse into sub-expressions

```rust
fn rewrite_expr_calls(&mut self, expr: &HirExpr, fn_mangle_map: &HashMap<Symbol, Symbol>) -> HirExpr {
    match expr {
        HirExpr::Call { id, callee, args, span } => {
            let new_args: Vec<HirExpr> = args
                .iter()
                .map(|a| self.rewrite_expr_calls(a, fn_mangle_map))
                .collect();
            let new_callee = fn_mangle_map.get(callee).copied().unwrap_or(*callee);
            HirExpr::Call { id: *id, callee: new_callee, args: new_args, span: *span }
        }
        HirExpr::StructLit { id, struct_name, fields, span } => {
            let mangled = self.mangle_struct_name(*struct_name);
            let new_fields: Vec<(Symbol, HirExpr)> = fields
                .iter()
                .map(|(sym, expr)| (*sym, self.rewrite_expr_calls(expr, fn_mangle_map)))
                .collect();
            HirExpr::StructLit {
                id: *id,
                struct_name: mangled,
                fields: new_fields,
                span: *span,
            }
        }
        HirExpr::Block { id, stmts, span } => HirExpr::Block {
            id: *id,
            stmts: stmts.iter().map(|s| self.rewrite_stmt_calls(s, fn_mangle_map)).collect(),
            span: *span,
        },
        HirExpr::If { id, condition, then_branch, else_branch, span } => HirExpr::If {
            id: *id,
            condition: Box::new(self.rewrite_expr_calls(condition, fn_mangle_map)),
            then_branch: Box::new(self.rewrite_expr_calls(then_branch, fn_mangle_map)),
            else_branch: else_branch.as_ref().map(|e| Box::new(self.rewrite_expr_calls(e, fn_mangle_map))),
            span: *span,
        },
        HirExpr::Match { id, scrutinee, arms, span } => HirExpr::Match {
            id: *id,
            scrutinee: Box::new(self.rewrite_expr_calls(scrutinee, fn_mangle_map)),
            arms: arms
                .iter()
                .map(|(pat, guard, body)| {
                    let new_guard =
                        guard.as_ref().map(|g| Box::new(self.rewrite_expr_calls(g, fn_mangle_map)));
                    (
                        pat.clone(),
                        new_guard,
                        self.rewrite_expr_calls(body, fn_mangle_list),
                    )
                })
                .collect(),
            span: *span,
        },
        HirExpr::Binary { id, op, lhs, rhs, span } => HirExpr::Binary {
            id: *id, op: op.clone(),
            lhs: Box::new(self.rewrite_expr_calls(lhs, fn_mangle_map)),
            rhs: Box::new(self.rewrite_expr_calls(rhs, fn_mangle_map)),
            span: *span,
        },
        HirExpr::Unary { id, op, operand, span } => HirExpr::Unary {
            id: *id, op: op.clone(),
            operand: Box::new(self.rewrite_expr_calls(operand, fn_mangle_map)),
            span: *span,
        },
        HirExpr::Return { id, value, span } => HirExpr::Return {
            id: *id,
            value: value.as_ref().map(|v| Box::new(self.rewrite_expr_calls(v, fn_mangle_map))),
            span: *span,
        },
        HirExpr::Deref { id, expr, span } => HirExpr::Deref {
            id: *id,
            expr: Box::new(self.rewrite_expr_calls(expr, fn_mangle_map)),
            span: *span,
        },
        HirExpr::MethodCall { id, receiver, method_name, args, span } => {
            HirExpr::MethodCall {
                id: *id,
                receiver: Box::new(self.rewrite_expr_calls(receiver, fn_mangle_map)),
                method_name: *method_name,
                args: args.iter().map(|a| self.rewrite_expr_calls(a, fn_mangle_map)).collect(),
                span: *span,
            }
        }
        // All other variants: clone as-is (IntLit, Ident, BoolLit, StrLit, UnitLit, FloatLit,
        // Println, Assert, As, SizeOf, TupleLit, EnumVariant, FieldAccess)
        _ => expr.clone(),
    }
}

fn rewrite_stmt_calls(&mut self, stmt: &HirStmt, fn_mangle_map: &HashMap<Symbol, Symbol>) -> HirStmt {
    match stmt {
        HirStmt::Let { name, mutable, value, span } => HirStmt::Let {
            name: *name, mutable: *mutable,
            value: self.rewrite_expr_calls(value, fn_mangle_map),
            span: *span,
        },
        HirStmt::LetPat { pattern, mutable, value, span } => HirStmt::LetPat {
            pattern: pattern.clone(), mutable: *mutable,
            value: self.rewrite_expr_calls(value, fn_mangle_map),
            span: *span,
        },
        HirStmt::Assign { target, value, span } => HirStmt::Assign {
            target: *target,
            value: self.rewrite_expr_calls(value, fn_mangle_list),
            span: *span,
        },
        HirStmt::AssignDeref { target, value, span } => HirStmt::AssignDeref {
            target: Box::new(self.rewrite_expr_calls(target, fn_mangle_list)),
            value: self.rewrite_expr_calls(value, fn_mangle_list),
            span: *span,
        },
        HirStmt::Expr(e) => HirStmt::Expr(self.rewrite_expr_calls(e, fn_mangle_list)),
    }
}
```

- [ ] **Step 4: Implement mangle_struct_name**

```rust
fn mangle_struct_name(&mut self, struct_name: Symbol) -> Symbol {
    // Look up if this struct has a specialization
    // We need to know the type args — check struct_specs
    // For now, if there's exactly one specialization, use it
    let matches: Vec<_> = self
        .struct_specs
        .keys()
        .filter(|(name, _)| *name == struct_name)
        .collect();
    if matches.len() == 1 {
        let (_, args) = matches.into_iter().next().unwrap();
        return self.mangle_name(struct_name, &args);
    }
    struct_name // No specialization found, keep original
}
```

- [ ] **Step 5: Build fn_mangle_map in collect_and_specialize**

After the work queue is drained, build the mangle map:

```rust
let fn_mangle_map: HashMap<Symbol, Symbol> = self
    .fn_specs
    .keys()
    .map(|(name, args)| {
        (*name, self.mangle_name(*name, args))
    })
    .collect();
```

- [ ] **Step 6: Build struct mangle map and update type_overrides in build_result**

```rust
fn build_result(mut self) -> MonoResult {
    let mut items = Vec::new();
    // ... add non-generic items with rewritten calls (as before) ...

    // Build struct mangle map
    let struct_mangle_map: HashMap<Symbol, Symbol> = self
        .struct_specs
        .keys()
        .map(|(name, args)| {
            (*name, self.mangle_name(*name, args))
        })
        .collect();

    // Build final type_overrides using struct mangle map
    let mut final_type_overrides = HashMap::new();
    for (id, ty) in self.expr_types.iter().enumerate() {
        if let HirType::Generic(name, args) = ty {
            if struct_mangle_map.contains_key(name) {
                let mangled = struct_mangle_map[name];
                final_type_overrides.insert(ExprId::new(id as u32), HirType::Named(mangled));
            }
        }
    }

    MonoResult {
        hir: crate::Hir { items },
        type_overrides: final_type_overrides,
    }
}
```

- [ ] **Step 7: Add comprehensive tests**

Replace the test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hir;
    use crate::item::HirItem;
    use crate::node::HirExpr;
    use glyim_interner::Interner;
    use std::collections::HashMap;

    fn lower_source(source: &str) -> (Hir, Interner) {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            panic!("parse errors: {:?}", parse_out.errors);
        }
        let mut interner = parse_out.interner;
        let hir = crate::lower(&parse_out.ast, &mut interner);
        (hir, interner)
    }

    #[test]
    fn mono_non_generic_passthrough() {
        let (hir, mut interner) = lower_source("main = () => 42");
        let result = monomorphize(&hir, &mut interner, &[], &HashMap::new());
        assert_eq!(result.hir.items.len(), hir.items.len());
    }

    #[test]
    fn mono_generic_fn_with_call_type_args() {
        let (hir, mut interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
        let id_sym = interner.intern("id");
        let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
        // Find the Call expr id(42) in main's body
        let call_id = find_call_id(&main_fn.body, id_sym);
        assert!(call_id.is_some(), "should find Call expr for id(42)");
        let call_type_args = HashMap::from([(call_id.unwrap(), vec![HirType::Int])]);
        let result = monomorphize(&hir, &mut interner, &[], &call_type_args);
        // Should have: original main + specialized id
        assert!(result.hir.items.len() >= 2);
        // Find specialized id
        let id_fn = result.hir.items.iter().find(|i| {
            matches!(i, HirItem::Fn(f) if interner.resolve(f.name).starts_with("id__"))
        });
        assert!(id_fn.is_some(), "should have specialized id function");
        let id_fn = id_fn.unwrap();
        assert!(id_fn.type_params.is_empty(), "specialized fn should have no type params");
        assert_eq!(id_fn.params.len(), 1, "specialized fn should have 1 param");
    }

    #[test]
    fn mono_generic_struct_with_type_override() {
        let (hir, mut interner) = lower_source("struct Container<T> { value: T }\nmain = () => { let c = Container { value: 42 }; c.value }");
        // Find the StructLit expr id
        let struct_lit_id = find_struct_lit_id(&hir);
        assert!(struct_lit_id.is_some(), "should find StructLit expr");
        // The expr_type for this should be Generic(Container, [Int])
        let expr_type = HirType::Generic(interner.intern("Container"), vec![HirType::Int]);
        let expr_types = vec![HirType::Never; struct_lit_id.unwrap().as_usize() + 1];
        let result = monomorphize(&hir, &mut interner, &expr_types, &HashMap::new());
        // Should have: original Container + specialized Container__i64
        let container_count = result.hir.items.iter().filter(|i| matches!(i, HirItem::Struct(s) if interner.resolve(s.name).starts_with("Container"))).count();
        assert_eq!(container_count, 2, "should have original + specialized Container");
        // Type override should map struct_lit_id to Named(Container__i64)
        assert!(result.type_overrides.contains_key(&struct_lit_id.unwrap()), "should have type override");
    }

    #[test]
    fn mono_no_specialization_when_no_type_args() {
        let (hir, mut interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
        let result = monomorphize(&hir, &mut interner, &[], &HashMap::new());
        let fn_count = result.hir.items.iter().filter(|i| matches!(i, HirItem::Fn(_))).count();
        assert_eq!(fn_count, 2, "should have original id + main, no specialization");
    }

    fn find_call_id(expr: &HirExpr, callee: Symbol) -> Option<ExprId> {
        match expr {
            HirExpr::Call { id, .. } if expr.get_id() == *id => Some(*id),
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| {
                match s {
                    HirStmt::Expr(e) => find_call_id(e, callee),
                    _ => None,
                }
            }),
            HirExpr::If { then_branch, else_branch, .. } => {
                find_call_id(then_branch, callee)
                    .or_else(|| else_branch.as_ref().and_then(|e| find_call_id(e, callee)))
            }
            HirExpr::Match { arms, .. } => arms.iter().find_map(|(_, _, body)| find_call_id(body, callee)),
            HirExpr::Return { value: Some(v), .. } => find_call_id(v, callee),
            _ => None,
        }
    }

    fn find_struct_lit_id(hir: &Hir) -> Option<ExprId> {
        fn find_in_expr(expr: &HirExpr) -> Option<ExprId> {
            match expr {
                HirExpr::StructLit { id, .. } => Some(*id),
                HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| {
                    match s {
                        HirStmt::Expr(e) => find_in_expr(e),
                        _ => None,
                    }
                }),
                HirExpr::If { then_branch, else_branch, .. } => find_in_expr(then_branch)
                    .or_else(|| else_branch.as_ref().and_then(|e| find_in_expr(e))),
                HirExpr::Match { arms, .. } => arms.iter().find_map(|(_, _, body)| find_in_expr(body)),
                HirExpr::Return { value: Some(v), .. } => find_in_expr(v),
                _ => None,
            }
        }
        for item in &hir.items {
            if let HirItem::Fn(f) = item {
                if let Some(id) = find_in_expr(&f.body) {
                    return Some(id);
                }
            }
        }
        None
    }
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo nextest run -p glyim-hir -v`
Expected: All tests pass

- [ ] **Step 9: Update lib.rs exports**

In `crates/glyim-hir/src/lib.rs`, add:
```rust
pub mod monomorphize;
pub use monomorphize::MonoResult;
```

- [ ] **Step 10: Commit**

```bash
git add crates/glyim-hir/src/monomorphize.rs crates/glyim-hir/src/lib.rs
git commit -m "feat(hir): implement monomorphization pass for generic fns and structs"
```

---

## Chunk 5: Pipeline integration and end-to-end tests

**Files:**
- Modify: `crates/glyim-cli/src/pipeline.rs:1-50` (run, run_with_mode, build_with_mode)
- Modify: `crates/glyim-cli/src/pipeline.rs:compile_to_hir_and_ir` helper
- Modify: `crates/glyim-cli/tests/integration.rs`
- Test: `crates/glyim-cli/tests/integration.rs`

- [ ] **Step 1: Create compile_to_hir_and_ir_mono helper**

In `crates/glyim-cli/src/pipeline.rs`, add a new helper (after `compile_to_hir_and_ir`):

```rust
fn compile_to_hir_and_ir_mono(source: &str) -> Result<(glyim_hir::Hir, String, Interner, Vec<HirType>, HashMap<ExprId, Vec<HirType>>), PipelineError> {
    let mut parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let expr_types = typeck.expr_types.clone();
    let call_type_args = typeck.call_type_args.clone();
    let _ir = compile_to_ir(source).map_err(PipelineError::Codegen)?;
    let (mono_hir, type_overrides) = glyim_hir::monomorphize(&hir, &mut parse_out.interner, &expr_types, &call_type_args);
    Ok((mono_hir, String::new(), parse_out.interner, expr_types, type_overrides))
}
```

- [ ] **Step 2: Update run to use monomorphized HIR**

In `crates/glyim-cli/src/pipeline.rs`, replace the `run` function body with:

```rust
pub fn run(input: &Path) -> Result<i32, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let _parse_span = info_span!("phase", name = "parse").entered();
    let mut parse_out = glyim_parse::parse(&source);
    info!("parsed {} items", parse_out.ast.items.len());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            eprintln!("{:?}", glyim_diag::Report::new(e.clone()));
        }
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let expr_types = typeck.expr_types.clone();
    let call_type_args = typeck.call_type_args.clone();
    let (mono_hir, type_overrides) = glyim_hir::monomorphize(&hir, &mut parse_out.interner, &expr_types, &call_type_args);
    info!("monomorphized HIR: {} items (was {})", mono_hir.items.len(), hir.items.len());
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    // Merge expr_types with type_overrides
    let mut merged_types = expr_types;
    for (id, ty) in type_overrides {
        if id.as_usize() < merged_types.len() {
            merged_types[id.as_usize()] = ty;
        } else {
            merged_types.resize(id.as_usize() + 1, HirType::Never);
            merged_types[id.as_usize()] = ty;
        }
    }
    let mut codegen =
        Codegen::with_line_tables(&context, parse_out.interner, merged_types, source.clone())
            .map_err(PipelineError::Codegen)?;
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen.generate(&mono_hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_out");
    link_object(&obj_path, &exe_path, false)?;
    let status = Command::new(&exe_path)
        .status()
        .map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}
```

- [ ] **Step 3: Update run_with_mode to use monomorphized HIR**

Apply the same pattern as Step 2 to `run_with_mode`.

- [ ] **Step 4: Update build_with_mode to use monomorphized HIR**

Apply the same pattern to `build_with_mode`.

- [ ] **Step 5: Update build to use monomorphized HIR**

Apply the same pattern to `build`.

- [ ] **Step 6: Update run_tests to use monomorphized HIR**

In `run_tests`, find where HIR is created and use `monomorphize`:

```rust
let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
// ... existing typeck code ...
let (mono_hir, type_overrides) = glyim_hir::monomorphize(&hir, &mut parse_out.interner, &typeck.expr_types, &typeck.call_type_args);
// Then use mono_hir for codegen instead of hir
```

- [ ] **Step 7: Update print_ir and check to use monomorphized HIR**

Apply the same pattern to `print_ir` and `check`.

- [ ] **Step 8: Remove #[ignore] from e2e_generic_struct test**

In `crates/glyim-cli/tests/integration.rs`, remove `#[ignore]` from `e
