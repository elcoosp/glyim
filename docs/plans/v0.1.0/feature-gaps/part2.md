## 1.6 Dynamic Range Slicing (`arr[i..j]`)

### Current state

`glyim-opt/src/slice_desugar.rs` is real and correct for what it claims: it
makes `ConstantIndex`/`Subslice` *terminal* in every `Place` projection
chain by splitting non-terminal occurrences into a temporary + continuation
(`desugar_place`, fully implemented, tested). Its own doc comment (top of
file) already specifies **exactly** where dynamic `arr[i..j]` support
belongs and why it can't live in this pass: `ConstantIndex`/`Subslice` only
carry `u64` constant offsets, so a runtime-bounded range can't be expressed
as a `Place` projection at all. It must be lowered as an ordinary `Rvalue`
sequence (pointer arithmetic + aggregate construction) at **THIR → MIR
build time**, in `glyim-lower`, before this pass ever runs.

### Target design

When THIR contains an index-with-range expression whose bounds are not
compile-time constants, `glyim-lower` emits:

```text
StorageLive(_ptr_tmp)
_ptr_tmp = Offset(base_ptr, Copy(i) * elem_size)      // Rvalue::BinaryOp/Cast chain
StorageLive(_len_tmp)
_len_tmp = Copy(j) - Copy(i)                          // Rvalue::BinaryOp(Sub, ..)
StorageLive(_slice_tmp)
_slice_tmp = Aggregate(Slice, [_ptr_tmp, _len_tmp])   // matches the {ptr,len} shape
                                                       // slice_desugar.rs already assumes
```

### Step-by-step instructions

**Step 0.** `grep -rn "ExprKind::Index\|thir::ExprKind" glyim-typeck/src glyim-lower/src`
to find (a) how THIR represents `arr[i..j]` today — likely as
`ExprKind::Index { base, index }` where `index` is itself a `Range`
expression — and (b) the existing THIR→MIR lowering function for
`ExprKind::Index` with a **constant** index/range (which must already exist,
since slice patterns produce `ConstantIndex`/`Subslice` upstream of
`glyim-opt`). Find that function; call it `lower_index_expr` for reference
below (rename to match reality).

**Step 1. Detect "compile-time-constant bounds" vs "dynamic bounds" at THIR
lowering time.** A range index's bounds are constant only when both the
`from` and `to` THIR sub-expressions are `ExprKind::Literal(Int(..))` (or,
more generally, can be evaluated by the existing const-evaluator — reuse
`glyim-const-eval`'s public entry point via `grep -n "pub fn eval_const\|pub fn evaluate"
glyim-const-eval/src/lib.rs` rather than re-implementing constant folding
here). If both fold to constants, keep using the existing
`ConstantIndex`/`Subslice` path unchanged (no regression). Otherwise, take
the new dynamic path below.

**Step 2. Implement `lower_dynamic_range_index` in `glyim-lower`** (new
function, colocated with `lower_index_expr`):

```rust
// glyim-lower/src/expr.rs (or wherever lower_index_expr lives — match the
// real file from Step 0)

/// Lower `base[from..to]` where `from`/`to` are NOT compile-time constants.
/// Produces `{ ptr: *T, len: usize }` slice value via explicit pointer
/// arithmetic, matching the aggregate shape `glyim-opt/src/slice_desugar.rs`
/// already assumes for its own materialized temporaries (see that file's
/// module doc comment for the exact shape to match).
fn lower_dynamic_range_index(
    builder: &mut MirBuilder,
    base: &thir::Expr,
    from: Option<&thir::Expr>,
    to: Option<&thir::Expr>,
    result_ty: Ty,
    span: Span,
) -> LocalIdx {
    let base_place = builder.lower_expr_to_place(base);
    let elem_ty = builder.ctx.element_ty_of(base_place.ty(builder.ctx, &builder.locals));
    let elem_size = builder.ctx.layout_of(elem_ty).size();

    // `from` defaults to 0, `to` defaults to `base.len()`, matching normal
    // Rust range-index-on-slice semantics — mirror whatever helper already
    // computes `base.len()` for plain (non-range) index bounds-checks
    // (grep `fn emit_len\|Rvalue::Len` in this same lowering module).
    let from_local = match from {
        Some(e) => builder.lower_expr_to_operand(e),
        None => builder.const_usize_operand(0, span),
    };
    let to_local = match to {
        Some(e) => builder.lower_expr_to_operand(e),
        None => builder.emit_len_operand(&base_place, span),
    };

    // Runtime bounds check: `from <= to && to <= base.len()`. Reuse the
    // existing bounds-check emission helper for ordinary index exprs
    // (grep `fn emit_bounds_check` — every existing `arr[i]` lowering must
    // already do this; do not duplicate the panic-call plumbing here).
    builder.emit_range_bounds_check(&base_place, &from_local, &to_local, span);

    let base_ptr = builder.alloc_local(builder.ctx.ptr_ty(elem_ty), Mutability::Not, span);
    builder.push_stmt(
        StatementKind::Assign(
            Place::new(base_ptr),
            Rvalue::Ref(base_place.clone(), BorrowKind::Shared),
        ),
        span,
    );

    let offset_bytes = builder.alloc_local(builder.ctx.usize_ty(), Mutability::Not, span);
    builder.push_stmt(
        StatementKind::Assign(
            Place::new(offset_bytes),
            Rvalue::BinaryOp(
                BinOp::Mul,
                Box::new((from_local.clone(), builder.const_usize_operand(elem_size, span))),
            ),
        ),
        span,
    );

    let ptr_tmp = builder.alloc_local(builder.ctx.ptr_ty(elem_ty), Mutability::Not, span);
    builder.push_stmt(
        StatementKind::Assign(
            Place::new(ptr_tmp),
            Rvalue::BinaryOp(
                BinOp::Offset,
                Box::new((Operand::Copy(Place::new(base_ptr)), Operand::Copy(Place::new(offset_bytes)))),
            ),
        ),
        span,
    );

    let len_tmp = builder.alloc_local(builder.ctx.usize_ty(), Mutability::Not, span);
    builder.push_stmt(
        StatementKind::Assign(
            Place::new(len_tmp),
            Rvalue::BinaryOp(BinOp::Sub, Box::new((to_local, from_local))),
        ),
        span,
    );

    let slice_tmp = builder.alloc_local(result_ty, Mutability::Not, span);
    builder.push_stmt(
        StatementKind::Assign(
            Place::new(slice_tmp),
            Rvalue::Aggregate(
                Box::new(AggregateKind::Slice), // confirm exact variant name via
                                                 // `grep -n "enum AggregateKind" glyim-mir/src`
                vec![Operand::Copy(Place::new(ptr_tmp)), Operand::Copy(Place::new(len_tmp))],
            ),
        ),
        span,
    );
    slice_tmp
}
```

If `BinOp::Offset` doesn't exist (`grep -n "enum BinOp" glyim-mir/src` or
`glyim-core/src`), use a plain pointer-to-integer cast + integer add +
integer-to-pointer cast instead (`Rvalue::Cast(CastKind::PtrToInt, ..)` /
`Rvalue::BinaryOp(BinOp::Add, ..)` / `Rvalue::Cast(CastKind::IntToPtr, ..)`)
— check `CastKind`'s variants first (`grep -n "enum CastKind" glyim-mir/src`)
since this crate's exact cast vocabulary must be matched precisely.

**Step 3. Wire it into whatever function currently rejects/panics on
dynamic-bound range indexing.** The report says today "the compiler will
error or generate invalid code (likely panic)" for this case — find that
error/panic site (`grep -rn "range.*index\|dynamic.*slic" glyim-lower/src
glyim-typeck/src`) and replace it with a call to
`lower_dynamic_range_index` gated by the Step 1 constant-detection.

### Tests

```rust
// glyim-lower/src/expr.rs (or glyim-lower/tests/*.rs, matching existing
// integration-test conventions — grep `glyim-lower/tests` first)
#[test]
fn dynamic_range_index_runtime_bounds() {
    let out = compile_and_run(r#"
        fn main() -> i32 {
            let arr = [10, 20, 30, 40, 50];
            let i: usize = 1;
            let j: usize = 4;
            let s = &arr[i..j];
            s.len() as i32
        }
    "#);
    assert_eq!(out, 3);
}

#[test]
fn dynamic_range_index_out_of_bounds_panics() {
    let result = compile_and_run_catching_panic(r#"
        fn main() {
            let arr = [1, 2, 3];
            let i: usize = 2;
            let j: usize = 5;
            let _ = &arr[i..j];
        }
    "#);
    assert!(result.is_panic());
}

#[test]
fn constant_range_index_still_uses_subslice_projection() {
    // Regression: `arr[1..3]` (both bounds literal) must still lower to
    // ProjectionElem::Subslice, NOT the new dynamic path — assert on the
    // emitted MIR text/dump, not just end-to-end behavior.
    let mir = compile_to_mir(r#"fn f(arr: &[i32; 5]) -> &[i32] { &arr[1..3] } "#);
    assert!(mir_contains_subslice_projection(&mir));
    assert!(!mir_contains_aggregate_slice_construction(&mir));
}
```

### Acceptance criteria

- [ ] `arr[i..j]` with runtime `i`/`j` compiles and runs correctly.
- [ ] Out-of-bounds dynamic ranges panic (not UB/garbage reads).
- [ ] Constant-bound ranges are unaffected (still use `Subslice`
      projections, verified by a MIR-shape test, not just behavior).
- [ ] `glyim-opt/src/slice_desugar.rs`'s doc comment's "Known gap" section is
      deleted/updated once this lands (it should now say dynamic slicing is
      implemented in `glyim-lower`, with a pointer to the exact function).

---

## 1.7 Const-Eval Cast Legality Wiring

### Current state

This is **further along than the report implies**. `glyim-const-eval/src/eval.rs`
already has an `is_valid_cast` gate wired into `eval_cast`: when
`ConstEvaluator` is constructed `with_ty_ctx` (giving it a `TyCtx` +
precomputed `primitive_tys` map), `eval_cast` calls
`glyim_type::is_valid_cast(ctx, from_ty, to_ty)` and rejects illegal casts
*before* falling through to the primitive-conversion allowlist. The
remaining, real gap: (a) `from_ty`/`to_ty` are **reconstructed from the
runtime `ConstValue`/`TypeRef`** via `ty_of_value`/`ty_of_typeref` helpers —
this loses precision versus the *actual* THIR-inferred source type (e.g. a
value that happens to equal `0i32` reconstructed from `ConstValue::Int` may
not distinguish `i32` from a `newtype` wrapping `i32` if such exist), and
(b) it is unclear (verify, don't assume) whether **every** real call site
that constructs a `ConstEvaluator` for actual user code (not just tests)
uses `with_ty_ctx` — if any production call path builds a bare
`ConstEvaluator` without a `TyCtx`, the gate silently doesn't fire for that
path, defeating the whole feature.

### Step-by-step instructions

**Step 0.** `grep -rn "ConstEvaluator::new\|ConstEvaluator::with_ty_ctx" --include=*.rs .`
across the whole workspace (every crate, not just `glyim-const-eval`) to
enumerate every construction site.

**Step 1. Audit every call site found in Step 0.** For each one NOT calling
`with_ty_ctx`, determine whether a `TyCtx` is available at that call site
(it almost certainly is — const-eval only runs after/during type-checking).
Change every production call site (leave test-only bare `::new()` calls that
intentionally test the pre-§13.2 fallback behavior) to use `with_ty_ctx`.

**Step 2. Thread the precise THIR source type instead of reconstructing it.**
Find where `Expr::Cast`/THIR's cast node is evaluated
(`grep -n "Expr::Cast\|ExprKind::Cast" glyim-const-eval/src/eval.rs
glyim-typeck/src/thir*.rs`). If THIR's `Cast` node already carries a
`from_ty: Ty` field (it should, post-type-checking — check
`glyim-typeck/src/thir.rs`'s `ExprKind::Cast` variant), plumb that `Ty`
straight into `eval_cast` as a new parameter instead of deriving it from the
value:

```rust
// Before:
fn eval_cast(&self, val: ConstValue, ty: &glyim_hir::TypeRef, span: Span) -> ConstEvalResult<ConstValue>

// After:
fn eval_cast(
    &self,
    val: ConstValue,
    from_ty: Option<Ty>, // Some(..) when the THIR Cast node supplied it (the
                         // common case in real compilation); None only for
                         // callers that still go through the legacy
                         // TypeRef-only path (e.g. some test harnesses) —
                         // those fall back to `ty_of_value` exactly as today.
    ty: &glyim_hir::TypeRef,
    span: Span,
) -> ConstEvalResult<ConstValue> {
    ...
    if let (Some(ctx), Some(map)) = (self.ty_ctx, &self.primitive_tys)
        && let Some(interner) = &self.interner
    {
        let from_ty = from_ty.or_else(|| ty_of_value(map, &val));
        if let (Some(from_ty), Some(to_ty)) = (from_ty, ty_of_typeref(map, ty, interner))
            && !glyim_type::is_valid_cast(ctx, from_ty, to_ty)
        {
            return Err(ConstEvalError::new("illegal cast rejected by is_valid_cast", span));
        }
    }
    ...
}
```

Update the single call site of `eval_cast` (find it via `grep -n
"eval_cast(" glyim-const-eval/src/eval.rs`) to pass the THIR node's
`from_ty` field through.

**Step 3. Update the stale doc comment.** The "Plan §13.2 ... a separate
change" comment block above `eval_cast` should be replaced with a short note
that the gate is wired and takes the precise THIR source type when
available, falling back to value-derived typing only for legacy callers.

### Tests

```rust
#[test]
fn const_eval_rejects_cast_typeck_would_reject() {
    // Pick a cast `is_valid_cast` rejects (e.g. casting a `&str` const to
    // `i32`, or whatever glyim's cast-legality rules actually forbid — grep
    // `glyim-type/src` for `is_valid_cast`'s doc comment for a concrete
    // illegal example) and assert `ConstEvaluator::with_ty_ctx(..).eval(..)`
    // now errors where it previously silently succeeded.
}

#[test]
fn const_eval_without_ty_ctx_keeps_legacy_allowlist_behavior() {
    // Explicit regression test for the intentional fallback path (bare
    // `ConstEvaluator::new`) — must still behave exactly as before.
}
```

### Acceptance criteria

- [ ] Every non-test `ConstEvaluator` construction site uses `with_ty_ctx`.
- [ ] `eval_cast` prefers a THIR-supplied `from_ty` over value-reconstruction
      when available.
- [ ] New tests pass; existing const-eval test suite unaffected.

---

## 1.8 Drop Elaboration — Per-Projection Move-Path Tracking

### Current state

`glyim-lower/src/builder.rs::elaborate_scope_drops` (verified in full above)
does correct **whole-local** drop elaboration: every non-Copy local
(excluding the return place and parameters) gets a `Drop` terminator chained
before `Return`, in reverse declaration order. It has **no notion of
partial initialization** — if a local's *field* was moved out earlier in the
function (`let y = x.field;` where `field` is not `Copy`), this pass still
emits `Drop(x)` unconditionally at scope exit, which double-drops
`x.field` (already moved into `y`, itself later dropped) and, depending on
how struct drop glue reads fields, may also touch already-invalid memory
for any *other* still-initialized fields if the whole-struct drop glue
doesn't independently guard per field.

### Target design

The standard, proven technique (used by rustc pre-NLL) that is right-sized
for this codebase's current single-block-drop-chain design: **boolean drop
flags**. For every local (or, more precisely, every *droppable place* that
can be partially moved — start with top-level locals and their direct
fields, which covers the common case the report calls out explicitly: "e.g.,
moving a field out of a struct"), allocate a shadow `bool` local
(`_flag_N`), initialized to `true` when the place becomes initialized
(`StorageLive`/first-assignment) and set to `false` at every `Move` of that
place (or a place that contains it). At scope exit, wrap each `Drop`
terminator's target with a flag check: `if _flag_N { drop N } else { skip }`.

This does not require full move-path lattice tracking (rustc's later,
much more complex `MoveDataBuilder`); it is a conservative, sound
approximation: a struct is dropped in full unless *the whole local* was
moved, and a *partially* moved struct still gets a full drop of its
still-owned fields by relying on the struct's own generated drop glue being
per-field-flag-aware too (Step 2 below extends flags one level into field
projections, not just whole locals, which is what the report explicitly
asks for — "which fields of a struct are initialized").

### Step-by-step instructions

**Step 0.** `grep -n "fn needs_drop\|fn lower_stmt\|Move\b" glyim-lower/src/builder.rs`
to find (a) `needs_drop` (already used by `elaborate_scope_drops`) and (b)
every place a THIR `Move` (field-move, `let y = x.field;` where field isn't
`Copy`) is lowered — that's where flags must be cleared.

**Step 1. Introduce `MirBuilder::drop_flags: HashMap<LocalIdx, LocalIdx>`**
(maps a droppable local to its shadow bool flag local), plus
`field_drop_flags: HashMap<(LocalIdx, FieldIdx), LocalIdx>` for per-field
flags on struct-typed locals whose fields independently need drop.

```rust
// glyim-lower/src/builder.rs, on MirBuilder:
pub(crate) drop_flags: std::collections::HashMap<LocalIdx, LocalIdx>,
pub(crate) field_drop_flags: std::collections::HashMap<(LocalIdx, FieldIdx), LocalIdx>,
```

Initialize both to empty in `MirBuilder::new`.

**Step 2. Allocate a flag when a droppable local is declared**, next to its
`StorageLive` (find where locals get `StorageLive` emitted — the params loop
in `lower_body` and wherever `let` statements lower, `grep -n
"StorageLive" glyim-lower/src/builder.rs`):

```rust
fn alloc_drop_flag(&mut self, owner: LocalIdx, span: Span) -> LocalIdx {
    let flag = self.alloc_local(self.ctx.bool_ty(), Mutability::Mut, span);
    self.push_stmt(
        StatementKind::Assign(Place::new(flag), Rvalue::Use(Operand::Constant(
            MirConst::from_bool(true), // adjust to the real MirConst constructor
        ))),
        span,
    );
    self.drop_flags.insert(owner, flag);
    flag
}
```

Call `alloc_drop_flag` immediately after allocating any local `l` for which
`self.needs_drop(decl_ty)` is true, at every point a local is introduced
(function params that need drop, and every `let` binding). For struct-typed
locals, additionally call a `alloc_field_drop_flags(local, ty)` variant that
walks the struct's fields (via `TyCtx`'s field-type accessor — `grep -n
"fn fields_of\|fn field_ty" glyim-type/src`) and allocates one flag per
droppable field, defaulted `true`.

**Step 3. Clear the relevant flag at every `Move`.** Find THIR `Move`
lowering (Step 0). Whenever a `Move` operand's source `Place` is:
- a bare local `l` → after emitting the move, set `self.drop_flags[&l]` (if
  present) to `false`.
- a field projection `l.field` → set `self.field_drop_flags[&(l, field)]`
  (if present) to `false`; **do not** clear the whole-local flag for `l`
  (other fields may still be live and need `l`'s remaining drop glue to
  run, guarded per-field — see Step 5).

```rust
fn clear_drop_flag_for_move(&mut self, place: &Place, span: Span) {
    match place.projection.as_ref() {
        [] => {
            if let Some(&flag) = self.drop_flags.get(&place.local) {
                self.push_stmt(
                    StatementKind::Assign(Place::new(flag), Rvalue::Use(Operand::Constant(MirConst::from_bool(false)))),
                    span,
                );
            }
        }
        [ProjectionElem::Field(f), ..] if place.projection.len() == 1 => {
            if let Some(&flag) = self.field_drop_flags.get(&(place.local, *f)) {
                self.push_stmt(
                    StatementKind::Assign(Place::new(flag), Rvalue::Use(Operand::Constant(MirConst::from_bool(false)))),
                    span,
                );
            }
        }
        _ => {
            // Deeper/mixed projections (move out of a nested field, or
            // through a deref) are conservatively NOT flag-tracked in this
            // first pass — matches "Tier 1.6" scope in the existing code's
            // own naming convention. Fall through without clearing a flag;
            // this is sound (never under-drops) but may over-drop nested
            // partial moves. Track as a follow-up, do not attempt to solve
            // arbitrary-depth move paths in this change.
        }
    }
}
```

Call this from every THIR `Move` lowering site found in Step 0.

**Step 4. Guard whole-local `Drop` terminators with their flag** in
`elaborate_scope_drops`. Change the drop-chain construction to interleave a
`SwitchInt`/`if` on the flag before each `Drop`:

```rust
let mut target = return_bb;
for local in to_drop {
    let drop_bb = self.new_block();
    self.basic_blocks[drop_bb].terminator = Terminator {
        kind: TerminatorKind::Drop { place: Place::new(local), target, /* existing fields */ },
        source_info: SourceInfo::new(span),
    };
    let next_target = if let Some(&flag) = self.drop_flags.get(&local) {
        let check_bb = self.new_block();
        self.basic_blocks[check_bb].terminator = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(flag)),
                targets: SwitchTargets::if_else(1 /* true */, drop_bb, target),
                // adjust to this crate's real SwitchTargets constructor —
                // grep `SwitchTargets::` for the existing bool-branch idiom
                // used elsewhere (e.g. lowering `if` expressions).
            },
            source_info: SourceInfo::new(span),
        };
        check_bb
    } else {
        drop_bb // no flag was allocated for this local's type (e.g. it's
                 // never partially-movable, or drop-flag allocation is
                 // still pending for it — degrade to the pre-existing
                 // unconditional-drop behavior, which is always sound,
                 // just occasionally over-eager).
    };
    target = next_target;
}
```

**Step 5. Field-level guard inside generated struct drop glue.** Find
wherever struct `Drop` terminators lower into per-field destructor calls
(likely in `glyim-lower` or `glyim-mir`'s drop-glue builder — `grep -rn
"fn build_drop_glue\|fn drop_in_place" glyim-lower/src glyim-mir/src`).
Wrap each field's destructor call the same way Step 4 wraps whole-local
drops, consulting `field_drop_flags` for that `(local, field)` pair.

### Tests

```rust
#[test]
fn partial_move_does_not_double_drop() {
    // struct S { a: Box<i32>, b: Box<i32> }
    // fn f() { let s = S { a: Box::new(1), b: Box::new(2) }; let _a = s.a;
    //          /* s.b still owned, s.a moved out */ }
    // Instrument Box's drop (or use a counting Drop-impl test type per the
    // existing drop-counting test fixtures — grep `DropCounter` across the
    // test suites) and assert exactly ONE drop of the moved-out value's
    // origin and exactly ONE drop of `s.b`, never a double-drop of `s.a`'s
    // slot and never a missed drop of `s.b`.
}

#[test]
fn whole_local_move_skips_drop_entirely() {
    // `let x = String::from("x"); let y = x;` — assert `x`'s scope-exit
    // drop is a no-op (flag false) and only `y`'s drop fires.
}

#[test]
fn no_move_case_unaffected() {
    // A function with no moves at all must produce IDENTICAL MIR drop
    // structure to before this change wherever no local's type needs
    // per-field tracking (or: behaviorally identical drop-count, if flag
    // scaffolding is unconditionally emitted — pick whichever the real
    // implementation does and assert that explicitly, don't leave it
    // ambiguous which regression contract is being protected).
}
```

### Acceptance criteria

- [ ] Field-level partial moves no longer double-drop or read
      already-moved-from fields.
- [ ] Whole-local moves still skip the moved-from local's drop entirely.
- [ ] Existing (non-partial-move) programs' drop behavior is unchanged.
- [ ] Deep/nested projections are explicitly documented as still
      conservative (never unsound, may over-drop) rather than silently
      assumed correct — update the "Tier 1.6" comment to describe the new,
      narrower remaining gap precisely.

---

## 1.9 Native Executable Entry Symbol for `--emit=exec`

### Current state

`glyip/src/commands.rs` has a test,
`compile_and_run_compiled_runs_real_binary`, marked
`#[ignore = "native compiled-exec: glyim codegen does not yet emit a
standalone \`main\`/start symbol for a bare \`cc\` link (link fails with
'undefined reference to main'); the full \`glyim run\`/\`glyim build\` path
links the runtime start object. Re-enable once codegen emits a linkable
entry point."]`. This means `glyim-codegen-llvm` *does* emit a C-ABI `main`
wrapper when `entry_main` is set (per the report, confirmed by `grep -n
"entry_main" glyim-codegen-llvm/src/lower.rs`), but `glyip`'s **bare `cc`
link** test path doesn't provide whatever the runtime start object supplies
(likely C-runtime init: `glyim_runtime_init()` call, argc/argv marshaling,
panic-hook installation — check `glyim-cli/src/linker.rs` for what object(s)
the *working* `glyim run`/`glyim build` path links in that this test's bare
`cc` invocation omits).

### Step-by-step instructions

**Step 0.** `grep -n "entry_main\|fn lower_body" glyim-codegen-llvm/src/lower.rs`
to see the exact C-ABI `main` wrapper shape currently emitted. Then `grep -n
"fn link\|runtime.*start\|crt0\|_start" glyim-cli/src/linker.rs` to find
what the **working** `glyim build` path links that the failing test's direct
`cc` invocation does not.

**Step 1. Diff the two link command lines.** Add a `tracing::debug!` (or
temporary `eprintln!`, removed before commit) that prints the full `cc`
argument list both in the working `glyim build` path and in the ignored
test's manual link step (`glyip/src/commands.rs`, search near the ignored
test for how it invokes the linker directly — `grep -n "Command::new(\"cc\")"`
or similar in that file). The difference between these two argument lists
**is** the bug; it is very likely a missing runtime object file (e.g.
`glyim_rt_start.o`, or a missing `-lglyim_runtime`) rather than anything
wrong in codegen's `main` emission itself.

**Step 2. Fix the test's link invocation** to match the working path's
object/library set exactly (do not modify codegen — the report explicitly
says codegen *does* emit the wrapper; this is a linker-invocation
completeness bug in the test's bespoke, minimal `cc` call, not a codegen
gap). If `glyip/src/commands.rs`'s test currently hand-rolls its own `cc`
command instead of reusing `glyim-cli::linker::link(..)`, **stop hand-rolling
it** — call the real linker function instead:

```rust
// glyip/src/commands.rs, near compile_and_run_compiled_runs_real_binary
#[test]
fn compile_and_run_compiled_runs_real_binary() {
    let obj_path = compile_fixture_to_object("fn main() -> i32 { 42 }");
    let exe_path = tempdir.path().join("out");
    // Reuse the SAME linker driver `glyim build` uses in production instead
    // of a bespoke `Command::new("cc")` call, so this test exercises (and
    // therefore locks in) the real link recipe rather than a hand-rolled
    // approximation of it that can drift out of sync.
    glyim_cli::linker::link(&[obj_path], &exe_path, LinkOptions::default())
        .expect("link should succeed with the real linker driver");
    let output = std::process::Command::new(&exe_path).output().unwrap();
    assert_eq!(output.status.code(), Some(42));
}
```

**Step 3. Remove the `#[ignore = ...]` attribute** once the test passes
under the real linker driver.

**Step 4.** If, after Step 1's diff, the *actual* root cause turns out to be
in codegen after all (e.g. `main`'s wrapper doesn't call a required runtime
init routine that only the working path injects separately) — do not guess;
follow whichever object is actually missing from the failing link and either
(a) fix the test to link it (preferred, if it's legitimately
runtime-support that every native binary needs and the runtime crate
already builds it as a linkable object), or (b) if codegen's `main` wrapper
itself is genuinely incomplete (e.g. it never calls
`glyim_runtime_init()`), fix `lower_body`'s wrapper emission to include that
call, matching whatever the working `glyim run` path does implicitly via a
separately-linked start object.

### Tests

The re-enabled `compile_and_run_compiled_runs_real_binary` test itself is
the acceptance test. Add one more:

```rust
#[test]
fn compile_and_run_compiled_binary_with_panic_exits_nonzero() {
    let obj_path = compile_fixture_to_object(r#"fn main() { panic!("boom"); }"#);
    // ... link via the real driver, run, assert non-zero exit and that the
    // panic message appears on stderr — validating that the standalone
    // exec path's runtime init actually wires up panic handling, not just
    // a trivial return-value case.
}
```

### Acceptance criteria

- [ ] `compile_and_run_compiled_runs_real_binary` passes, `#[ignore]`
      removed.
- [ ] The test exercises the real `glyim-cli::linker::link` path, not a
      bespoke `cc` invocation.
- [ ] A panicking compiled binary exits non-zero with the panic message on
      stderr.
- [ ] CI runs this test (confirm it isn't excluded by a `--skip` filter
      elsewhere in CI config, e.g. `.github/workflows/*.yml` — `grep -rn
      "compile_and_run_compiled" .github` if such files exist in the repo).

---
