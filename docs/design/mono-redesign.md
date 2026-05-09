Yes, absolutely. Once you look past the "monomorphizer doing type inference" issue, the code is riddled with secondary hacks that exist solely to patch up the consequences of a weak typechecker and an ill-defined HIR contract.

Here are the additional hacks and workarounds you should refactor, categorized by the architectural violation they represent.

---

### Hack 1: The `concretize_type` and `rewrite_stmt` Fixpoint Loops
**Location:** `rewrite.rs` (`rewrite_stmt` for `LetPat`) and `build_result.rs`

In `rewrite_stmt`, there is a loop that repeatedly calls `concretize_type` until the type stops changing:
```rust
// rewrite.rs - rewrite_stmt
let mut concretized = self.concretize_type(&substituted);
let mut prev = concretized.clone();
loop {
    concretized = self.concretize_type(&prev);
    if concretized == prev { break; } // FIXPOINT LOOP
    prev = concretized.clone();
}
```
And in `build_result.rs`, there is a heavy-handed pass that walks the *entire output HIR* just to concretize lingering `Generic` types into `Named` mangled types.

**Why it's a hack:** This is a classic "fixpoint" hack. It admits that the substitution and rewriting passes left intermediate, half-resolved types (like `Generic("Option", [Named("i64")])`) in the tree, and attempts to brute-force resolve them by repeatedly iterating until the type settles. 

**The Fix:** If the typechecker produces fully resolved types, and the monomorphizer applies a single, mathematically sound substitution (`substitute_type`), a single pass is all that should ever be needed. You should never need a fixpoint loop. The fact that this exists means your `substitute_type` function is failing to recursively resolve inner types in a single traversal. Fix `substitute_type` and delete these loops entirely.

---

### Hack 2: The "Brute-Force" `force_substitute_as_targets` Pass
**Location:** `specialize.rs`

There is an entire tree-walking pass dedicated solely to fixing `As` (cast) expressions:
```rust
// specialize.rs
// Brute-force pass: walk the body and replace any As target type
// that matches a type parameter with its concrete type.
if !sub.is_empty() {
    mono.body = Self::force_substitute_as_targets(mono.body, &sub);
}
```
**Why it's a hack:** This pass exists because the standard `substitute_expr_types` walk deliberately skipped or failed to substitute the `target_type` inside `HirExpr::As`. Instead of fixing the root cause, someone hacked in a second, complete tree traversal just for casts.

**The Fix:** `substitute_expr_types` should handle *all* types uniformly. There is no reason an `As` expression should be special. Ensure `substitute_expr_types` matches `HirExpr::As` and applies `substitute_type` to the `target_type`, and then delete `force_substitute_as_targets` and `force_substitute_stmt_as` completely.

---

### Hack 3: The "Exactly One Specialization" Fallback in Call Rewriting
**Location:** `rewrite.rs` (`rewrite_expr` for `Call`)

When rewriting a call, if the monomorphizer can't find the exact mangled name in the function map, it gives up and just grabs *whatever* it can find:
```rust
// rewrite.rs - rewrite_expr
// Fallback 2: if exactly one specialization exists for this callee, use it
let new_callee = new_callee.unwrap_or_else(|| {
    let matches: Vec<_> = fn_map.iter().filter(|((sym, _), _)| sym == callee).collect();
    if matches.len() == 1 {
        *matches[0].1 // JUST PICK THE ONLY ONE
    } else {
        *callee // OR GIVE UP AND USE THE GENERIC NAME
    }
});
```
**Why it's a hack:** This is deeply semantically incorrect. If a generic function is instantiated with `i64` and `f64`, and a call site cannot figure out which one it is, returning the `i64` version because "it's the only one we made" will result in silent memory corruption and undefined behavior in the backend. If the monomorphizer doesn't know exactly which specialization to call, **the compiler must panic or emit an Internal Compiler Error (ICE)**.

**The Fix:** Once the typechecker is properly providing `call_type_args` for *every* generic call, `fn_map.get(&(*callee, type_args))` will always hit. The entire `Fallback 1` and `Fallback 2` logic should be deleted and replaced with an `.expect("ICE: Missing specialization for generic call")`.

---

### Hack 4: Hardcoded "Option" and "Result" Special Cases
**Location:** `specialize.rs` (`concretize_enum_variant_names`) and `context.rs` (`concretize_type`)

The monomorphizer contains hardcoded string checks for standard library types:
```rust
// specialize.rs
let name = self.interner.resolve(*enum_name).to_string();
if name == "Option" || name == "Result" { ... }
```
**Why it's a hack:** The compiler should not know that `Option` and `Result` are special. They are user-defined enums in the prelude. Hardcoding them makes it impossible to compile the language without these specific names, and breaks the moment a user defines their own generic enum.

**The Fix:** The typechecker should attach the concrete generic arguments to *every* `EnumVariant` and `StructLit` expression (via `expr_types` or `call_type_args`). The monomorphizer should look at the structured `HirType::Generic("Option", [Int])` attached to the expression, not try to reverse-engineer it by looking up the enum definition and checking if its name is "Option".

---

### Hack 5: Detecting Type Parameters by Single Uppercase Letter
**Location:** `context.rs` (`has_unresolved_type_param`)

```rust
// context.rs
pub(crate) fn has_unresolved_type_param(&self, ty: &HirType) -> bool {
    match ty {
        HirType::Named(sym) => {
            let s = self.interner.resolve(*sym);
            s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase())
        }
        // ...
    }
}
```
**Why it's a hack:** This is a massive code smell. The compiler is determining semantic meaning (is this a type variable?) via a naming convention (is it a single capital letter?). This means a user cannot name a type `T` or `A` without the compiler treating it as a generic variable, and conversely, it breaks if you name your type params `Item` or `Key`.

**The Fix:** The HIR type system must distinguish between `HirType::Named` (concrete types like `i64`, `MyStruct`) and `HirType::Param` (or `HirType::GenericVar`, representing `T`, `K`). If a type is a parameter, it should be structurally different in the AST. Then `has_unresolved_type_param` just becomes `matches!(ty, HirType::Param)`.

---

### Hack 6: Linear O(N) Item Lookups Everywhere
**Location:** `context.rs` (`find_fn`, `find_struct`, `find_enum`, `get_fn_type_params`)

```rust
// context.rs
pub(crate) fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
    for item in &self.hir.items { ... }
    for item in &self.hir.items { ... }
    for item in &self.hir.items { ... }
}
```
**Why it's a hack:** While not a semantic hack, this is a massive performance hack. Every time the monomorphizer needs to find a function or struct, it iterates over the entire HIR array, often multiple times, and *clones* the function definition. For a large codebase, monomorphization will become quadratic or cubic in time and memory.

**The Fix:** During `MonoContext::new`, build a `HashMap<Symbol, &HirFn>` and `HashMap<Symbol, &StructDef>`. Lookups will go from O(N) to O(1), and you can stop cloning function definitions just to read them.

---

### Summary of Refactoring Actions

To clean up the monomorphizer alongside the typechecker migration:

1. **Delete the fixpoint loops.** Fix `substitute_type` to resolve deeply in one pass.
2. **Delete `force_substitute_as_targets`.** Make `substitute_expr_types` handle `As` properly.
3. **Delete the "Fallback 2" callee selection.** Replace with a hard ICE.
4. **Delete string checks for "Option"/"Result".** Rely on the structured `HirType` from the typechecker.
5. **Introduce `HirType::Param`.** Stop detecting generics by uppercase single-letter strings.
6. **Build HashMap indices.** Stop iterating `self.hir.items` in O(N) loops.

By the end of this refactoring, your monomorphizer should shrink by about 40-50%. It will consist of:
1. A queue of `(Symbol, Vec<HirType>)` from the typechecker.
2. A single pass to substitute those types and generate specialized HIR nodes.
3. Mangled name generation. 

To design a modern, performant monomorphizer from the ground up, we need to look at how state-of-the-art compilers like **Rust (rustc)**, **Swift**, and **Modern ML-family compilers (like OCaml 5/Flambda2)** handle this. 

The overarching philosophy of modern monomorphization is: **The Typechecker produces a fully resolved, unambiguous graph of types; the Monomorphizer simply walks that graph and stamps out concrete copies.**

Here is the state-of-the-art architecture you should adopt, designed to eliminate all the hacks and make the compiler highly performant.

---

### 1. Demand-Driven Discovery (The Work-Queue Model)
Your current monomorphizer scans the entire HIR upfront (`scan_expr_for_generic_calls`, `scan_hir_for_type_instantiations`) which is O(N²) and requires multiple fixpoint iterations.

**State of the art:** Monomorphization should be a **lazy, demand-driven BFS traversal** starting from the program's entry point (`main`).

1.  **Seed the Queue:** Start with `main()`. It has no type arguments.
2.  **Process Item:** Pop an item from the queue. Substitute its type arguments into its body.
3.  **Discover Edges:** As you substitute, whenever you encounter a call to a generic function `foo<T>` where `T` is now concretely known (e.g., `foo<i64>`), compute the concrete signature.
4.  **Deduplicate & Enqueue:** If `foo<i64>` has not been emitted yet, add it to the work queue.
5.  **Terminate:** When the queue is empty, you are done. You never even look at dead code or unused generic branches.

*Why this kills the hacks:* You no longer need `body_depends_on_type_params` or `infer_from_same_var_in_block`. If a type isn't concretely known at the call site during this walk, it's a type error, not something to infer later.

### 2. Type Interning and Structural Hashing
Your current code compares `Vec<HirType>` and does O(N) linear scans of the HIR arrays to find functions/structs. This is catastrophically slow for large codebases.

**State of the art:** All types must be **interned** and represented as compact integer IDs, not recursive tree pointers.

1.  Create a `Ty` type that is just a `u32` index into a global arena.
2.  `Generic("Vec", [Int])` becomes a single `u32`.
3.  Specialization maps become `HashMap<(SymbolId, Vec<TyId>), SymbolId>`, which is blindingly fast to hash and compare (just comparing integers).
4.  Lookups (`find_fn`, `find_struct`) must use `HashMap`s built during HIR lowering, not `for item in self.hir.items` loops.

### 3. Strict Phase Separation: Typing vs. Substitution
In rustc, the typechecker outputs a `TypeckResults` map. This map contains the **fully resolved type** for every expression `ExprId` in the AST. There are no "unresolved" generics left by the time mono runs.

**State of the art:** 
*   **Typechecker:** Resolves all types. Populates `expr_types: HashMap<ExprId, Ty>`. If it cannot resolve a generic, it emits a `TypeError` and aborts. It *never* leaves a generic ambiguous.
*   **Monomorphizer:** Does zero type inference. It takes the `Ty` from the typechecker, applies its current substitution, and if the result is concrete, emits it.

*Why this kills the hacks:* No more `call_type_args_overrides`. No more fallbacks. The monomorphizer becomes a pure functional transformation: `Substitute(Item, Subst) -> Item`.

### 4. MIR (Medium-Level IR) Monomorphization
Right now, you are monomorphizing the HIR directly. This means you are walking high-level syntax (blocks, loops, match arms, string literals) just to substitute a type in a struct field.

**State of the art:** Lower the HIR to a **MIR (Monomorphized Intermediate Representation)** *before* monomorphizing. 
1.  MIR flattens control flow into basic blocks and simple statements (`Assign`, `Call`, `Cast`).
2.  Generic MIR is generated once.
3.  Monomorphization simply clones the MIR basic blocks and substitutes the type variables in the `Call` and `Assign` instructions. 
This dramatically reduces the size of the tree the monomorphizer has to walk and makes the subsequent codegen phase drastically simpler.

### 5. Symbolic Resolution (No String Mangling in the Core)
Your code parses `"__"` strings to figure out what function to call. This is brittle and slow.

**State of the art:** The HIR must use structured references, not names.
*   Instead of `callee: Symbol` (where Symbol is a string like `"Vec_new"`), use `callee: DefId` (a unique integer pointing to the definition in the HIR).
*   Instead of generating a mangled string name during mono, generate a **new `DefId`** for the specialized function.
*   String mangling (e.g., `Vec_new__i64`) is deferred entirely to the LLVM/Backend emission phase, purely for assembly labeling.

---

### The Redesigned Architecture

Here is what your new pipeline should look like:

#### Phase 1: Typechecking (The Authority)
*   Input: HIR
*   Output: `TypeCheckOutput` containing:
    *   `expr_types: Vec<Ty>` (Indexed by ExprId. EVERY expression has a fully resolved, concrete `Ty`, or an Error).
    *   `impl_resolutions: HashMap<ExprId, DefId>` (For method calls, exactly which `impl` block method was resolved).
*   **Rule:** If `expr_types[id]` contains an unresolved type parameter, the typechecker failed.

#### Phase 2: HIR Lowering to MIR (Optional but highly recommended)
*   Input: HIR
*   Output: Generic MIR (Basic blocks, DefId references).

#### Phase 3: The Demand-Driven Monomorphizer
*   **Data Structures:**
    *   `WorkQueue: Vec<(DefId, Subst)>`
    *   `Emitted: HashMap<(DefId, Subst), DefId>` (Maps generic item + subst to its new concrete DefId).
    *   `Interner` (For types and symbols).
*   **Algorithm:**
```rust
fn monomorphize(entry_point: DefId) -> MonoResult {
    let mut queue = WorkQueue::new();
    let mut emitted = HashMap::new();
    
    // 1. Seed
    queue.push((entry_point, Subst::empty()));

    // 2. BFS Loop
    while let Some((def_id, subst)) = queue.pop() {
        if emitted.contains_key(&(def_id, subst.clone())) {
            continue; // Already stamped this out
        }

        // Generate a new unique ID for this concrete function
        let concrete_def_id = new_def_id(); 
        emitted.insert((def_id, subst.clone()), concrete_def_id);

        // Fetch the generic item (IR or HIR)
        let generic_item = fetch_item(def_id);

        // Substitute the body. 
        // As we walk the body, whenever we see a Call/StructLit:
        let concrete_body = substitute(generic_item.body, &subst, |new_item_def_id, new_subst| {
            // Callback: we found a dependency!
            // If the new_subst is fully concrete, add it to the queue.
            if new_subst.is_fully_concrete() {
                queue.push((new_item_def_id, new_subst));
            } else {
                // ICE: Internal Compiler Error. The typechecker failed us.
                panic!("ICE: Unresolved generic in mono");
            }
        });

        // Store the concrete body for codegen
        save_concrete_item(concrete_def_id, concrete_body);
    }

    MonoResult { items: emitted }
}
```

### Why this design annihilates your existing hacks:
1.  **No `concretize_type` loops:** The `substitute` function is purely recursive. If you substitute `T -> i64` in `Option<T>`, it directly yields `Option<i64>`. No fixpoints.
2.  **No `force_substitute_as_targets`:** The `substitute` function handles *all* nodes uniformly. `As` expressions are not special.
3.  **No string parsing:** Everything operates on `DefId` (integers). Method calls are resolved by the typechecker into exact `DefId`s.
4.  **No O(N) scans:** Demand-driven discovery means you only process what is actually called.
5.  **No `Int` fallbacks:** If a type is generic at the mono phase, the compiler crashes (ICE). This forces you to fix the typechecker properly.

By adopting this model, your monomorphizer goes from being the smartest, buggiest part of the compiler to the dumbest, most reliable part. It simply becomes a "stamp and queue" machine.
**No, you do not have to rewrite your codegen right now.** 

Introducing a MIR is the "gold standard" architecture, but it is a massive undertaking. You can—and should—fix the monomorphization hacks **without** changing the IR level your codegen consumes. 

The core problem with your monomorphizer is not that it operates on HIR; the problem is that it does type inference, uses string parsing, and runs unbounded fixpoint loops. You can implement the **demand-driven, typechecker-is-authority** architecture directly on the HIR. 

Here is the pragmatic path forward that fixes your compiler today without requiring a codegen rewrite:

### The Pragmatic Approach: Fix HIR Mono Now

Keep your codegen exactly as it is: walking a monomorphized HIR. But restructure the HIR monomorphizer to act like a modern pass.

1. **The Typechecker's New Contract:** The typechecker guarantees that for *every* expression involving generics, `expr_types[expr_id]` contains a fully concrete type (e.g., `Generic("Vec", [Int])`), or it emits a `TypeError`. No more ambiguous parameters.
2. **The Monomorphizer's New Algorithm:** Instead of scanning the whole HIR upfront, implement the BFS queue on HIR nodes.
3. **The Output:** The mono still spits out a `Vec<HirItem>` where all generics are replaced by concrete `Named` types, which your existing codegen happily eats up.

Instead of substituting MIR basic blocks, you will substitute `HirExpr` trees, but you will do it in a single, clean, deterministic pass.

### Why this is still a massive win
If you apply the demand-driven BFS algorithm to your HIR, you will delete ~60% of your current monomorphizer code. The loops, the string hacks, the `Int` fallbacks—gone. Your HIR mono will become a simple "find and replace" engine for type variables, driven entirely by a queue.

---

### The Long-Term Path: When to introduce MIR?

You should only introduce MIR when your compiler starts needing **optimizations** that are impossible or awkward on a tree-based HIR. 

Right now, your codegen likely walks the HIR and directly emits LLVM IR / assembly. This means you are doing **no mid-level optimizations** (like dead code elimination, inlining of non-generic functions, or constant folding). 

When you are ready to add optimizations, here is the exact migration path you should follow so you aren't overwhelmed:

#### Step 1: Lower *after* Mono (The easiest transition)
Instead of rewriting your codegen immediately, insert a "HIR Lowering" pass right between Monomorphization and Codegen.

*   **Old Pipeline:** HIR -> Mono (produces HIR) -> Codegen (walks HIR)
*   **New Pipeline:** HIR -> Mono (produces HIR) -> **Lower to MIR** -> Codegen (walks MIR)

At this stage, the MIR is just a flatter representation of your HIR. Your codegen gets simpler because it doesn't have to handle nested `if` expressions or complex `match` arms anymore (they become basic blocks and jumps). Mono still works on HIR.

#### Step 2: Move Mono to MIR (The performance payoff)
Once your codegen is stable on MIR, *then* you move the monomorphizer to operate on MIR.

*   **Final Pipeline:** HIR -> Typecheck -> **Lower to MIR** -> Mono (stamps out MIR) -> Optimize MIR -> Codegen

This is the Rust compiler architecture. Why is it so fast? Because substituting a flat MIR basic block (which is just an array of simple instructions) is exponentially faster than recursively cloning and rewriting an `HirExpr` syntax tree with massive nested `Block` and `Match` nodes.

### Summary Recommendation

**Do not write a MIR yet.** 

Your immediate priority should be restoring the compiler's invariants:
1. Fix the typechecker to output concrete types.
2. Rewrite the HIR monomorphizer to be a demand-driven BFS queue that blindly trusts the typechecker.
3. Leave the codegen walking the HIR.

Once that is stable, if you find that compiling large files is slow because recursive `HirExpr` cloning takes too much memory, *that* is the day you start writing a MIR lowerer.
Here is the formal specification for the new Monomorphizer. This design enforces strict phase separation, eliminates all inference hacks, and uses a demand-driven work queue to ensure optimal performance.

---

# Monomorphizer v2 Specification

## 1. Architectural Principles

1. **The Authority Principle:** The Typechecker is the single source of truth for all types. The Monomorphizer **MUST NOT** infer types, guess types, default types to `Int`, or walk blocks looking for variable usages.
2. **The Demand-Driven Principle:** The Monomorphizer **MUST NOT** scan the entire HIR upfront. It starts from known entry points (e.g., `main`) and discovers dependencies dynamically via a Breadth-First Search (BFS) work queue.
3. **The Determinism Principle:** Type substitution is a pure, single-pass mathematical transformation. There are no fixpoint loops. If a type remains generic after substitution, it is an Internal Compiler Error (ICE).
4. **The Structural Resolution Principle:** Function and method lookups are performed via structured indices (e.g., `HashMap<Symbol, ...>`), never by parsing mangled string names (e.g., `find("__")`).

---

## 2. Preconditions (The Typechecker Contract)

Before the Monomorphizer runs, the `TypeCheckOutput` MUST satisfy:

1. `expr_types: Vec<HirType>`: Fully populated. For any expression involving generics, the type MUST be fully concrete (e.g., `Generic("Vec", [Int])`), NOT an unresolved parameter.
2. `call_type_args: HashMap<ExprId, Vec<HirType>>`: MUST contain an entry for **every** `Call`, `MethodCall`, and `StructLit` that requires generic arguments. If the typechecker could not resolve these, it MUST have emitted a `TypeError` and aborted compilation.
3. Method calls MUST be resolved to a specific callee or reported as an error.

---

## 3. Core Data Structures

```rust
/// Represents a fully resolved substitution mapping: TypeParam -> ConcreteType
pub type Subst = HashMap<Symbol, HirType>;

/// A request to stamp out a concrete instance of an item
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct WorkItem {
    def_id: Symbol,       // The original, unmangled name (e.g., "Vec", "new")
    kind: ItemKind,       // Fn, Struct, or Enum
    type_args: Vec<HirType>, // The concrete type arguments
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum ItemKind {
    Fn,
    Struct,
    Enum,
}

pub struct MonoContext {
    // --- Inputs ---
    hir: Hir,
    interner: Interner,
    expr_types: Vec<HirType>,
    call_type_args: HashMap<ExprId, Vec<HirType>>,
    
    // --- Indices (Built once during init) ---
    fn_index: HashMap<Symbol, HirFn>,
    struct_index: HashMap<Symbol, StructDef>,
    enum_index: HashMap<Symbol, EnumDef>,
    impl_index: HashMap<(Symbol, Symbol), HirFn>, // (TypeSymbol, MethodSymbol) -> HirFn

    // --- Work Queue & Deduplication ---
    work_queue: VecDeque<WorkItem>,
    emitted: HashSet<WorkItem>,

    // --- Outputs ---
    mangle_table: MangleTable,
    output_items: Vec<HirItem>,
    type_overrides: HashMap<ExprId, HirType>,
}
```

---

## 4. The Algorithm (Demand-Driven BFS)

### Phase 1: Initialization
1. Build `fn_index`, `struct_index`, `enum_index`, and `impl_index` from `hir.items`. This replaces all `find_fn` / `find_struct` O(N) linear scans.
2. Seed the work queue with the program entry point (e.g., `main`).

### Phase 2: The BFS Loop
```text
WHILE work_queue IS NOT EMPTY:
    POP item FROM work_queue
    
    IF item IN emitted: CONTINUE
    INSERT item INTO emitted

    MATCH item.kind:
        Fn      -> process_fn(item.def_id, item.type_args)
        Struct  -> process_struct(item.def_id, item.type_args)
        Enum    -> process_enum(item.def_id, item.type_args)
```

### Phase 3: Processing & Discovery

#### `process_struct(name, type_args)`
1. Lookup generic struct in `struct_index`. If missing, ICE.
2. Construct `Subst` mapping `struct.type_params` -> `type_args`.
3. Clone the struct definition. Substitute all field types using `Subst`.
4. **Discover:** For each substituted field type, extract any `Generic(Base, Args)` and push `WorkItem(Base, Args, Struct/Enum)` to the queue.
5. Mangle the name. Add the concrete struct to `output_items`.

#### `process_enum(name, type_args)`
1. Identical logic to `process_struct`, but substituting variant field types.

#### `process_fn(name, type_args)`
1. Lookup generic fn in `fn_index` or `impl_index`. If missing, ICE.
2. Construct `Subst` mapping `fn.type_params` -> `type_args`.
3. Clone the function definition.
4. Substitute param types and return type using `Subst`.
5. **Walk the body** (`walk_expr`): Perform single-pass substitution of the body AND discover new dependencies.

### Phase 4: Expression Substitution & Discovery (`walk_expr`)

This function recursively walks an `HirExpr`, applying the current `Subst`, and extracting new `WorkItem`s to enqueue. It replaces all previous `scan_expr_*` passes.

*   **`HirExpr::Call { id, callee, args }`**
    *   Look up `call_type_args[id]`. If missing, ICE ("Typechecker failed to resolve call").
    *   Apply `Subst` to the call's type args to get `concrete_args`.
    *   **Discover:** Push `WorkItem(callee, concrete_args, Fn)` to the queue.
    *   Rewrite the `callee` symbol to the mangled name.
    *   Recursively `walk_expr` on args.

*   **`HirExpr::MethodCall { id, receiver, method_name, args }`**
    *   Get `receiver_ty` from `expr_types[receiver.get_id()]`. Apply `Subst`.
    *   Extract the base type name from `receiver_ty` (e.g., `Generic("Vec", _)` -> "Vec").
    *   Look up the method in `impl_index[(base_name, method_name)]`.
    *   Look up `call_type_args[id]`. Apply `Subst` to get `concrete_args`.
    *   **Discover:** Push `WorkItem(method_name, concrete_args, Fn)` to the queue.
    *   **Desugar:** Convert `MethodCall` into a standard `Call` with the `receiver` prepended to `args` and the mangled callee name.
    *   Recursively `walk_expr` on args.

*   **`HirExpr::StructLit { id, struct_name, fields }`**
    *   Look up `expr_types[id]`. It must be `Generic(struct_name, type_args)`.
    *   Apply `Subst` to get `concrete_args`.
    *   **Discover:** Push `WorkItem(struct_name, concrete_args, Struct)` to the queue.
    *   Rewrite `struct_name` to the mangled name.
    *   Recursively `walk_expr` on field values.

*   **`HirExpr::EnumVariant { id, enum_name, variant_name, args }`**
    *   Look up `expr_types[id]`. It must be `Generic(enum_name, type_args)`.
    *   Apply `Subst` to get `concrete_args`.
    *   **Discover:** Push `WorkItem(enum_name, concrete_args, Enum)` to the queue.
    *   Rewrite `enum_name` to the mangled name.
    *   Recursively `walk_expr` on args.

*   **`HirExpr::As { expr, target_type }`**
    *   Apply `Subst` to `target_type`. (This replaces the `force_substitute_as_targets` hack).
    *   Recursively `walk_expr` on `expr`.

*   **`HirExpr::Match { scrutinee, arms }`**
    *   Recursively `walk_expr` on `scrutinee`.
    *   For each arm: Apply `Subst` to patterns (specifically mangling `EnumVariant` patterns identically to how we mangle the `EnumVariant` expression). `walk_expr` the body.

*   *(All other expressions)*: Simply recurse into child expressions.

---

## 5. Post-Conditions (Output)

The `MonoResult` returned to the codegen MUST contain:
1. `hir: Hir`: A new HIR containing *only* concrete, monomorphized items. No `Generic` types or type parameters should exist anywhere in the output HIR.
2. `type_overrides`: A map of `ExprId -> HirType` providing the fully concrete types for expressions, which the codegen uses for layout information.

---

## 6. Explicitly Forbidden Patterns

To ensure this spec is followed, the following patterns from the old codebase are **explicitly forbidden** in the new implementation:

1. ❌ `fn infer_from_same_var_in_block` (Mono does not infer types).
2. ❌ `fn body_depends_on_type_params` (Mono does not check semantics; if TC says it's generic, we stamp it out).
3. ❌ `fn force_substitute_as_targets` (Substitution applies uniformly to all nodes).
4. ❌ `loop { concretize... }` (No fixpoint loops; single pass substitution only).
5. ❌ String parsing on `Symbol` values (e.g., `name_str.find("__")`).
6. ❌ Defaulting unknown type parameters to `HirType::Int` (Unknowns are ICEs).
7. ❌ Linear scans `for item in self.hir.items` for lookups (Must use indices).
8. ❌ Special-casing the strings `"Option"` or `"Result"` (They are just standard enums in the index).
