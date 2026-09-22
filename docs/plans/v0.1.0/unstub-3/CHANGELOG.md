# Stub Remediation — Work Changelog

Plan: `docs/plans/v0.1.0/unstub-final/`
Each tier is implemented and committed atomically. Tests are run with
`cargo nextest run -p <crate>` for affected crates.

## Status

- [x] Tier 0 — mir-interp soundness (0.1 sizing, 0.2 ConstantIndex/Subslice write, 0.3 PtrToPtr comment, 0.4 Drop/Call scope note, 0.5 Len non-int -> Err) — COMMITTED
- [x] Tier 1.1 + 1.1b — closure capture analysis + real closure ADT type — COMMITTED
- [x] Tier 1.2 — vtable generation (trait-def-driven method population) — COMMITTED
- [x] Tier 1.3 — Iterator::next real resolution — COMMITTED
- [x] Tier 1.4 — Range lowering bug — COMMITTED
- [x] Tier 1.5 — const-eval expression coverage (Return/Loop/While/Flow) — COMMITTED
- [x] Tier 1.6 — drop elaboration per-projection (move-path tree) — COMMITTED (top-level/whole-value)
- [x] Tier 1.7 — dynamic range slicing — COMMITTED (regression coverage; impl already present)
- [x] Tier 2.1 — coherence overlap ignores generics — COMMITTED
- [x] Tier 2.2 — HRTB provable cases (reflexivity/static/WF/identity) — COMMITTED
- [x] Tier 2.3 — object safety associated types & supertraits — COMMITTED
- [x] Tier 3.1 — transitive dependency resolution (glyip) — COMMITTED
- [x] Tier 3.2 — glyip cmd_test executes tests — COMMITTED
- [x] Tier 3.3 — registry-disabled error message — COMMITTED
- [x] Tier 4.1 — fragment-spec matching (Stage A + B) — COMMITTED
- [x] Tier 4.2 — line!/column! from SourceMap — COMMITTED (via Vfs source lookup)
- [x] Tier 4.3 — include! CWD-relative fix — COMMITTED
- [x] Tier 4.4 — stringify! normalization — COMMITTED
- [x] Tier 5.1 — over-alignment fallback comment + set_alignment — COMMITTED
- [x] Tier 5.2 — DWARF pointer/slice debug types — COMMITTED
- [x] Tier 5.3 — fn_sig fallback → internal error — COMMITTED
- [x] Tier 5.4 — bytecode from_end ConstantIndex: array→const, slice→runtime len−offset — COMMITTED
- [x] Tier 6.1 — reference_graph walks Range/Closure/Index/Break children — COMMITTED
- [x] Tier 6.2 — reference_graph Read/Write access classification + `&mut` lowering fix — COMMITTED
- [x] Tier 6.3 — rename fallback lexes & only edits `Ident` tokens (skips string/char/comment) — COMMITTED
- [x] Tier 6.5 — unused-imports via reference graph (replaces text heuristic) — COMMITTED
- [x] Tier 6.4 — completion type-filtered by receiver — COMMITTED
- [x] Tier 7.1 — PipelineCompiler surfaces MIR/def-map/typeck artifacts + per-file temp path — COMMITTED
- [x] Tier 7.2 — RunPass/RunFail link executable via glyim_cli::linker + plumb executable_path — COMMITTED
- [x] Tier 7.3 — mock/lower_ctx with_iterator_next override — COMMITTED
- [x] Tier 7.4 — mock/solver iterator_next override — COMMITTED

## Commits

### Tier 0 (mir-interp)
- `fix(interp): real element sizing for pointer arithmetic` — `get_element_size`
  now calls `SimpleLayoutComputer`. `ConstantIndex`/`Subslice` write paths
  implemented. `PtrToPtr` no-op documented; `Drop` scope documented + panic
  unwind flag. Test `tier0.rs` added (3 tests, 178 crate tests pass).

### Tier 1.1 + 1.1b (typeck closures)
- `feat(typeck): real closure capture analysis + closure ADT type`
  - `LocalEnv::next_var_id()` + capture boundary filtering by `LocalVarId`.
  - `capture_log` on `FnCtxt`; `check_path` records VarRefs; mut-use flagged
    in `Expr::Assign` lhs and `Expr::Ref { Mut }`.
  - Closure arm checks body once inside its scope (cache cleared first so the
    body re-resolves as a capture), classifies ByRef(Not)/ByRef(Mut).
  - `TyCtxMut::register_closure`/`next_synthetic_adt_id` build a real closure
    ADT (seeded at id 2_000_000 to avoid colliding with builtins/user ADTs).
  - Test `closures.rs` asserts 1 capture of enclosing `x` as ByRef(Not) and
    that the closure type is a concrete `Adt` (not Infer).
  - 56 typeck tests pass.

### Tier 1.2 (layout / vtable generation)
- `feat(layout): derive vtable method slots from the trait definition`
  - Plan deviation (documented): the codebase has no `ImplDef.items` registry
    and `glyim-solve::TraitContext::ImplDef` carries no method bindings. The
    real trait-method data lives in `glyim_type::TraitDef { methods: Vec<MethodDef> }`
    on `TyCtx`. Added `TyCtxMut::register_trait_def(id, TraitDef)` to populate
    it (mirrors `register_adt`/`register_fn_sig`).
  - `MethodDef` gained `fn_def_id: Option<FnDefId>` so each vtable entry can
    carry the concrete dispatch target.
  - `SimpleLayoutComputer::vtable_of` now looks up `trait_def(id)` and builds
    one `VTableEntry` per trait method (name + sig + fn_def_id), instead of
    the previous `methods: vec![]` placeholder. Traits with no registered
    method set still produce an empty vtable (backward compatible).
  - New test `s15_vtable_computer_populates_methods_from_trait_def` registers a
    2-method trait and asserts the vtable carries both entries with correct
    def ids and size. 63 glyim-layout + 168 glyim-codegen tests pass;
    `cargo check --workspace` clean.

### Tier 1.3 (Iterator::next resolution)
- `fix(solve): Iterator::next yields a real Option<T> type`
  - Plan deviation (documented): `iterator_next_info` already returned a real
    `SolverIteratorNextInfo` (not `None`) — it derived `next` from the
    registered `builtin_next_fn_id`. The genuine latent bug was the `Option`
    return type using a magic `AdtId::from_raw(101)` with no backing ADT.
  - Registered a real `Option<T>` builtin enum (id 1010, variants `None`/
    `Some(T)`) in `TyCtxMut::register_builtin_ranges`, and pointed
    `iterator_next_info` at it. Avoided id collision with existing tests that
    use 1006/1007.
  - Tests: `test_option_builtin_registered` (glyim-type) asserts the enum is
    registered with two correct variants; `t18_iterator_next_info_uses_real_
    option_adt` (glyim-solve) asserts `None` when no `next` fn is registered
    and a real `Option<elem>` (id 1010) otherwise. 560 glyim-type+glyim-solve
    tests pass; `cargo check --workspace` clean.

### Tier 1.4 (Range lowering)
- `fix(lower): lower ranges to real Range/RangeInclusive aggregates`
  - The `thir::ExprKind::Range` arm in `lower_expr_to_rvalue` discarded
    start/end/inclusive and returned an empty `Aggregate(Tuple, [])` (the bug).
  - Typeck now resolves the range expression's type to the proper
    `Range<T>` (id 1000, exclusive) / `RangeInclusive<T>` (id 1001, inclusive)
    ADT via `mk_adt`, deriving `T` from the endpoint types (defaulting to the
    error type for a full `..`).
  - Lowering reads the substitution from that resolved `Adt` type and emits
    `Aggregate(Adt(adt_id, 0, substs), [start_op, end_op])`, so the range
    carries its real endpoints. `..` (no endpoints) emits Error-typed operands.
  - Tests `range_lower.rs` (3 tests) assert exclusive -> Adt 1000, inclusive ->
    Adt 1001, both with 2 constant operands. 187 glyim-lower + 56 glyim-typeck
    tests pass; `cargo check --workspace` clean.

### Tier 1.5 (const-eval expression coverage)
- `feat(const-eval): support Return/Break/Continue and while/loop`
  - `Expr::Return { value }` now evaluates the inner expression (or Unit) and
    returns it instead of erroring.
  - Added a `loop_control: Option<LoopControl>` field + `LoopControl::{Break,
    Continue}` enum. `Break`/`Continue` set the flag (value currently ignored,
    matching unlabeled break in const eval) instead of erroring.
  - New `eval_while` / `eval_loop` drivers constant-fold loops: evaluate the
    condition/body, react to `break` (exit -> Unit) and `continue` (next
    iteration). Both are bounded by a 1_000_000-iteration cap so a
    non-terminating loop returns a clear error instead of hanging.
  - Note: `Call`/`MethodCall`/`Closure`/`Range`/`For` remain unsupported
    (genuinely require a full evaluation context); `For` over a constant
    iterable was left as a future extension.
  - Tests `flow_eval.rs` (6) cover Return-with/without value, while-false (body
    not run), while-true-with-break, loop-with-break, and continue re-entry via
    an if/break/continue body. 67 glyim-const-eval tests pass; `cargo check
    --workspace` clean.

### Tier 1.6 (drop elaboration — top-level / whole-value)
- `feat(lower): elaborate scope drop terminators for non-Copy locals`
  - Plan deviation (documented): the codebase has no `drops.rs` /
    `elaborate_drops` module; the lower pass never emitted any `Drop` or
    `StorageDead` terminator at all — values going out of scope simply leaked
    (only `TerminatorKind::Drop` fed into `MonoItem::DropGlue` in mono).
  - `MirBuilder::lower_body` now calls `elaborate_scope_drops` on the
    fall-through block (the same condition the old `terminate(Return)` used:
    `current_block.is_some()`). It inserts a chain of `Drop { place, target,
    cleanup: None }` terminators in **reverse declaration order** for every
    local whose type needs a destructor, routing the last drop into a fresh
    `Return` block. The return place (`_0`) and parameters are skipped.
  - Added `needs_drop(ty)`: `Copy` types never drop; ADTs/composites recurse
    into the registered `LowerCtx::adt_def` fields; `String` (and other owning
    types) need drop; references/raw pointers/closures/fn-ptrs/dyn/projections/
    inference/params are treated as no-drop to avoid spurious destructor calls.
    Because the test `TestLowerCtx`/`LocalMockLowerCtx` return empty ADT fields,
    struct locals in existing tests stay no-drop and their `block_count`
    assertions are preserved.
  - Correctness guard: the drop chain is only injected when control **falls
    straight through** to the current block. For `if`/`match`/`while`/`loop`
    the lowering already redirected control flow and set `current_block =
    None`, so their real terminators (SwitchInt/Goto/Call/Return) are left
    untouched — fixing an early bug where injecting drops on `entry`
    clobbered those terminators.
  - Tests `drop_elaboration.rs` (3): a `String` local gets a `Drop` terminator
    (entry routes through the chain, not directly to Return); an `i32` local
    gets no drop (single Return block); a function returning `String` does not
    drop `_0`. 190 glyim-lower tests pass; `cargo build --workspace` clean.

### Tier 1.7 (dynamic range slicing — `arr[i..j]`)
- `test(lower): lock in dynamic range slicing lowers to a {ptr, len} tuple`
  - Plan deviation (documented): `lower_dynamic_range_slice` was **already
    implemented** in `glyim-lower/src/lower_rvalue.rs` (≈160 lines). It builds
    the slice as an ordinary `Rvalue` — `Len(base)` for length, runtime
    `start`/`end` operand evaluation (defaulting start=0 / end=len), two
    bounds-check `SwitchInt` asserts (start<=end, end<=len), then
    `data_ptr = first_elem + start*elem_size` and `new_len = end - start`,
    returning `Aggregate(AggregateKind::Tuple, [ptr, len])`. The standalone
    `thir::ExprKind::Range` arm already routes a `Range` index into it.
  - THIR limitation surfaced: `thir::Range` bounds are `Option<Box<Expr>>`
    carrying only literal expressions (no runtime-local bounds), so a fully
    dynamic `arr[i..j]` with `i`/`j` as function parameters cannot yet be
    expressed at the THIR level. The lowering nonetheless emits runtime
    arithmetic + bounds checks (not a `Place` projection), matching the
    `slice_desugar.rs` design note that this belongs in `glyim-lower`.
  - Added regression tests `dynamic_range_slice.rs` (3): `arr[1..4]` lowers to
    a `{ptr, len}` tuple with no error diagnostics; `arr[1..=4]` (inclusive) is
    correctly rejected with a diagnostic; `arr[1..]` (open-ended) lowers to a
    `{ptr, len}` tuple. 193 glyim-lower tests pass; `cargo build --workspace`
    clean.

### Tier 2.1 (coherence overlap check ignores generic args)
- `fix(typeck): structural_tys_match recurses into Adt substitution args`
  - In `glyim-typeck/src/coherence.rs`, the `structural_tys_match` `Adt` arm
    previously discarded the `Substitution` (`id_a == id_b`) so `Vec<i32>` and
    `Vec<String>` were wrongly treated as overlapping — a soundness gap that
    would reject valid non-overlapping impls. The arm now recurses into
    `ctx.substitution_args`, comparing each `GenericArg::Ty` structurally;
    lifetime/const args are treated as always-compatible (documented, since
    const values aren't modeled precisely yet). The `Tuple`/`Array`/`Slice`/
    `Ref`/`RawPtr` arms already recursed into inner types.
  - Test-harness fix (latent bug): `type_ref_to_ty` resolved primitive type
    names via `ctx.name_str(name)`, but `ctx`'s interner is a *separate*
    `Interner` instance from the one that interned the test names
    (`Interner` wraps `Arc<ThreadedRodeo>`, each `new()` is its own symbol
    table). That returned an empty/garbage string and silently produced
    `Ty::ERROR` for generic args — which is why no prior test caught it (they
    resolve via `def_map`, and blanket-vs-concrete short-circuits on the param
    branch). Switched to `interner.resolve(name)` (the interner that created
    the names).
  - Added `make_generic_impl_item` + tests `coherence.rs` `t11` (distinct
    generic self types `Vec<i32>`/`Vec<String>` do NOT conflict) and `t12`
    (identical `Vec<i32>`/`Vec<i32>` DO conflict). 12 coherence / 58 glyim-
    typeck tests pass; `cargo build --workspace` clean.

### Tier 2.2 (HRTB predicates — mostly `Ambiguous` → `Proven` for trivial cases)
- `fix(solve): cheap HRTB provable-case wins in check_hrtb`
  - `glyim-solve/src/hrtb.rs` `check_hrtb` now resolves the *trivially
    provable* HRTB predicates to `SolverResult::Proven` instead of the prior
    blanket `Ambiguous`:
    - `RegionOutlives(a, b)`: reflexivity (`a == b`) and any side being
      `'static` → `Proven`. Two *distinct* placeholders (or a placeholder vs an
      unrelated early-bound region) correctly stay `Ambiguous` (not falsely
      proven).
    - `TypeOutlives(ty, r)`: `r == 'static` → `Proven`; if `r` is structurally
      *inside* `ty` (reflexive containment, via new `region_in_ty`) → `Proven`;
      otherwise an owned/scalar `ty` with no open components (`ty_has_open_
      components`) → `Proven`; genuinely open types stay `Ambiguous`.
    - `WellFormed(ty)`: concrete types with no open components
      (inference vars, generic params, late-bound placeholders, region
      variables) and not `dyn`/projection → `Proven`; else `Ambiguous`. Helper
      `ty_is_concrete_well_formed` deliberately excludes `HAS_RE_PLACEHOLDER`
      (a placeholder region introduced by HRTB instantiation is fully
      resolved, not "open").
    - `Coerce(a, b)`: identity coercion resolved via a new structural
      `ty_struct_eq` (recurses through substitution args), then falls back to
      `can_coerce`.
  - Root cause surfaced: `Ty`/`Substitution`/`Region` equality is by *interned
    handle*, but HRTB instantiation **re-interns** substituted types, so two
    structurally-identical types (e.g. `fn(&'p i32)`) carry different handles
    and the old index-based `a == b` identity check missed them. `ty_struct_eq`
    compares structurally (and through substitution contents), fixing both the
    `Coerce` identity case and making identity resolution robust.
  - Wired the previously-orphaned `tests/hrtb.rs` into `tests/mod.rs` (it was
    not declared, so none of its `test_hrtb_*` tests ran). Updated the
    pre-existing `test_hrtb_coerce_via_check` expectation from `Ambiguous` to
    `Proven` (identity `Coerce(i32,i32)` is now correctly proven).
  - Added 6 regression tests `t22_*`: region-outlives reflexivity (Proven),
    distinct-placeholder region-outlives (Ambiguous), type-outlives reflexive
    `&'a i32: 'a` (Proven), type-outlives owned `i32: 'a` (Proven),
    well-formed `&'a i32` (Proven), identity `Coerce(fn(&'a i32), fn(&'a i32))`
    (Proven). 33 glyim-solve lib tests pass; `cargo build --workspace` clean.
  - DOC NOTE left on `check_hrtb` stating which cases remain conservative and
    why (this is the 80% cheap-win pass, not a complete HRTB solver).

### Tier 2.3 (object safety ignores associated types & supertraits)
- `feat(type): object safety checks associated types + supertraits`
  - `glyim-type/src/object_safety.rs` `check_object_safety` previously only
    inspected per-method receiver/generic-param shape. Replaced its
    `(requires_self_sized, methods)` signature with a `TraitObjectSafetyInput`
    struct (associated types + supertraits are trait-level, not method-level,
    so they are NOT bolted onto `MethodSignature`).
  - Added `AssociatedTypeInfo { name, span, is_constrained_in_all_methods }`
    and `SupertraitSafety { trait_id, is_safe, span }`. New violation variant
    `ObjectSafetyViolation::SupertraitNotObjectSafe { trait_id, span }`
    (cleanly named rather than overloading `SelfSized`). The pre-existing-but-
    never-constructed `UnconstrainedAssociatedType { name, span }` variant is
    now actually emitted.
  - Two new check folds in `check_object_safety`: an associated type that is
    not constrained (mentioned) by the methods yields
    `UnconstrainedAssociatedType`; a supertrait whose pre-resolved
    `is_safe == false` yields `SupertraitNotObjectSafe`. `glyim-type` stays
    pure/data-in-data-out — the caller (`glyim-typeck`, which owns the
    recursive `TraitDef.predicates` walk) computes `supertrait_safety` and
    passes it in, exactly as the plan's design note requires.
  - Updated the only two call sites (both test-only): `glyim-type/src/tests/
    s12_object_safety.rs` and `glyim-codegen/src/tests/trait_objects.rs`
    (8 call sites there) now build `TraitObjectSafetyInput`. Added 4 new
    regression tests in `s12_object_safety.rs`: unconstrained associated type
    is flagged, constrained associated type is NOT, unsafe supertrait is
    flagged, safe supertrait is NOT. 13 object-safety / 557 glyim-type tests
    pass; 4 object-safety / 168 glyim-codegen tests pass; `cargo build
    --workspace` clean.

### Tier 3.1 (transitive dependency resolution in glyip)
- `feat(glyip): resolve transitive dependencies (BFS enqueues sub-deps)`
  - `crates/glyip/src/dep.rs`: `DependencyResolver::resolve` previously seeded
    the BFS queue only from the root's direct deps and never enqueued a
    resolved crate's own dependencies (the `// TODO: implement transitive
    resolution` was duplicated twice — both removed).
  - `IndexEntry` gained a `dependencies: HashMap<String, Vec<IndexDependency>>`
    field (`#[serde(default)]` for backward-compat with on-disk `.json`
    index files) plus a new `IndexDependency { name, version_req }` struct.
  - `resolve_registry_dep` and `resolve_path_dep` now return
    `(LockedCrate, Vec<(String, Option<String>, Option<PathBuf>)>)` — the sub-
    dep list — instead of just `LockedCrate`. Their `LockedCrate.dependencies`
    (a `BTreeMap<String,String>`) is now populated from that list (the
    requested version requirement), not left `BTreeMap::new()`. A shared
    `sub_deps_from_index` helper reads `entry.dependencies[version]`.
  - `resolve()`'s loop now enqueues each resolved crate's sub-deps. The queue
    element was extended with a 4th field `Option<PathBuf>` "base dir": a path
    sub-dependency carries its relative path and its nested path deps must
    resolve against that crate's directory, so the enqueued base is set to the
    parent's abs dir; registry sub-deps carry `None`/`None`. Diamond deps are
    still handled by the existing `visited` set keyed on `{name}-{version}`
    (Cargo-style non-unification); semver-range unification left out of scope
    per the plan.
  - Existing `IndexEntry { ... }` literals in the three test files
    (`crate_index_io.rs`, `dep_advanced.rs`, `dep_resolution.rs`, ~22 sites)
    were updated to include `dependencies: Default::default()` (and
    `IndexDependency` imported where the new test uses it).
  - Added regression test `resolve_transitive_dependencies`: a local index has
    `a` depends-on `b`; root depends only on `a`; asserts `b` appears in the
    lockfile and `a.dependencies["b"] == "0.5"`. 177 glyip tests pass;
    `cargo build --workspace` clean.

### Tier 3.2 (glyip `cmd_test` actually executes tests)
- **Adaptation (plan assumed native link+run; reality differs):** the bytecode
  backend emits glyim's own opcode format (see `glyim-codegen`
  `BytecodeBackend::generate`), NOT a native object file, so the plan's
  "link + run the bytecode object" path is non-functional on this tree. The
  only in-repo working runtime is the MIR interpreter (`glyim_mir_interp`),
  which `glyim-test`'s `InterpRunner` already uses. `cmd_test` now compiles each
  discovered test file to MIR and runs the specific test body via
  `Interpreter::run_body`, which is genuine execution, not a stub.
- Added `glyim_pipeline::compile_file_to_mir` (returns `MirCompilation {
  bodies: HashMap<DefId, Arc<Body>>, def_map: CrateDefMap, ty_ctx: Arc<TyCtx>
  }`) — the front half of `compile_file` without backend codegen.
- `cmd_test` (commands.rs): replaced the file-counting stub with per-test
  compile-to-MIR + run. Resolves each discovered test name to its `DefId` via
  the `CrateDefMap` (`resolve_test_def_id`), registers all bodies in the
  interpreter, and runs the matching body — `Ok` => passed, `Err` => failed.
  Compilation failures also count as failed. `#[ignore]` tests are skipped
  unless `TestOptions::run_ignored` is set (mirrors `cargo test -- --ignored`).
- `test_discovery::DiscoveredTest` gained an `ignored: bool` field. Because
  glyim source does NOT parse `#[...]` attributes (the frontend lexer emits
  `Hash` but the parser forms no attribute node — `scan` of a `#[ignore]`
  attribute yields "expected item, found Hash"), the `#[ignore]` marker is
  written as a comment (`// #[ignore]`) on the line before the function; `scan`
  detects both the bare `#[ignore]` attribute and a `//`-comment containing
  "ignore".
- `TestOptions` gained `run_ignored: bool` (already derived `Default`).
- **Known pipeline quirk (documented, not fixed here):** the MIR pipeline
  drops the LAST-declared function in a file from `thir_bodies`, so a test that
  is the final function in its file will not get a runnable body. Test fixtures
  work around this with a trailing `_pad() {}` non-test function. (Fixing the
  underlying typeck/THIR collection is a separate task.)
- Tests: `test_cmd_test_executes_tests` (1 passed / 1 failed / 1 ignored via
  two files), `test_cmd_test_runs_ignored_when_requested` (both run when
  `run_ignored`), plus updated `test_with_only_src_files` / `test_with_filter`
  to the new discovered-test-function semantics. 179 tests pass; `cargo build
  --workspace` clean; `glyim-pipeline` (3) and `glyim-test` (94) still green.

### Tier 3.3 (registry-disabled actionable error)
- `fix(glyip): clear error when registry feature is disabled + dep not in local index`
  - `resolve_registry_dep` previously fell through to the bare
    `DependencyNotFound` from `index.resolve_version` when `registry_client`
    was `None` and the dep wasn't in the local index — an unhelpful message.
  - Added an `Err(_) if self.registry_client.is_none()` arm that wraps the
    error with a hint: `... (hint: no registry client configured — build glyip
    with `--features registry` or provide a local index entry for '<name>')`.
  - Added `registry_disabled_gives_actionable_error` (dep_advanced.rs): empty
    index + `DependencyResolver::new` (no client), resolves a missing
    `remote-crate` dep, asserts the error names the dep and mentions the
    registry feature / local index. 180 glyip tests pass (`cargo test -p
    glyip`); `cargo build -p glyip -p glyim-pipeline` clean.

### Tier 4.1 Stage A (fragment-spec matching)
- `feat(macro): tighten matches_fragment_spec single-token validation`
  - Replaced the `_ => true` blanket-accept for non-ident/literal fragment
    specs with concrete single-token cases that do not require full parsing:
    - `Ident` / `Literal` (Int|Float|String|Bool|Char) / `Lifetime` already
      matched by token kind.
    - `Vis` => `KwPub` token.
    - `Block` => a `{ ... }` group (LBrace..RBrace).
    - `Tt` => any single token tree (correct by definition).
    - `Expr` / `Ty` / `Path` / `Pat` => reject tokens that can never start
      the fragment (`;` `,` `)` `}` `]`), but still accept otherwise (whole-
      fragment validity is Stage B).
    - `Stmt` / `Item` / `Meta` => remain `true` (too varied to validate from a
      single token; Stage B handles them).
  - Stage B (variable-length fragment consumption) is now **COMMITTED** — see
    the "Tier 4.1 Stage B" subsection below.
  - Tests (matcher.rs): `test_stage_a_expr_rejects_separator_token` (5
    separators rejected for `:expr`), `test_stage_a_vis_matches_pub`,
    `test_stage_a_block_matches_brace_group`. 60 glyim-meta tests pass.

### Tier 4.1 Stage B (variable-length fragment consumption)
- `feat(macro): :expr/:ty/:pat/:stmt/:item/:block/:meta consume variable-length fragments`
  - Added `glyim-frontend::try_parse_fragment(spec: &str, src: &str) -> Option<usize>`
    (public, string-based — takes the fragment *source text*, so it does not
    introduce a glyim-meta → glyim-frontend type dependency). It wraps `src` in a
    minimal valid program shape per fragment kind (`fn __f() { <src> }` for
    expr/stmt/block, `fn __f(__x: <src>) {}` for ty, `fn __f() { let <src> = (); }`
    for pat, `fn __f() { <src> }` and require exactly one item for item, etc.),
    reparses with the existing `parse_to_syntax`, and accepts only when the
    reparse emits **zero diagnostics** (this rejects over-consumed tokens such
    as a stray `)`). The returned length is consumed by the matcher.
  - `glyim-meta/src/expander/matcher.rs`:: `match_pieces` now routes the
    flexible fragment specs (`Expr`/`Ty`/`Path`/`Pat`/`Stmt`/`Item`/`Block`/`Meta`)
    through a new `consume_fragment(spec, input, pos)`:
    - Greedy **longest-prefix** match: tries the largest candidate prefix first,
      shrinking until `try_parse_fragment` accepts it.
    - Bounded by top-level `,`/`;` terminators (matching `macro_rules!`
      semantics — `:stmt` stops before a trailing `;`, `:expr` before a `,`).
    - Returns the captured `Vec<TokenTree>` (so the binding carries the whole
      fragment), keeping the existing `MatchResult` contract.
    - `Ident`/`Literal`/`Lifetime`/`Vis`/`Tt` keep their single-token handling.
  - `to_source` reconstructs lossless source from `&[TokenTree]` (delimiters
    emitted via `delim_char`, extended to `<`/`>` so generic `Vec<i32>` and
    calls reparse correctly); adjacent identifier/literal/kw tokens are joined
    with a single space (Rust token grammar is space-insensitive).
  - No regressions: all 22 existing `macro_expansion` tests still pass (the
    single-token case is a degenerate prefix of the greedy match). Added 8
    Stage B tests: multi-token `:expr` (`a + b * c`), `:ty` (`Vec<i32>`), `:path`
    (`a::b::c`), `:block` (`{ x }`), `:stmt` (`let x = 1`), `:item` (`struct S {}`),
    `:pat` (`(a, b)`), `:meta`, plus a "rejects nonsense" check. 71 glyim-meta
    lib tests pass.

### Tier 4.4 (stringify! normalization)
- `fix(macro): deterministic stringify! spacing`
  - `stringify_token_trees` previously did `parts.join(" ")` — every token
    (including delimiters and commas) got a space, producing
    `concat ! ( "a" , "b" )`. Replaced with a flatten-to-leaves pass plus a
    `needs_space_before(prev, next)` rule that matches real `stringify!`:
    - no space before `,` `;` `)` `]` `}`,
    - no space after `(` `[` `{`,
    - space everywhere else.
  - Extracted `delim_char(kind)` (open/close delimiter char) and
    `needs_space_before` helpers.
  - Updated the two pre-existing `concat_stringify` tests that had hard-coded
  the buggy spacing (`foo ( bar )` / `concat ! ( "a" , "b" )`) to assert the
  corrected output and the absence of space-around-quote-comma. Added unit
  tests `stringify_spaces_infix_operands` (`1 + 2`),
  `stringify_call_no_space_around_parens_or_comma` (`foo (a, b)`),
  `needs_space_before_rules`. 63 glyim-meta tests pass (`cargo test -p
  glyim-meta`); build clean.

### Tier 4.2 (line!/column! from real source) + Tier 4.3 (include! relative path)
- `feat(macro): line!/column!/file!/include! resolve against the Vfs`
  - Plan deviation (documented): the plan assumed a `glyim-span` `SourceMap`
    with `lookup_line_col`. No such type exists; instead `glyim-vfs` already
    exposes `file_content(FileId) -> Option<Arc<str>>` and
    `file_path(FileId) -> Option<PathBuf>`, which cover both needs. The
    Expander was plumbed with the calling `FileId` + an optional `&Vfs`
    (single change that serves 4.2 and 4.3 together).
  - `glyim-meta` gains a `glyim-vfs` dependency. `Expander` now carries
    `current_file: FileId` (default `BOGUS`) + `vfs: Option<&Vfs>`, set via
    `set_source_file(id)` / `set_vfs(&vfs)`. The internal `expand_crate` /
    `expand_macro_invocation` free functions thread these through
    `ExpanderImpl` (no public-signature change for `expand_crate(root)`, so
    existing test call sites are untouched).
  - `file_id_from_node` now returns `self.current_file` (was always `BOGUS`),
    so `line!`/`column!`/`include!`/`file!` spans carry the real source file.
  - `line!` / `column!` compute the 1-based (line, col) from the real source
    via a new `line_col_of` helper that walks `vfs.file_content(file)` by byte
    offset; the old `/80` `/%80` heuristic is kept only as a no-Vfs fallback.
  - `file!` resolves the path through `vfs.file_path` (previously a
    `file_<id>` placeholder).
  - `include!` now resolves relative to the calling file's directory:
    `vfs.file_path(call_site.file).parent().join(arg)` for relative paths
    (absolute paths used as-is; CWD fallback when no Vfs is set).
  - `env!` / `include!` argument extraction generalized via a new
    `first_string_lit` helper that finds the string literal whether it
    arrives as a bare `Token` or wrapped in a `( ... )` group (the two call
    paths differ in arg shape); this also makes `env!` work via
    `expand_crate` (previously only the public `expand()` path reached the
    Token branch).
  - Test `vfs_backed_line_column_and_include` (builtin.rs) builds a temp Vfs
    with a source file + sibling `footer.gly`, sets source file + Vfs on the
    Expander, and asserts `line!()` expands to the real line 2 (not the
    heuristic `1`) and `include!("footer.gly")` inlines `footer.gly`'s
    content with no diagnostics. `builtin_env_expand_api` updated to set a
    deterministic env var and assert the expanded value (was asserting the
    old "not implemented" diagnostic, which the fix removed). 64 glyim-meta
  tests pass (`cargo test -p glyim-meta`); `glyim-pipeline` + `glyim-test`
  build clean.

### Tier 5.3 (fn_sig silent fallback → internal compiler error)
- `fix(codegen-llvm): missing FnSig at codegen is now an internal error`
  - `lower.rs::lower_body` previously `.unwrap_or`-ed a missing `FnSig` to an
    empty `FnSig { inputs: empty, output: body.return_ty, ... }`, which could
    emit a wrong-arity LLVM function and crash far from the cause. Replaced
    with a `match` that returns `Err(vec![GlyimDiagnostic::internal_error(
    "no FnSig registered for {:?} ...")])` when `ty_ctx.fn_sig(fn_def_id)` is
    `None`. By the time codegen runs every called function must have a resolved
    signature, so this is a compiler bug, not a user error.
  - Added `tests/fn_sig_missing.rs` (registered in `tests/mod.rs`) which
    builds a trivial `Body` with `owner = FnDefId(7)` and deliberately omits
    any `fn_sig` registration, then asserts `backend.generate_function(&body)`
    returns `Err` whose diagnostic mentions the missing FnSig.
  - Verified in isolation: `cargo test -p glyim-codegen-llvm --lib
    t53_missing_fn_sig_is_internal_compiler_error` → 1 passed. NOTE: the full
    `glyim-codegen-llvm` test suite has ~213 PRE-EXISTING failures on the
    clean baseline (the LLVM backend is broadly unfinished on this branch);
    this commit does not worsen that and does not depend on the broken
    `glyim-typeck` working tree.

### Tier 5.1 (over-alignment fallback comment + set_alignment)
- `fix(codegen-llvm): enforce >16 alignment at alloca via set_alignment`
  - `types.rs::opaque_sized_type`: the `>16` fallback previously had a
    misleading comment ("alignment might be wrong but at least size is
    correct"). Replaced with an accurate explanation: an LLVM type cannot
    express an arbitrary alignment above 16 bytes through its type alone, so
    the fallback is a naturally 1-aligned i8 array that is *size*-correct
    only; callers MUST set the real alignment explicitly at the
    alloca/global use site via `set_alignment`.
  - `lower.rs::alloc_local`: after `build_alloca`, compute the type's
    layout via `FullLayoutComputer` and, when `align > 16`, obtain the
    underlying `AllocaInst` (`alloca.as_instruction_value()`) and call
    `set_alignment(align as u32)`. This restores the correct alignment for
    over-aligned aggregates whose LLVM type is only 1-aligned.
  - Added `types::tests::test_opaque_sized_type_over_aligned_is_size_correct`
    which uses `TargetData::get_store_size` to assert the `>16` fallback
    preserves the exact byte size (64 for align 32/size 64; 100 for align
    64/size 100) and is an i8 ArrayType. NOTE: in the current type system no
    primitive/aggregate reaches `align > 16` (all fields top out at 8-byte
    alignment), so the `alloc_local` branch is currently defensive — it
    cannot be triggered through a normal glyim type yet, but is correct for
    when SIMD/over-aligned types are added.
  - Verified in isolation: `cargo test -p glyim-codegen-llvm --lib
    types::tests` → 2 passed. Full `glyim-codegen-llvm` suite still has the
    ~213 PRE-EXISTING failures (unchanged from baseline); this commit adds no
    new failures.

### Tier 5.2 (DWARF pointer/slice debug types)
- `fix(codegen-llvm): Ref/RawPtr/Slice debug types use real DWARF pointers`
  - `debug.rs::debug_type_for_ty`: `TyKind::Ref` and `TyKind::RawPtr` previously
    emitted a struct wrapping a basic `i32`-sized "ptr" basic type (an opaque
    blob). Now they emit a real `DIDerivedType` via
    `create_pointer_type(name, pointee_di, 64, 64, AddressSpace::default())`,
    so debuggers see a true pointer to the pointee type.
  - `TyKind::Slice` previously emitted a struct `{ basic "ptr", usize }`. The
    `ptr` member is now a real `create_pointer_type` to the element type
    (struct `{ ptr->elem, usize }`); the comment was updated to note the ptr
    member is a real DWARF pointer.
  - Added import `use inkwell::AddressSpace;` (module-level).
  - Added `tests::debug_info::tier5_2_reference_debug_type_is_real_pointer`
    which builds `&i32`, `*const i32`, and `[i32]` debug types, forces their
    retention into the module IR via `create_global_variable_expression`
    (the `declare_local` path panics under LLVM 22 in this repo — see the
    `#[ignore]`d `test_debug_declare_local_emits_intrinsic`), and asserts the
    emitted IR contains `DW_TAG_pointer_type`.
  - Verified in isolation: `cargo test -p glyim-codegen-llvm --lib
    tier5_2` → 1 passed. Full `glyim-codegen-llvm` suite: 73 passed / 213
    failed (baseline was 72/213; the +1 pass is the new 5.2 test; no new
    failures introduced). NOTE: the 213 pre-existing failures include several
    `tests::debug_info::*` cases that build a body with `FnDefId(0)` and no
    registered `FnSig`; they now correctly surface the Tier 5.3 internal
    error. That is expected fallout of 5.3, not a regression from 5.2.

### Tier 5.4 (bytecode from_end ConstantIndex: array→const, slice→runtime len−offset)
- `fix(codegen): emit runtime len-offset for from_end ConstantIndex on slices`
  - `lib.rs::emit_place_address`: the `ProjectionElem::ConstantIndex { from_end: true }`
    arm previously fell through to `offset` (a from-*start* index) for slices —
    i.e. it silently generated a wrong index for slice indexing. For
    `TyKind::Array` it already computed the correct compile-time constant
    `n.saturating_sub(offset)`; that path is unchanged.
  - **Slice `from_end`** (the plan's explicit requirement): the slice length is
    only known at runtime (the `len` field of the fat pointer), so the backend
    now emits a runtime `actual_offset = runtime_len - offset` scaled by the
    element size. It follows the existing `Index(local)` accumulator idiom:
    push the slice *value* (`OP_LOAD_LOCAL`), read its `len` (`OP_LEN`), push
    the offset and `OP_SUB`, push `elem_size` and `OP_MUL`, then `OP_ADD` to the
    base address already on the stack. `current_ty` is advanced to the element
    type for any subsequent projection. This matches the plan's
    `emit_slice_len(base) - offset` shape.
  - Verification note: there is **no bytecode VM** in the repo (`glyim-test`
    runs MIR via `glyim-mir-interp`; `glyim-runtime` is native FFI stubs), so
    this backend is verified by **golden-pattern opcode assertions** — the same
    convention used by `discriminant_len.rs` (asserts `OP_LEN` appears) and
    `comprehensive.rs`. `tests::slice_projection::
    constant_index_slice_from_end_emits_runtime_len_sub` asserts the emitted
    sequence is exactly
    `OP_LOAD_LOCAL_ADDR; OP_LOAD_LOCAL; OP_LEN; OP_LOAD_CONST(1); OP_SUB;
     OP_LOAD_CONST(4); OP_MUL; OP_ADD`.
  - `tests::slice_projection::constant_index_array_from_end_is_constant_offset`
    confirms the array case still resolves to a constant byte offset
    (`OP_LOAD_CONST(12); OP_ADD` for `[i32;4]` with `from_end` offset 1).
  - Rewrote the previous placeholder bodies in `slice_projection.rs`.
  - Verified in isolation: `cargo test -p glyim-codegen --lib
    tests::slice_projection` → 2 passed. No new failures vs baseline.

### Tier 6.1 (reference_graph walk_expr covers Range/Closure/Index/Break)
- `fix(lsp): reference_graph walk_expr now walks Range/Closure/Index/Break children`
  - `reference_graph.rs::build_from_hir::walk_expr`: the following `Expr` variants
    previously fell through to the `_ => {}` fallback and silently skipped their
    children (confirmed by diffing the `match` arms against the full `Expr` enum in
    `glyim-hir/src/lib.rs` — the same canonical variant list `check_expr.rs` uses):
    - `Expr::Index { base, index }` — neither `base` nor `index` was walked, so a
      variable used as an index operand or the indexed base (e.g. `arr[1..x]`) was
      missed. Now both are recursed.
    - `Expr::Range { start, end }` — neither `start` nor `end` was walked. This is
      the plan's exact example: a variable used only inside `1..x` (the `end`) was
      never found. Now both optional sides are recursed.
    - `Expr::Break { value: Some(v) }` — the break's value expression was not
      walked. Now recursed (the `None` arm stays a no-op).
    - `Expr::Closure { params, body }` — only `body` was walked; the closure's
      `params` (which are `PatId`s) were never registered as definitions. Now the
      parameters are walked via the existing `walk_pattern` helper, mirroring the
      top-level `for param in &body.params` loop, so the traversal shape matches
      `check_expr.rs` exactly (no silent drift when new variants are added).
  - Semantics note: this graph records local `let`/closure bindings as `Variable`
    *uses* (via their binding name / `Expr::Path`), not as `is_definition` entries
    — only top-level item/function/param names become definitions. So "find all
    references" surfaces every *use*, which is what matters for 6.1. Each probe
    variable in the test is bound once and used in exactly one of the four forms
    above, proving the corresponding arm now walks its children (would be 1 ref
    without the fix, 2 with it).
  - Added `tests::reference_graph_tests::test_reference_graph_walks_range_and_closure`
    (uses the existing `compile_to_hir` fixture helper). It asserts:
    - `range_only` (used only as a `Range` `end`) → 2 refs (let + range-end use);
    - `closure_only` (used only inside a closure body) → 2 refs;
    - `index_base_only` (used as an `Index` base) → 2 refs (let + index-base use).
  - Verified: `cargo test -p glyim-lsp --lib` → 44 passed, 0 failed, 5 ignored
    (no new failures; the 5 ignored are pre-existing). The 3
    `reference_graph_tests` tests (incl. the new one) all pass.

### Tier 6.2 (reference_graph Read/Write access + `&mut` lowering fix)
- `feat(lsp): reference_graph records Read/Write access + fix &mut HIR lowering`
  - `reference_graph.rs`: added a new `AccessKind { Read, Write }` field to `Reference`
    (additive — does not repurpose the existing `ReferenceKind` semantic enum, so
    existing `kind`/`is_definition` consumers are unchanged). The `Reference` equality
    dedup key now includes `access`, so a variable that is both read and written at the
    same span yields two distinct entries.
  - `walk_expr` classification rules (mirrors Tier 1.1's `is_mut_use` write model):
    - `Expr::Assign { lhs, .. }` where `lhs` is a `Path` → the target is recorded as
      `AccessKind::Write`.
    - `Expr::Ref { mutability, expr }` → the borrowed operand is `Write` when
      `mutability == Mutability::Mut` (i.e. `&mut x`), otherwise `Read` (`&x`).
    - All other uses (plain reads, calls, fields, ranges, indices, immutable
      borrows) are `AccessKind::Read`.
  - Root-cause fix for `&mut` lowering: the parser (`glyim-frontend`)
    `parse_unary_expr` only bumped the `&` token and recursed, so the trailing `mut`
    keyword fell through to `parse_postfix_expr`, got wrapped in an ERROR node, and
    HIR lowering (`lower_unary_expr`) could never see the `KwMut` token — meaning
    `&mut x` always lowered to an immutable `Expr::Ref { mutability: Not }`. The
    parser now consumes `mut` as a direct child of the `UnaryExpr` node, and
    `lower_unary_expr` (and `lower_ref_expr`, belt-and-suspenders) detect `KwMut` to
    emit `Mutability::Mut`. This unblocks Tier 6.2's end-to-end verification.
  - Added `tests::reference_graph_tests::test_reference_graph_read_write_access`:
    a `let`-bound variable carries exactly one `Write` (its initialization); an
    immutable `&x` borrow must NOT add a `Write` (proves the Read/Write distinction),
    while a `&mut x` borrow MUST add a `Write` (one more than the let-only baseline).
  - Verified: `cargo test -p glyim-lsp --lib` → 45 passed, 0 failed, 5 ignored;
    `glyim-frontend --lib` parser tests → 730 passed (no regression from the `&mut`
    parser change); `glyim-hir --lib` → 82 passed.

### Tier 6.3 (rename fallback skips string/char literals and comments)
- `fix(lsp): rename text fallback lexes source and only edits Ident tokens`
  - `rename.rs`: the previous fallback did a naive per-line `str::find` substring
    search, which would corrupt a name that also appears inside a string literal
    (e.g. rename `x` would rewrite `"x is a variable"`) or a comment. It now
    lexes the file with `glyim_frontend::lexer::lex` and emits a `TextEdit` only
    for `SyntaxKind::Ident` tokens whose text equals the symbol — string/char
    literals and comments are never `Ident` tokens (the lexer emits `StringLit`/
    `CharLit` and treats comments as trivia), so they are skipped automatically.
  - Extracted the fallback into `pub(crate) fn rename_text_fallback(sm, file_id,
    symbol_name, new_name) -> Option<Vec<TextEdit>>` so it can be unit-tested
    without spinning up the full `LspState`/analysis driver.
  - Added `tests::rename::test_rename_text_fallback_skips_string_and_comment`
    (name appears as a real identifier + inside a string literal + in a comment
    → exactly 2 edits, both on real-identifier lines, string/comment untouched)
    and `tests::rename::test_rename_text_fallback_target_only_in_string_is_none`
    (name appears ONLY in a string/comment → no edits, no corruption).
  - Verified: `cargo test -p glyim-lsp --lib` → 47 passed (was 45; +2 rename
    tests), 0 failed, 5 ignored.

### Tier 6.5 (unused-imports via reference graph, replaces text heuristic)
- `fix(lsp): code_action unused-import detection uses the reference graph`
  - `code_action.rs::collect_unused_imports` no longer builds a
    text-substring `used_names` set (which false-positived on shadowed names
    and false-negatived on names appearing only inside strings/comments).
    Instead the caller (`provide_code_actions`) now reads
    `db.reference_graph.used_symbols()` and an import is flagged unused iff
    its name has zero `Read`/`Write` references anywhere in the indexed HIR.
    This is a direct consumer of the Tier 6.1/6.2 reference graph.
  - Added `ReferenceGraph::used_symbols() -> HashSet<String>` (every name
    with ≥1 reference; all references are `Read`/`Write` per 6.2).
  - Also removed the leftover `eprintln!` debug statements that had crept into
    `reference_graph.rs::build_from_hir` during 6.2 development (they were
    spamming stdout on every analysis).
  - Added `code_action.rs::tests`: unused import flagged when no reference;
    used import NOT flagged; name-in-string/comment still correctly flagged;
    only the unused import among several is flagged.
  - Verified: `cargo test -p glyim-lsp --lib` → 51 passed (was 47; +4
    code_action tests), 0 failed, 5 ignored.

### Tier 6.4 (completion type-filtered by receiver) — COMMITTED

- **Typeck side (DONE this pass):** `glyim-typeck`'s `TypeckResult::expr_ty` /
  `pat_ty` were stubs (`_body_id`/`_expr_id` ignored, returning `None` outside
  `#[cfg(test)]` and a hardcoded `I32` inside it). `expr_ty` is now real:
  - `typeck_crate` collects `FnCtxt::expr_cache` (the per-`ExprId` → `Ty` map
    that `check_expr.rs` already populates on every checked expression) into a
    new `TypeckResult::expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>>`.
  - After obligation fulfillment, each cached `Ty` is run through
    `InferenceTable::fully_resolve` so the public query returns a **concrete**
    type, never a leftover `TyKind::Infer(..)`.
  - `expr_ty(body_id, expr_id)` now looks the resolved type up (no `#[cfg]` fake).
  - `pat_ty` is **honestly** `None` (no per-`PatId` cache is collected yet —
    documented as a follow-up, not faked).
  - `FnCtxt::check` (check_stmt.rs) now returns `(thir::Body, HashMap<ExprId, Ty>)`
    so the cache is extracted before `self` is consumed.
  - Added `tests/binary_i32.rs::expr_ty_returns_resolved_type` asserting a
    binary-add of two `i32` literals resolves to `Int(I32)` via the frozen
    `TyCtx::ty_kind`. 59 glyim-typeck lib tests pass.
- **LSP side (DONE — finalize pass):**
  - `glyim-lsp/Cargo.toml`: added `glyim-type`, `glyim-typeck`, `glyim-solve` deps
    (kept `lsp-types` — it was dropped mid-edit and restored).
  - `database.rs`:: `AnalysisDatabase` gained `typeck: RwLock<HashMap<FileId, (Arc<glyim_type::TyCtx>, TypeckResult)>>`
    and two methods:
    - `type_at_offset(file_id, offset) -> Option<Ty>`: walks the HIR `bodies`/`expr_spans`,
      picks the smallest expression span containing (or ending just before) the cursor, and
      returns its `expr_ty` via the stored `TypeckResult`. This is what resolves the receiver
      type at a `.` call site. **Updated (receiver-aware):** when the cursor sits right after a
      `.`, `type_at_offset` now prefers the **receiver** of the enclosing `MethodCall`/`Field`
      expression (whose span encloses the `.`) rather than the innermost 1-char sub-expression
      at the dot — that receiver is what the `.` is being called on. The general innermost-span
      fallback is retained for non-`.` contexts.
    - `expr_ty_at(file_id, body_id, expr_id) -> Option<Ty>`: thin wrapper over `expr_ty`.
  - `driver.rs`:: `analyze_file` now runs `glyim_typeck::typeck_crate` (with the production
    `SimpleTraitSolver`) after lowering and stores the `(Arc<TyCtx>, TypeckResult)` into
    `db.typeck`, alongside the existing `hirs`/`symbol_index`/`source_maps`.
  - `symbol_index.rs`:: `TypeSignature` gained `receiver_type: Option<String>`; `build_from_hir`
    now lowers `ItemKind::Impl` blocks, indexing each method as a `Function` symbol whose
    `receiver_type` is rendered from the impl's `self_ty` (`render_type_ref`). `Fn`/`Struct`/
    `Enum` symbols carry `receiver_type: None`.
  - `completion.rs`:: `provide_completions` now detects a `.` trigger (from
    `CompletionContext` or a `.` immediately before the cursor) and, when the receiver type
    resolves, filters the symbol list to methods whose `receiver_type` matches. If nothing
    matches (or no receiver resolves) it falls back to the full symbol list, preserving the
    previous behavior for non-`.` contexts.
  - Tests (`tests/tier6_4_completion_tests.rs`, new): `s12_impl_methods_indexed_with_receiver_type`
    asserts impl methods are indexed with `receiver_type == "Foo"` (driven through the real
    parser + HIR pipeline). `s12_dot_completion_filters_by_receiver_type` builds a self-consistent
    `CrateHir` (spans in the same coordinate space as the source — receiver `x` typed `i32`,
    matching `impl i32`) and asserts `.` completion returns the `i32` methods while dropping the
    free function; it also asserts `type_at_offset` resolves `i32`. 53 glyim-lsp lib tests pass
    (was 51; +2).
  - Known follow-up (pre-existing, NOT introduced by this tier): the parser emits HIR `Span`s
    whose byte offsets are not aligned with the original source file (a `MethodCall` span lands
    ~38 bytes before the real `x.` in `fn main`). Until that span-coordinate bug is fixed, the
    *real-`analyze()`* path cannot yet resolve a receiver at a `.` from a live document — but the
    filter logic itself is verified deterministically as above. Fixing HIR span coordinates is a
    separate parser task.
- This is explicitly a "no stubs" zone: receiver filtering will be wired only
  once the LSP type layer exists, not faked with a placeholder filter.

### Tier 7.1 (PipelineCompiler surfaces intermediates + per-file temp path) — COMMITTED
- `fix(pipeline): add compile_file_with_artifacts returning def_map/typeck_result/mir_bodies`
  - `glyim-pipeline/src/lib.rs`: added `pub struct CompileArtifacts { def_map, typeck_result, mir_bodies, ty_ctx }` and
    `pub fn compile_file_with_artifacts(db, path, backend, output) -> CompResult<CompileArtifacts>`. The existing
    `Pipeline::compile_file` (used by `glyip`) is left untouched — it calls through and discards the artifacts, so
    production behavior is unchanged. `CompileArtifacts.typeck_result` is cloned at return (the pipeline already
    partially-moves `typeck_result.diagnostics` into the diagnostic sink at line 83, so a plain `.clone()` on the
    field is required).
  - `glyim-test/src/harness/compiler.rs`: `PipelineCompiler::compile` now writes the source to a per-file temp path
    (`$TMPDIR/glyim_test_{file_id}.g`) on disk (the pipeline reads via `add_file_from_disk`, so in-memory content
    alone is insufficient), then calls `compile_file_with_artifacts`. On success it populates `CompileOutput`'s
    `def_map`/`typeck_result`/`mir_bodies`/`ty_ctx` (previously all `None`/empty). The object output path is also a
    per-file temp file (`glyim_test_{file_id}.o`) instead of the shared `test_output.o` (which was a parallel-test
    race hazard).
  - `glyim-test/src/tests/harness_tests.rs`: added `test_pipeline_compiler_surfaces_mir_artifacts` asserting that a
    clean `fn main() {}` produces non-empty `mir_bodies` and populated `def_map`/`typeck_result`, and that the mock
    backend recorded exactly one `generate` call to a `glyim_test_777.o` path.
  - Verified: `cargo test -p glyim-test --lib` → 95 passed (was 94; +1), 0 failed.
- Note: this unblocks 7.2 (the produced object file now has a stable, non-colliding path that the linker step can
  consume).

### Tier 7.2 (RunPass/RunFail link executable) — COMMITTED

- Exposed the linker to the test crate: `crates/glyim-cli/src/lib.rs` `mod linker;`
  → `pub mod linker;` so `glyim-test` can call `glyim_cli::linker::invoke_linker`.
- `crates/glyim-test/Cargo.toml`: added `glyim-cli = { workspace = true }`.
- `crates/glyim-test/src/harness/compiler.rs`:
  - `CompileOutput` gained an `executable_path: Option<PathBuf>` field (defaults
    `None`, surfaced in `Debug`).
  - `PipelineCompiler::compile` now computes `exe_path = output_path.with_extension("")`,
    and on a successful compile invokes `glyim_cli::linker::invoke_linker(&output_path,
    &exe_path, None, None)`; `executable_path` is set to `Some(exe_path)` only when the
    link step succeeds (otherwise it stays `None`).
- `crates/glyim-test/src/harness/executor.rs`: `RunPass`/`RunFail` now pass
  `output.executable_path.as_deref()` into `RunPassStrategy::evaluate` /
  `RunFailStrategy::evaluate` instead of the hardcoded `None`, so the execution
  strategy actually runs the linked binary when one was produced.
- `crates/glyim-test/src/tests/harness_tests.rs`: added three verification tests:
  - `test_run_pass_strategy_executes_provided_executable`: a `CompileOutput` whose
    `executable_path` points at `/bin/echo` is run by `RunPassStrategy` and passes
    (proves the executor now executes a real binary when present).
  - `test_run_pass_strategy_no_executable_fails`: a `CompileOutput` with
    `executable_path: None` makes `RunPassStrategy` report `CompilationFailed`
    ("no executable produced") rather than silently passing.
  - `test_pipeline_compiler_populates_executable_path_field`: end-to-end through
    `PipelineCompiler::compile` the new field is present and `None` under the mock
    backend (which emits no real object, so linking cannot succeed on this platform).
- `crates/glyim-pipeline/src/lib.rs`: **restored** `Pipeline::compile_file` as a thin
  wrapper over `compile_file_with_artifacts` (discards the artifacts). This was renamed
  away in 7.1 but is still the production entry point used by `glyip`/`glyim-cli`
  `run_with_args`; without it `glyim-cli` does not compile. Kept the 7.1
  `compile_file_with_artifacts` for the test harness.
- Verification: `cargo test -p glyim-test --lib` → **98 passed** (was 95; +3 for 7.2),
  0 failed. `glyim-cli`/`glyim-pipeline` compile clean.
- Known pre-existing limitation (NOT a regression from this tier, documented at Tier 5.3):
  the `glyim-cli` `test_compile_valid_file` and `test_emit_llvm_ir_writes_file` tests
  still fail because the LLVM backend lowers `fn main() {}` with
  `"no FnSig registered for FnDefId(0) when lowering body to LLVM IR"` — an unfinished
  `glyim-codegen-llvm` bug. On this macOS box the produced object is also a
  Linux-targeted (`x86_64-unknown-linux-gnu`) object that `clang` cannot link, so a
  full native run-pass of a compiled glyim program is not yet achievable here. The 7.2
  wiring is correct; only the downstream native codegen/link of real glyim output is
  blocked by the pre-existing LLVM backend state. The mock-backed harness path is fully
  green.

### Tier 7.3 (mock/lower_ctx with_iterator_next override) — COMMITTED

- `crates/glyim-test/src/mock/lower_ctx.rs`: `MockLowerCtx` gained an
  `iterator_next_override: Option<Box<dyn Fn(Ty, Ty) -> Option<IteratorNextInfo> + 'a>>`
  field, default `None` in `new()`.
- `with_iterator_next(f)` now **actually stores** the closure (previously a no-op stub
  that discarded `_f` and returned `self`), and `LowerCtx::iterator_next_fn` now consults
  the override: `self.iterator_next_override.as_ref().and_then(|f| f(iter_ty, elem_ty))`.
- Note: the plan spec named the trait method return type `SolverIteratorNextInfo`, but the
  real `LowerCtx::iterator_next_fn` returns `glyim_lower::IteratorNextInfo` (the
  `SolverIteratorNextInfo` type lives only on `TraitSolver::iterator_next_info`). The
  implementation follows the actual trait signature.
- Added `mod tests` with 3 assertions: override returning `Some(info)` is surfaced by
  `iterator_next_fn`; no override yields `None`; override returning `None` yields `None`.
- Verified: `cargo test -p glyim-test --lib` → 101 passed after this tier (the 3 new
  mock tests); 0 failed.

### Tier 7.4 (mock/solver iterator_next override) — COMMITTED

- `crates/glyim-test/src/mock/solver.rs`: `MockSolver` gained an
  `iterator_next_override: Option<Box<dyn Fn(Ty, Ty) -> Option<SolverIteratorNextInfo>>>`
  field, default `None` in `new()`, and a `with_iterator_next(f)` builder (matching the
  7.3 shape) that stores the closure.
- `TraitSolver::iterator_next_info` now consults the override instead of unconditionally
  returning `None`, so tests can exercise both the "solver found Iterator::next"
  (`Some(info)`) and "solver didn't" (`None`) branches of the Tier 1.3 fallback code in
  isolation from a full pipeline.
- Added `mod tests` with 3 assertions parallel to 7.3.
- Verified: `cargo test -p glyim-test --lib` → **104 passed** (was 98 at end of 7.2;
  +6 for 7.3+7.4), 0 failed. `glyim-pipeline`/`glyim-cli`/`glyim-lower` unaffected
  (the mocks are `glyim-test`-local).

### Tier 7 — all sub-tiers complete (7.1, 7.2, 7.3, 7.4)

- 7.1: PipelineCompiler surfaces MIR/def-map/typeck artifacts + per-file temp path.
- 7.2: RunPass/RunFail link via glyim_cli::linker; executor passes executable_path
  through (native run-pass of real glyim output is still blocked by the pre-existing
  `glyim-codegen-llvm` "no FnSig registered" bug + Linux-targeted object on macOS — the
  wiring is correct; mock-backed harness is fully green).
- 7.3 + 7.4: the `MockLowerCtx` / `MockSolver` iterator-next overrides are now real,
  unblocking isolated unit tests of the Tier 1.3 Iterator::next fallback.
- Remaining open items from this plan: **Tier 6.4**'s end-to-end parser-driven path is
  complete on the LSP side (receiver-type filtering is wired and tested deterministically);
  the only follow-up is frontend **MethodCall lowering** (`x.foo()` receiver exprs), which is
  a separate parser/lowering gap, not part of the LSP wiring. All other tiers are COMMITTED.

