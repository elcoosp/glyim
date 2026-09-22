# Glyim Stub Resolution Plan — Agent Execution Guide

## How to use this document

You are going to work through this file **top to bottom, one task at a time**.
Each task has:

- **File** — the file to open.
- **Anchor** — an exact string to search for (use it to confirm you're
  looking at the right code; the actual surrounding code may have drifted
  slightly from the report's snippet, so read ~30 lines around the anchor
  before editing).
- **Fix** — either an exact replacement, or, where the exact replacement
  depends on types/APIs this plan can't see directly, a precise algorithm
  plus the specific things to grep for before writing the replacement.
- **Verify** — how to confirm the task is actually done.

### Non-negotiable rules for every task

1. **Never guess a type or field name.** If a fix references a type,
   method, or field not shown verbatim in this plan, run
   `grep -rn "<name>" --include="*.rs" .` first and read the real
   definition before writing code against it.
2. **After every single task, run `cargo check --workspace` (or at minimum
   `cargo check -p <the crate you touched>`).** Do not batch multiple
   tasks together before checking. If a task doesn't compile, fix it before
   moving to the next task — do not leave the tree in a broken state
   between tasks.
3. **Never widen a `match` with a bare `_ => <old behavior>` to "fix" an
   exhaustiveness error.** If adding a variant breaks exhaustiveness
   somewhere else, go add the correct arm there too. A silent catch-all is
   the exact anti-pattern this whole plan exists to remove — reintroducing
   one anywhere is a failed task, even if it compiles.
4. **Replace `tracing::warn!`/`eprintln!` + silent-fallback with either a
   real implementation or a hard `diagnostics.push(...)` / `panic!` /
   `unreachable!`.** A loud failure during development is acceptable; a
   silent wrong answer shipped to a user is not. This applies to every task
   below even when not called out explicitly.
5. **Add a regression test for every CRITICAL and HIGH task** in that
   crate's existing test module (find it via `#[cfg(test)] mod tests` in
   the same file or a sibling `tests.rs` — follow the existing pattern in
   that crate, don't invent a new test harness).
6. Where a task depends on an earlier task in this document, it says so
   explicitly under **Depends on**. Do the phases in order.

---

## Phase 0 — Setup

0.1. `cargo check --workspace 2>&1 | tee /tmp/baseline.log` and save the
     output. This is your starting point; you'll compare against it after
     each phase to make sure you're only fixing things, not adding new
     warnings/errors.

0.2. Grep for the full list of remaining silent-fallback patterns so you
     have a complete picture before starting, since this report is a
     snapshot and may already be slightly stale:
     ```
     grep -rn "tracing::warn!\|tracing::debug!(\"STUB\|STUB:" --include="*.rs" .
     ```
     Cross-check this list against Phase 1–8 below. If you find a hit not
     covered by any task here, add a new task for it following the same
     format before continuing (don't silently skip it).

---

## Phase 1 — Isolated arithmetic/cast correctness bugs

These are all self-contained, single-function fixes with no cross-crate
dependencies. Do these first — they're the highest-severity, lowest-risk
items.

### 1.1 — Signed `>>` hardcoded to unsigned shift

**File:** `glyim-codegen-llvm/src/lower.rs`, `lower_binop()`, `BinOp::Shr` arm.

**Anchor:**
```rust
self.builder.build_right_shift(l_int, r_int, false, "shr")
```

**Problem:** `build_right_shift`'s third argument is `sign_extend: bool`
(inkwell). It's hardcoded `false` (always logical/unsigned shift), so
`-8i32 >> 1` will produce a large positive number instead of `-4`.

**Fix:** `lower_binop` needs to know whether the *operand type* is signed.
Currently `lower_binop(&self, op: BinOp, l: BasicValueEnum, r: BasicValueEnum)`
only has LLVM values, which are signless — there is no way to recover
signedness from `l`/`r` alone. Change the signature to also take the
operand's semantic `Ty`:

```rust
fn lower_binop(
    &self,
    op: BinOp,
    l: BasicValueEnum<'ctx>,
    r: BasicValueEnum<'ctx>,
    operand_ty: Ty,   // NEW: the Ty of `l` (and `r`, for arithmetic ops both operands share a type)
) -> CompResult<BasicValueEnum<'ctx>> {
```

At the one call site (in `lower_rvalue`, the `Rvalue::BinaryOp` arm),
compute `operand_ty` from the left operand:
```rust
Rvalue::BinaryOp(op, operands) => {
    let (left, right) = operands.as_ref();
    let l = self.lower_operand(left);
    let r = self.lower_operand(right);
    let operand_ty = self.operand_ty(left); // operand_ty() already exists in this file
    self.lower_binop(*op, l, r, operand_ty)
}
```

Add a helper (put it right above `lower_binop`):
```rust
fn is_signed_int_ty(&self, ty: Ty) -> bool {
    matches!(self.ty_ctx.ty_kind(ty), TyKind::Int(_))
}
```
(Confirm `TyKind::Int(_)` vs `TyKind::Uint(_)`/`TyKind::Float(_)` are the
actual variant names by grepping `enum TyKind` in `glyim-type/src/ty.rs`
before writing this — use whatever the real variants are.)

Then in the `BinOp::Shr` arm:
```rust
BinOp::Shr => {
    if l.is_int_value() && r.is_int_value() {
        let signed = self.is_signed_int_ty(operand_ty);
        self.builder
            .build_right_shift(l_int, r_int, signed, "shr")
            .map(|v| v.as_basic_value_enum())
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("shr failed: {:?}", e))])
    } else {
        Err(vec![GlyimDiagnostic::internal_error("shr: expected integer types")])
    }
}
```

**Depends on:** none, but do this in the same pass as 1.2 since both touch
`lower_binop`'s signature.

**Verify:** add a codegen test: a function computing `-8i32 >> 1`, run it
through the interpreter or a compiled+executed binary, assert result is
`-4` not a large positive number.

---

### 1.2 — Div/Rem always signed, even for unsigned types

**File:** `glyim-codegen-llvm/src/lower.rs`, `lower_binop()`, `BinOp::Div`
and `BinOp::Rem` arms.

**Anchor:**
```rust
self.builder.build_int_signed_div(l_int, r_int, "div")
...
self.builder.build_int_signed_rem(l_int, r_int, "rem")
```

**Fix:** using the same `operand_ty` parameter added in 1.1:
```rust
BinOp::Div => {
    if l.is_int_value() && r.is_int_value() {
        let result = if self.is_signed_int_ty(operand_ty) {
            self.builder.build_int_signed_div(l_int, r_int, "div")
        } else {
            self.builder.build_int_unsigned_div(l_int, r_int, "div")
        };
        result
            .map(|v| v.as_basic_value_enum())
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("div failed: {:?}", e))])
    } else if l.is_float_value() && r.is_float_value() {
        // unchanged float path
        ...
    } else {
        Err(vec![GlyimDiagnostic::internal_error("div: mismatched types")])
    }
}
```
Same pattern for `BinOp::Rem` using `build_int_unsigned_rem`.

**Verify:** test `4000000000u32 / 3` (a value that would be negative if
misinterpreted as signed `i32`) gives the correct unsigned quotient.

---

### 1.3 — Unknown binary operator silently becomes `Add`

**File:** `glyim-hir/src/lower/lower_expr.rs`, `lower_bin_op_token()`.

**Anchor:**
```rust
_ => {
    tracing::warn!("STUB: unknown bin op {:?}", token.text());
    BinOp::Add
}
```

**Note:** this is the *exact same bug class* previously found and fixed in
`glyim-lower`'s HIR→MIR path. Its reappearance here (a different file,
`glyim-hir`, i.e. earlier in the pipeline: syntax→HIR) means the fix needs
to be applied at *this* layer too — check first whether `lower_bin_op_token`
in `glyim-lower` (if it still exists there) is now dead code that should be
deleted, or whether there are genuinely two separate token→BinOp mapping
functions at two different pipeline stages that both need every operator.
Grep:
```
grep -rn "fn lower_bin_op_token" --include="*.rs" .
```
Fix **every** hit the same way:

```rust
fn lower_bin_op_token(token: &SyntaxToken) -> BinOp {
    match token.text() {
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "/" => BinOp::Div,
        "%" => BinOp::Rem,
        "==" => BinOp::Eq,
        "!=" => BinOp::Ne,
        "<" => BinOp::Lt,
        ">" => BinOp::Gt,
        "<=" => BinOp::LtEq,
        ">=" => BinOp::GtEq,
        "&&" => BinOp::And,
        "||" => BinOp::Or,
        "&" => BinOp::BitAnd,
        "|" => BinOp::BitOr,
        "^" => BinOp::BitXor,
        "<<" => BinOp::Shl,
        ">>" => BinOp::Shr,
        other => unreachable!(
            "parser produced a binary-expr node with unrecognized operator \
             token {:?} -- parser and HIR lowering are out of sync, this is \
             a compiler bug, not a user error",
            other
        ),
    }
}
```

**Verify:**
```
grep -rn "STUB: unknown bin op" --include="*.rs" .
```
must return zero results anywhere in the repo. Add a test compiling
`a & b`, `a | b`, `a ^ b`, `a << b`, `a >> b` and asserting the lowered
HIR/MIR `BinOp` is the correct variant (not `Add`) for each.

---

### 1.4 — Float-to-float cast mapped to `IntToFloat`

**File:** `glyim-lower/src/builder/lower_expr.rs`, cast-kind selection for
`thir::ExprKind::Cast`.

**Anchor:**
```rust
(TyKind::Float(_), TyKind::Float(_)) => CastKind::IntToFloat,
```

**Problem:** this needs a real `FloatToFloat` cast kind, which likely
doesn't exist yet. Check:
```
grep -n "pub enum CastKind" -A 10 glyim-mir/src/lib.rs
```

**Step A — add the variant** (in `glyim-mir/src/lib.rs`):
```rust
pub enum CastKind {
    IntToInt,
    FloatToInt,
    IntToFloat,
    FloatToFloat,   // NEW
    PtrToPtr,
    FnPtrToPtr,
    PtrToInt,
    IntToPtr,
}
```

**Step B — fix the selection site** in `glyim-lower/src/builder/lower_expr.rs`:
```rust
(TyKind::Float(_), TyKind::Float(_)) => CastKind::FloatToFloat,
```

**Step C — handle it in codegen.** `glyim-codegen-llvm/src/lower.rs`,
`lower_cast()`, add an arm (LLVM has separate truncate/extend for floats
just like ints — use bit width comparison the same way `CastKind::IntToInt`
already does):
```rust
CastKind::FloatToFloat => {
    if val.is_float_value() && target_llvm_ty.is_float_type() {
        let float_val = val.into_float_value();
        let target_float_ty = target_llvm_ty.into_float_type();
        // Compare bit widths to decide trunc vs ext. FloatType doesn't
        // expose bit width directly in inkwell; compare against the
        // known f32/f64 LLVM types instead.
        let src_is_f64 = float_val.get_type() == self.context.f64_type();
        let target_is_f64 = target_float_ty == self.context.f64_type();
        if src_is_f64 == target_is_f64 {
            Ok(float_val.as_basic_value_enum()) // same width, no-op
        } else if !src_is_f64 && target_is_f64 {
            self.builder.build_float_ext(float_val, target_float_ty, "fpext")
                .map(|v| v.as_basic_value_enum())
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("fpext failed: {:?}", e))])
        } else {
            self.builder.build_float_trunc(float_val, target_float_ty, "fptrunc")
                .map(|v| v.as_basic_value_enum())
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("fptrunc failed: {:?}", e))])
        }
    } else {
        Err(vec![GlyimDiagnostic::internal_error("FloatToFloat cast on non-float")])
    }
}
```

**Step D** — check the bytecode backend (`glyim-codegen/src/lib.rs`) for
its own `CastKind` match; it will now fail to compile (non-exhaustive)
once the variant is added — that's correct, per rule 3 above. Add the
equivalent real handling there too (find the bytecode ISA's existing
float-truncate/extend opcodes, or add them if missing — do not add a
`_ => {}` arm).

**Verify:** `cargo check --workspace` must show a compile error at every
non-exhaustive `CastKind` match until all are handled — that's the
mechanism confirming you found every consumer. Add a test: `let x: f32 =
3.14f64 as f32;` round-trips to the correctly-truncated value.

---

### 1.5 — Bytecode backend: string constants ignore actual content

**File:** `glyim-codegen/src/lib.rs`, `emit_operand()`, `MirConstKind::String` arm.

**Anchor:**
```rust
let idx = self.intern_string("string_payload");
```

**Fix:** `MirConstKind::String(name)` carries an interned `Name` — the
same one `glyim-codegen-llvm`'s `lower_const` already resolves via
`self.ty_ctx.name_str(*name)`. Use the same lookup here:
```rust
MirConstKind::String(name) => {
    let str_content = self.ty_ctx.name_str(*name); // confirm this method/field exists on whatever ty_ctx-equivalent this backend holds
    let idx = self.intern_string(str_content);
    ...
}
```
If `BytecodeBackend` doesn't currently hold a `&TyCtx` at all (check its
struct definition), this is actually a symptom of item 27
(`FallbackLayoutProvider`) — the backend needs a real `TyCtx` reference to
do this correctly. If so, do task 8.1 (below) first, then come back to
this one.

**Verify:** compile a program with two different string literals, run
through the bytecode backend, confirm the two interned strings are
distinct and each contains its actual text (not both "string_payload").

---

### 1.6 — Bytecode backend `generate()` discards output entirely

**File:** `glyim-codegen/src/lib.rs`.

**Anchor:**
```rust
fn generate(&self, bodies: &[Arc<Body>], _output: &Path) -> CompResult<()> {
    for body in bodies {
        let _ = self.generate_function(body)?;
        }
    Ok(())
}
```

**Fix:** collect each function's generated bytecode and write a real file:
```rust
fn generate(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()> {
    let mut module_bytes: Vec<u8> = Vec::new();
    // Write whatever header/format this bytecode ISA already uses
    // elsewhere (check for an existing serialization format constant,
    // e.g. a magic number / version, before inventing one here --
    // grep for "MAGIC" or "format_version" in this crate first).
    for body in bodies {
        let fn_bytes = self.generate_function(body)?;
        module_bytes.extend_from_slice(&fn_bytes);
    }
    std::fs::write(output, &module_bytes).map_err(|e| {
        vec![GlyimDiagnostic::internal_error(format!(
            "failed to write bytecode output to {}: {}",
            output.display(),
            e
        ))]
    })?;
    Ok(())
}
```
Before writing this, check `generate_function`'s actual return type (the
report shows `let _ = self.generate_function(body)?;` which implies it
returns something — find out what and adjust accordingly; it may already
return `Vec<u8>` or a bytecode-chunk struct that needs a `.to_bytes()` call).

**Verify:** run the bytecode backend end-to-end on a trivial program,
confirm the output file exists, is non-empty, and (if there's an existing
bytecode interpreter/loader) round-trips through it successfully.

---

### 1.7 — Bytecode backend `ConstantIndex { from_end }` ignored

**File:** `glyim-codegen/src/lib.rs`, `emit_place_address()`, `ConstantIndex` arm.

**Anchor:**
```rust
let index_val = if *from_end {
    *offset // Fallback: use offset as is (will be wrong for from_end)
} else {
    *offset
};
```

**Fix:** mirror the LLVM backend's already-correct logic (see
`glyim-codegen-llvm/src/lower.rs`'s `ConstantIndex` arm in `place_ptr` —
use that as your reference implementation, translated to this backend's
bytecode-emission style instead of LLVM IR-building calls): when
`from_end` is true, the real index is `length - offset`, where `length`
is either the array's compile-time constant length, or (for a slice) must
be loaded at runtime from the slice's length field. Concretely:
```rust
let index_val = if *from_end {
    match self.ty_kind_of(current_ty) { // use whatever this backend's equivalent of ty_ctx.ty_kind is
        TyKind::Array(_, const_val) => {
            let n = const_val_to_u64(const_val); // reuse/extract the same const_val -> u64 logic used elsewhere in this file
            n.saturating_sub(*offset)
        }
        TyKind::Slice(_) => {
            // Emit bytecode to load the slice's length field at runtime,
            // then a SUB instruction: len - offset. Follow the pattern
            // this file already uses elsewhere for reading a slice's
            // length (grep "slice_len" or similar in this file for the
            // existing opcode sequence and reuse it, then emit a
            // subtract-immediate opcode).
            emit_len_sub_offset_bytecode(bc, *offset) // write this as a small local helper following existing emit_* helpers' style in this file
        }
        other => panic!("ConstantIndex on non-array/slice type {:?}", other),
    }
} else {
    *offset
};
```

**Verify:** a slice pattern test matching `[.., last]` against a
runtime-length slice (not a fixed array) produces the correct last
element, not always index 0.

---

### 1.8 — Bytecode backend `Subslice` not implemented

**File:** `glyim-codegen/src/lib.rs`, `emit_place_address()`, `Subslice` arm.

**Anchor:**
```rust
tracing::warn!("Subslice projection in bytecode backend: not fully implemented");
bc.push(OP_LOAD_CONST);
bc.extend_from_slice(&0i64.to_le_bytes());
```

**Depends on:** understand the LLVM backend's `Subslice` implementation
first (in `glyim-codegen-llvm/src/lower.rs`'s `place_ptr`) — it computes:
1. base data pointer + length (loading from the slice struct at runtime,
   or from the compile-time array length),
2. `new_data_ptr = base_ptr + from * elem_size`,
3. `new_len = from_end ? (base_len - to - from) : (to - from)`,
4. materializes `{ new_data_ptr, new_len }` into a fresh stack slot and
   returns a pointer to it (because `Subslice`'s *result* is a value, not
   just a deeper address into the same allocation).

Port the same four steps to this backend's instruction set: emit
opcodes to (1) read the base ptr/len, (2) compute the new ptr via an ADD
opcode scaled by element size, (3) compute the new len via SUB opcodes,
(4) write both into a freshly-allocated local slot (check how this
backend allocates a temporary local slot elsewhere — reuse that, don't
invent a new allocation mechanism) and push its address as the result
instead of `OP_LOAD_CONST 0`.

**Verify:** a slice-pattern test with a `..rest` binding on a runtime
slice, run through the bytecode interpreter, produces a subslice with the
correct pointer and length (not a null/zero placeholder).

---

## Phase 2 — MIR-level pattern/type gaps (depends on Phase 1.3, 1.4)

### 2.1 — Slice pattern binding is a no-op in MIR building

**File:** `glyim-lower/src/builder/lower_expr.rs`, `bind_pattern()`,
`PatternKind::Slice` arm.

**Anchor:**
```rust
thir::PatternKind::Slice { prefix: _, slice: _, suffix: _ } => {
    tracing::debug!("Slice pattern lowering skipped (typeck only)");
}
```

This is the highest-value remaining item in the whole report: it silently
drops all variable bindings for `[a, b, ..rest]`-style patterns, so any
code using them compiles but reads garbage/unbound locals. Implement it
for real:

```rust
thir::PatternKind::Slice { prefix, slice, suffix } => {
    let Some(init) = init_local else { return; };
    let init_place = glyim_mir::Place::new(init);
    let base_ty = /* the Ty of `init_local` -- look up via whatever this
        builder already uses elsewhere in bind_pattern to get a local's Ty,
        e.g. self.local_ty(init) or similar; grep for how PatternKind::Tuple
        above gets field types for the pattern of reference */;

    // Bind each fixed-position prefix element via ConstantIndex.
    for (i, sub_pat) in prefix.iter().enumerate() {
        let proj = ProjectionElem::ConstantIndex {
            offset: i as u64,
            min_length: (prefix.len() + suffix.len()) as u64,
            from_end: false,
        };
        let elem_place = self.place_with_projection(init_place.clone(), proj);
        let temp_local = self.alloc_local(sub_pat.ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(temp_local),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(elem_place)),
            ),
            span,
        );
        self.bind_pattern(sub_pat, Some(temp_local), span);
    }

    // Bind each fixed-position suffix element via ConstantIndex from the end.
    for (i, sub_pat) in suffix.iter().enumerate() {
        let proj = ProjectionElem::ConstantIndex {
            offset: (suffix.len() - i) as u64, // check the exact from-end offset convention against the LLVM backend's ConstantIndex arm (offset counted from the end, 1-based per element) before finalizing this expression
            min_length: (prefix.len() + suffix.len()) as u64,
            from_end: true,
        };
        let elem_place = self.place_with_projection(init_place.clone(), proj);
        let temp_local = self.alloc_local(sub_pat.ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(temp_local),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(elem_place)),
            ),
            span,
        );
        self.bind_pattern(sub_pat, Some(temp_local), span);
    }

    // Bind the `..rest` middle binding, if present, via Subslice.
    if let Some(rest_pat) = slice {
        let proj = ProjectionElem::Subslice {
            from: prefix.len() as u64,
            to: suffix.len() as u64,
            from_end: true,
        };
        let rest_place = self.place_with_projection(init_place.clone(), proj);
        let temp_local = self.alloc_local(rest_pat.ty, glyim_core::primitives::Mutability::Not, span);
        self.push_stmt(glyim_mir::StatementKind::StorageLive(temp_local), span);
        self.push_stmt(
            glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(temp_local),
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(rest_place)),
            ),
            span,
        );
        self.bind_pattern(rest_pat, Some(temp_local), span);
    }

    let _ = base_ty; // remove if unused once real Ty lookup is wired in
}
```

Also insert a **runtime length check** before these bindings run if
`base_ty` is a `Slice` (not a fixed-size `Array`, where the length is
already guaranteed by typeck): emit an `Assert` terminator checking
`Len(init_place) >= prefix.len() + suffix.len()` — follow the pattern used
elsewhere in this crate for emitting `Assert` terminators (grep
`TerminatorKind::Assert` in this crate for the existing helper/pattern and
reuse it rather than hand-rolling a new one).

**Depends on:** Phase 2.2 (below) if `min_length`/`from_end` field names
or `ProjectionElem::Subslice`'s exact field types differ from what's
assumed here — confirm against the real `glyim-mir` enum definition
before writing this.

**Verify:** test `let [a, b, ..rest, z] = my_slice;` (or whatever exact
syntax this language uses) and assert `a`, `b`, `rest`, `z` all hold
correct values, and that a too-short input slice hits the length-check
`Assert` instead of reading out of bounds.

---

### 2.2 — Array type lengths always `ConstRef::Error`

**File:** `glyim-hir/src/lower/lower_type.rs`, `lower_type_ref()`,
`ArrayType` handling.

**Anchor:**
```rust
SyntaxKind::LitExpr => {
    len = Some(ConstRef::Error);
}
```

**Fix:** for the common case (`[T; 5]` — a literal integer), you don't
need full const evaluation, just parse the literal token directly:
```rust
SyntaxKind::LitExpr => {
    len = lit_expr_node
        .first_token() // or however this file already extracts token text from a LitExpr node elsewhere -- check nearby code for the established pattern
        .and_then(|tok| tok.text().parse::<u64>().ok())
        .map(ConstRef::Value) // confirm this is the real variant name/shape by checking `enum ConstRef` definition first
        .unwrap_or_else(|| {
            self.diagnostics.push(GlyimDiagnostic::type_error(
                span,
                "array length literal is not a valid non-negative integer",
            ));
            ConstRef::Error
        });
    len = Some(len);
}
```
For non-literal array lengths (`[T; SOME_CONST]` or `[T; N + 1]`), that's
a separate, larger case — check whether `SyntaxKind::PathExpr` or other
expr kinds are already partially handled nearby; if not, add a case that
defers to full const evaluation via `glyim-const-eval` the same way this
plan wires it up elsewhere (see Phase 3), rather than leaving it as
`ConstRef::Error`. If genuinely out of scope for this pass, leave a
**loud, specific** diagnostic (not `ConstRef::Error` silently) explaining
exactly which array-length expression form isn't supported yet.

**Verify:** `let a: [i32; 5] = [0; 5];` has the correct length `5`
(inspect the resulting HIR/Ty, not just "it compiles"). Also test that
`[i32; -1]` or `[i32; "x"]` (invalid literal) produces a real diagnostic,
not a silent `ConstRef::Error` with no message.

---

## Phase 3 — ABI / layout correctness (depends on nothing new; independent of Phase 1–2)

### 3.1 — Enum discriminant type hardcoded to `Ty::BOOL`

**File:** `glyim-codegen-llvm/src/abi.rs` (and, per the earlier audit,
**also** `glyim-pipeline/src/mono_cache.rs`'s `discriminant_info` — check
it, since it had the identical `bool_ty()` hardcoding; fix both together
or you'll have two disagreeing sources of truth for the same enum's tag
type).

**Anchor:**
```rust
let variants_shape = VariantsShape::Multiple {
    tag: glyim_type::Ty::BOOL,
    ...
};
```

**Fix:** compute the tag type from the variant count:
```rust
let variant_count = /* number of variants for this enum -- from AdtDef */;
let tag = if variant_count <= 1 {
    ty_ctx.unit_ty() // or whatever the "no discriminant needed" type is -- check Single-variant handling nearby first
} else if variant_count <= u8::MAX as usize + 1 {
    self.u8_ty()   // add this if TyCtx doesn't already expose it -- see below
} else if variant_count <= u16::MAX as usize + 1 {
    self.u16_ty()
} else {
    self.u32_ty()
};
```

If `TyCtx` doesn't already expose `u8_ty()`/`u16_ty()`/`u32_ty()`
(matching the existing `bool_ty()`/`error_ty()`/`unit_ty()` pattern in
`glyim-type/src/ty_ctx.rs`), add them there first — they're trivial
wrappers around whatever primitive-type interning `bool_ty()` already
does, just for `TyKind::Uint(UintTy::U8)` etc. instead of `TyKind::Bool`.

**Verify:** an enum with >256 variants gets a `u16` (not `u8`, and
definitely not `bool`) tag; an enum with 2–256 variants gets `u8`. Add a
test asserting the computed `tag` type's size matches expectations for
enums of 2, 3, 300, and 70000 variants.

---

### 3.2 — ABI `PassMode::Pair` not handled

**File:** `glyim-codegen-llvm/src/abi.rs`, `fn_abi_of()`.

**Anchor:**
```rust
_ => self.llvm_type_for_ty(arg_abi.ty),
```

**Fix:** first check whether `PassMode::Pair` actually exists in
`glyim-layout`'s `PassMode` enum and what it carries (likely two
sub-types, e.g. `Pair(Ty, Ty)` for a value split across two registers,
like a fat pointer or small two-field struct passed by value). If it
exists but is unhandled at the *call site* in
`glyim-codegen-llvm/src/lower.rs`'s `lower_call` too (search for every
`match ... PassMode` in the crate), fix all of them consistently:

```rust
PassMode::Pair(ty_a, ty_b) => {
    // Represent as a two-element LLVM struct so the value is passed as
    // two consecutive register-sized fields instead of one opaque blob.
    let llvm_a = self.llvm_type_for_ty(*ty_a);
    let llvm_b = self.llvm_type_for_ty(*ty_b);
    self.context.struct_type(&[llvm_a, llvm_b], false).into()
}
```
And in `lower_call`'s argument-passing loop, add a matching arm that
extracts the two fields from the source value and pushes them as two
separate LLVM arguments (mirroring how `PassMode::Indirect` already
allocas+stores, but here you `build_extract_value` twice instead).

**Verify:** a function taking a two-field-by-value struct small enough to
be passed in registers (per this ABI's rules) round-trips correctly
through a call — write a test calling such a function and asserting both
field values survive the call.

---

## Phase 4 — Drop/dealloc wiring (depends on nothing new)

### 4.1 — `type_needs_drop` overly broad

**File:** `glyim-codegen-llvm/src/lower.rs`.

**Anchor:**
```rust
fn type_needs_drop(&self, ty: Ty) -> bool {
    match self.ty_ctx.ty_kind(ty) {
        glyim_type::TyKind::Adt(_, _) => true,
        glyim_type::TyKind::Closure(_, _) => true,
        _ => false,
    }
}
```

**Fix:** this requires a real "does this ADT have drop glue" query, which
per earlier audit doesn't exist on `AdtDef` yet. Two-part fix:

**Step A:** add a `pub has_drop_glue: bool` (or equivalent) field to
`AdtDef` in `glyim-type/src/adt_def.rs`, computed during type-checking
(wherever `AdtDef`s are constructed/registered — grep
`register_adt`/`ty_ctx_mut.rs` and wherever it's called from typeck) as:
"this ADT's own definition has a `Drop` impl, OR any of its fields'
types themselves need drop (recursively)". If there's no `Drop` trait /
impl registry to query yet, at minimum implement the recursive
"any field needs drop" check, treating any ADT containing a known
heap-owning type (`Box`, `Vec`, `String`, etc. — check how those are
represented in this type system, likely as specific `AdtId`s the runtime
recognizes) as needing drop.

**Step B:** use it here:
```rust
fn type_needs_drop(&self, ty: Ty) -> bool {
    match self.ty_ctx.ty_kind(ty) {
        glyim_type::TyKind::Adt(adt_id, _) => {
            self.ty_ctx.adt_def(*adt_id).map(|d| d.has_drop_glue).unwrap_or(true) // fail safe: true if unknown
        }
        glyim_type::TyKind::Closure(_, _) => {
            // A closure needs drop iff any of its captured upvalues do.
            // Once Phase 9.C (closure capture analysis) lands, check the
            // actual capture list here instead of this conservative default.
            true
        }
        _ => false,
    }
}
```

**Verify:** a `Copy`, all-primitive-fields struct gets **no** drop call
emitted at its `StorageDead`/scope-end point (inspect the generated LLVM
IR / MIR for the absence of a `glyim_drop_in_place` call), while a struct
containing a heap-owning field still does.

---

### 4.2 — `_dealloc_fn` created but never called

**File:** `glyim-codegen-llvm/src/lower.rs`, `TerminatorKind::Drop` handling.

This is a real gap independent of `type_needs_drop`: `glyim_drop_in_place`
runs field/variant destructors, but if the type itself owns a heap
allocation (e.g. it's a `Box<T>`-like type whose *own* backing memory,
not just its pointee's fields, needs freeing), something must also call
`glyim_dealloc(ptr, size, align)` on it.

**Fix:** determine (via the same `AdtDef`/type-kind check as 4.1, or a
dedicated "is this an owning-pointer type" check — grep how `Box`-like
types are represented, e.g. a specific lang-item `AdtId`) whether `ty`
is itself a heap-owning pointer type. If so, after the
`glyim_drop_in_place` call (which should have already run the pointee's
destructor), also call `self._dealloc_fn` with the pointer, and the
size/align of the pointee type (computed via `FullLayoutComputer`, same
pattern used throughout this file):
```rust
if self.type_is_owning_pointer(ty) {
    let pointee_ty = /* extract from TyKind, however Box<T>-equivalent exposes its inner T */;
    let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
    let layout = layout_computer.layout_of(pointee_ty).map_err(...)?;
    let size_val = self.llvm_int_type(64).const_int(layout.size.0, false);
    let align_val = self.llvm_int_type(64).const_int(layout.align.0, false);
    let dealloc_args: Vec<BasicMetadataValueEnum> = vec![ptr.into(), size_val.into(), align_val.into()];
    self.builder.build_call(self._dealloc_fn, &dealloc_args, "dealloc_call")
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("call failed: {:?}", e))])?;
}
```

**Verify:** allocate a heap-owning value, let it go out of scope, run
under a memory-tracking allocator (or ASan-equivalent if this project has
one) and confirm no leak is reported. Absent that tooling, at minimum
assert the generated IR contains a `glyim_dealloc` call for such types and
none for plain-old-data structs.

---

## Phase 5 — Debug info correctness (independent, do any time after Phase 0)

### 5.1 — Local variables always get the *error* type's debug type

**File:** `glyim-codegen-llvm/src/debug.rs`, `declare_local()`.

**Anchor:**
```rust
let basic_ty = self.debug_type_for_ty(context, ty_ctx.error_ty(), ty_ctx);
```

**Fix:** `declare_local` must already receive (or be given) the local's
actual `Ty` — check its call site in `lower.rs`/wherever locals are
declared with debug info and thread the real `ty` parameter through
instead of hardcoding `ty_ctx.error_ty()`:
```rust
let basic_ty = self.debug_type_for_ty(context, ty, ty_ctx); // `ty` = the actual parameter, not ty_ctx.error_ty()
```
If `declare_local` doesn't currently take a `ty: Ty` parameter, add one
and update its one or two call sites.

**Verify:** compile with debug info, load the binary in a debugger (or
inspect the emitted DWARF/CodeView directly via `llvm-dwarfdump` if
available), confirm a local `let x: i32 = 5;` shows type `i32` in the
debugger, not whatever `error_ty()` maps to.

---

### 5.2 — Debug locations hardcoded to line 1, column 1

**File:** `glyim-codegen-llvm/src/debug.rs`, `declare_local()`.

**Anchor:**
```rust
let divar = self.builder.create_auto_variable(scope, name, file, 1, ...);
let loc = self.builder.create_debug_location(context, 1, 1, scope, None);
```

**Fix:** this function must be given the local's declaration `Span`
(check whether it already has access to one, e.g. via a parameter or
`self`) and convert it to line/column the same way
`location_for_span` (already used elsewhere in this file, per
`set_debug_location` in `lower.rs`) does. Reuse that exact conversion:
```rust
let (line, col) = self.line_col_for_span(&decl_span); // or whatever location_for_span's internal line/col extraction is called -- reuse it rather than reimplementing
let divar = self.builder.create_auto_variable(scope, name, file, line, ...);
let loc = self.builder.create_debug_location(context, line, col, scope, None);
```

**Verify:** debug info for two locals declared on different source lines
shows two different line numbers, not both "1".

---

### 5.3 — `clear_debug_location` is an empty stub

**File:** `glyim-codegen-llvm/src/lower.rs`.

**Anchor:**
```rust
#[allow(dead_code)]
fn clear_debug_location(&self) {}
```

**Fix:** first check whether it's called anywhere (`grep -rn
"clear_debug_location"`). If it's genuinely unused, either (a) delete it
entirely and remove the `#[allow(dead_code)]`, or (b) if compiler-generated
code (e.g. drop glue, synthesized landingpads) should have *no* debug
location attached (common practice: synthetic code gets a null/absent
location so debuggers don't attribute it to real source lines), implement
it for real and call it at those synthesis sites:
```rust
fn clear_debug_location(&self) {
    self.builder.unset_current_debug_location();
}
```
Then call it before emitting the landingpad in `emit_landingpad` and
before the synthetic `assert_fail`/`assert_panic_cont` blocks added in the
earlier `Assert` fix, since none of those correspond to a real source span.

**Verify:** debug info for compiler-synthesized blocks (landingpads,
assert-fail blocks) has no misleading source location attached.

---

### 5.4 — Enum debug type oversimplified to `{ i32, i8 }`

**File:** `glyim-codegen-llvm/src/debug.rs`, `debug_type_for_ty()`.

**Depends on:** Phase 3.1 (real enum tag type) should land first so this
uses the correct tag width instead of a hardcoded `i32`.

**Fix:** this is a bigger task — build a real DWARF union/variant-part
description mirroring the enum's actual layout (from
`FullLayoutComputer`, same source of truth used everywhere else in this
plan): for each variant, a `create_member_type` for its fields at their
real offsets, wrapped in `create_union_type` or the DIBuilder's variant
part API if `inkwell`/the LLVM DIBuilder bindings expose one (check
`inkwell::debug_info`'s `DIBuilder` methods available in this project's
locked inkwell version before assuming a specific API). If a full
variant-part encoding isn't feasible with the available bindings, at
minimum size the debug type to the enum's *actual* total size/alignment
(from layout) instead of a fixed `{ i32, i8 }`, and use the correct tag
width from 3.1. Document remaining limitations in a comment rather than
claiming full fidelity if you fall back to the simplified form.

**Verify:** `sizeof` as reported by the debugger for an enum type matches
`FullLayoutComputer`'s computed size, not a fixed 5 bytes.

---

## Phase 6 — Borrow checker precision (independent)

### 6.1 — Two-phase reservation status computed but discarded

**File:** `glyim-borrowck/src/lib.rs`, `check_stmt_conflicts()`.

**Anchor:**
```rust
let _in_reservation = if let Some(act) = activation {
    loan_is_in_reservation(loan, current_block, current_stmt_idx, act)
} else {
    false
};
```

**Fix:** the leading underscore means it's computed and thrown away.
Use it to actually suppress the write-conflict during the reservation
phase of a two-phase borrow (that's the entire point of two-phase
borrows: a shared "reserved" borrow doesn't conflict with a write until
it's *activated*):
```rust
let in_reservation = if let Some(act) = activation {
    loan_is_in_reservation(loan, current_block, current_stmt_idx, act)
} else {
    false
};
if in_reservation {
    // A two-phase borrow's reservation doesn't conflict with an ordinary
    // write yet -- only its activation point does. Skip this conflict.
    continue; // or whatever this function's real control-flow shape is at this point -- adjust to fit
}
// ... existing conflict-reporting code, now only reached when NOT in reservation
```

**Verify:** a two-phase-borrow pattern that today incorrectly fails to
borrow-check (e.g. `vec.push(vec.len())`-shaped code, if that's
expressible in this language) now passes; a genuine write-during-active-
borrow conflict still correctly fails.

---

### 6.2 — Liveness ignores `StorageDead`

**File:** `glyim-borrowck/src/liveness.rs`, `compute_stmt_liveness()`.

**Fix:** add explicit handling:
```rust
StatementKind::StorageDead(local) => {
    // The local's storage ends here -- it cannot be live *before* this
    // point via this path (kill it), even if it appeared live from a
    // later use, since that use is invalid once storage is gone.
    live.remove(local); // adjust to whatever this function's actual live-set representation/method names are
}
StatementKind::StorageLive(_) | StatementKind::Nop => {
    // No liveness effect.
}
```

**Verify:** a local that's read after its `StorageDead` (which should be
impossible in valid MIR, but borrowck should still treat liveness
correctly around the boundary) — construct a test with a local reused
across two disjoint scopes and confirm liveness for the first scope's
local doesn't incorrectly bleed into the second.

---

### 6.3 — `places_conflict` conservative catch-all for `ConstantIndex`/`Subslice`

**File:** `glyim-borrowck/src/visitor.rs`, `places_conflict()`.

**Note:** unlike the others in this phase, this one is *sound but
imprecise* (false positives, not false negatives) — lower priority. Fix
if time allows: two `ConstantIndex`/`Subslice` projections conflict iff
their statically-known offset ranges overlap; if either is `from_end` and
the base length isn't known at borrowck time, conservatively conflict (that
part of the catch-all is fine to keep) — only add precision for the
purely-compile-time-comparable cases:
```rust
(ProjectionElem::ConstantIndex { offset: o1, from_end: false, .. },
 ProjectionElem::ConstantIndex { offset: o2, from_end: false, .. }) => {
    if o1 != o2 { return false; /* provably disjoint, e.g. arr[0] vs arr[1] */ }
    // same offset -> keep checking the rest of the projection chain as before
}
```

**Verify:** borrowing `arr[0]` mutably and `arr[1]` mutably at the same
time (statically provably-disjoint indices) no longer incorrectly
conflicts; borrowing `arr[i]` and `arr[j]` with runtime `i`/`j` still
conservatively conflicts (correctly).

---

## Phase 7 — Const evaluation gaps (independent)

### 7.1 — Non-unit tuples rejected

**File:** `glyim-const-eval/src/eval.rs`.

**Fix:** add a `ConstValue::Tuple(Vec<ConstValue>)` variant (in
`glyim-const-eval/src/value.rs`), then evaluate each element:
```rust
Expr::Tuple(elements) => {
    let values: ConstEvalResult<Vec<ConstValue>> = elements
        .iter()
        .map(|&e| self.eval_expr(e, depth + 1)) // match this call to whatever the existing recursive-eval helper is actually named
        .collect();
    Ok(ConstValue::Tuple(values?))
}
```
Adding the new `ConstValue` variant means every existing `match
value { ... }` over `ConstValue` elsewhere (its own `validate_range`,
consumers in `glyim-lower`'s `const_block_to_u128`-equivalent, etc.) will
fail to compile until handled — that's expected; add a sensible arm
everywhere (e.g. `ConstValue::Tuple(_) => None` in places that only make
sense for scalar values, like a switch-value converter).

**Verify:** `const { (1, 2) }` evaluates successfully to
`ConstValue::Tuple([Int(1), Int(2)])`.

---

### 7.2 — Const pattern matching limited to `Wild`/`Literal`/`Or`

**File:** `glyim-const-eval/src/eval.rs`, `pattern_matches()`.

**Fix:** add `Struct`, `Tuple`, `Slice`, and `Range` arms, each
recursively evaluating sub-patterns against the corresponding
`ConstValue` (this depends on 7.1's `ConstValue::Tuple` for the `Tuple`
case, and a comparable `ConstValue::Struct`/`ConstValue::Slice` if not
already present — add them the same way). Follow the exact structure
`glyim-typeck`'s `check_pat.rs` uses for real (non-const) pattern
matching against the corresponding `Pat`/`PatternKind` variants — the
logic is structurally identical, just operating on `ConstValue` instead
of a runtime `Place`.

**Verify:** `const { (1, 2) }` matched against pattern `(a, b)` inside a
`match` binds `a = 1, b = 2` at compile time.

---

### 7.3 — `FallbackLayoutProvider` returns hardcoded 8-byte-everything layouts

**File:** `glyim-codegen/src/lib.rs`.

**Fix:** this fallback exists because `BytecodeBackend` can apparently be
constructed without a `TyCtx`. Determine whether that construction path
is actually reachable from the real compiler pipeline or only from tests.
If only from tests: gate it behind `#[cfg(test)]` and require a real
`TyCtx` everywhere else (change the public constructor to take `&TyCtx`
non-optionally). If it's reachable from real compilation: that's a
pipeline bug (codegen should never run without a fully-built `TyCtx`) —
fix the caller to always provide one, and make `FallbackLayoutProvider`
either `panic!("BytecodeBackend used without a TyCtx -- this is a pipeline bug")`
or simply not exist. Do not leave a silently-wrong 8-byte-everything
fallback reachable from real compilation.

**Verify:** `grep -rn "FallbackLayoutProvider" --include="*.rs" .` — every
remaining use should be behind `#[cfg(test)]`, or the type should be
deleted.

---

## Phase 8 — Parser / def-map / dead-code cleanup

### 8.1 — Raw pointer types have no syntax node wrapper

**File:** `glyim-frontend/src/parser/ty.rs`.

**Fix:** find the raw-pointer-parsing function (search for where `*const`
/ `*mut` tokens are consumed) and wrap the parsed `*`, `const`/`mut`, and
inner type in a proper node the same way every other type-parsing
function in this file does (look at how `parse_ref_type` or similar
brackets its output with `self.start_node(SyntaxKind::...Type)` /
`self.finish_node()` and mirror it exactly, adding a
`SyntaxKind::RawPtrType` — or whatever this file's naming convention for
type node kinds is — if it doesn't exist yet).

**Verify:** parse `*const i32` and `*mut i32`, confirm the resulting CST
contains a proper typed node (inspect via the crate's existing CST-dump
test utility, if any) rather than being invisible/flattened into its
parent.

---

### 8.2 — Def-map macro visibility not validated

**File:** `glyim-def-map/src/lib.rs`, `validate_import_visibility()`.

**Fix:** find the existing `types`/`values` namespace checks and add the
same check for the `macros` namespace — this should be almost entirely
copy-adjust-paste of the existing two blocks, just iterating the macro
namespace's import list instead.

**Verify:** an import that re-exports a private macro across a visibility
boundary that would already be rejected for a private type/value is now
also rejected for a private macro.

---

### 8.3 — Dead code: wire up or remove

For each of the following, first `grep -rn "<name>"` to confirm it's
truly unused; if a legitimate caller already exists elsewhere that the
report missed, just remove the `#[allow(dead_code)]`. Otherwise, either
wire it into a real caller or delete it — do not leave dead code with a
suppression attribute masking it.

- **`stub!`/`stub_impl!` macros** (`glyim-diag/src/lib.rs`): go back
  through every location fixed in Phases 1–7 that used to have a bare
  `tracing::warn!` + silent fallback (the exact anti-pattern these macros
  were built to replace) and confirm none of the *remaining*,
  not-yet-implemented edge cases in this codebase still use that pattern
  unconverted. Any genuinely-not-yet-implemented path that must remain a
  stub (e.g. something explicitly deferred to a future phase in this
  plan) should call `stub!("description")` instead of
  `tracing::warn!("STUB: ...")`, so it fails loudly during development
  instead of silently at runtime.
- **`LlvmBackend::lower_body_to_module` / `generate_ir`**
  (`glyim-codegen-llvm/src/lib.rs`): find the actual public entry point
  used by `glyim-pipeline` to drive LLVM codegen; if these two methods
  duplicate that entry point's functionality under a different name,
  delete them; if they represent a genuinely different, currently-unused
  capability (e.g. an IR-only output mode without full module lowering)
  that should be exposed, wire them into the CLI/pipeline as a real
  option instead of leaving them dead.
- **`dead_out`** (`glyim-borrowck/src/move_analysis.rs`) and **`live_in`**
  (`glyim-borrowck/src/liveness.rs`): these look like exactly the kind of
  computed-but-discarded analysis results that caused bug 6.1/6.2 above.
  Check whether some *other* part of borrowck should be consuming them
  (e.g. a use-after-move check reading `dead_out`, or a borrow-conflict
  check reading `live_in`) and is currently using a different, less
  precise substitute instead. If so, switch that consumer to use the real
  computed value. If truly unused by design, remove them.
- **`emit_arg_for_pass_mode`** (`glyim-codegen-llvm/src/abi_passmode.rs`):
  check whether `lower_call`'s inline argument-passing logic (in
  `lower.rs`) duplicates what this function does. If so, this is the
  exact kind of "correct logic written once, then re-implemented
  ad hoc elsewhere and left to rot" pattern flagged earlier in this
  project's audits (see the original slice-projection triplication
  finding) — consolidate: delete the duplicate inline logic in
  `lower_call` and call this function instead (useful side effect: fixing
  `PassMode::Pair` in task 3.2 only needs to happen in one place).

---

## Phase 9 — Architecture-level gaps (large, multi-file; do last)

These four are not mechanical patches — each is a genuine feature that
doesn't exist yet. Do not attempt to write final code for these blind;
each subsection is a scoped **design + step list**, not a diff.

### 9.A — Monomorphization pass

**Why it matters:** `TyKind::Param`, `TyKind::Projection`, `TyKind::Bound`,
and `TyKind::Infer` all `panic!` in `glyim-codegen-llvm/src/types.rs`
(item 8 in the report) precisely because nothing between typeck and
codegen ever substitutes concrete types for generic parameters. Every
other "fix the panic" instinct here is wrong — the panics are the correct
*symptom*; the missing pass is the *disease*.

**Steps:**
1. Add a new crate (or a module in `glyim-pipeline`, check which pattern
   this codebase already uses for pipeline-stage crates — e.g. compare to
   how `glyim-opt` is structured) called `glyim-mono` / `monomorphize`.
2. Walk the call graph starting from `main` (and any other roots — check
   how `glyim-pipeline` currently discovers entry points) collecting every
   concrete `(DefId, Substitution)` pair actually instantiated.
3. For each such pair, produce a specialized `Body` with every
   `TyKind::Param`/`TyKind::Bound` substituted for the concrete
   `Substitution`'s arguments (a straightforward recursive `Ty`
   substitution — check `glyim-type` for an existing substitution helper;
   `substitution_args` already used throughout the LLVM backend suggests
   one may partially exist).
4. Feed only the monomorphized bodies to codegen — never the generic
   originals.
5. `polymorphize.rs` (flagged elsewhere as "implemented but not
   integrated into the pipeline") may already do the *inverse*
   optimization (collapsing needlessly-distinct monomorphizations back
   together) — read it before building 9.A, since it likely expects to
   run as a pass *after* this one and may already assume/require its
   output shape.

**Verify:** a generic function called with two different concrete type
arguments produces two distinct, correctly-specialized function bodies in
the final binary, and the `TyKind::Param` panics in `types.rs` are never
hit by any passing test.

---

### 9.B — Trait resolution wired into codegen (dynamic dispatch)

**Depends on:** 9.A should land first (vtables need monomorphized method
bodies to point to).

**Steps:**
1. `glyim-codegen/src/vtable.rs` and `glyim-layout/src/vtable.rs` already
   compute vtable *layout* — read both fully before writing anything new.
2. Add vtable *construction*: for each concrete type known to implement a
   trait used as a trait object anywhere in the program (found via the
   same call-graph walk as 9.A, extended to also collect `dyn Trait`
   coercion sites), emit a global constant array of function pointers (one
   per vtable slot, per `glyim-codegen/src/vtable.rs`'s existing slot
   layout) pointing at that type's monomorphized method implementations.
3. Fix `thir::ExprKind::DynamicCall`'s lowering (currently the
   `diagnostics.push(...) + Unit` stub) in `glyim-lower` to instead lower
   to: load the vtable pointer from the trait object's second fat-pointer
   word, index into it at `method_index`, indirect-call the result with
   the data pointer as the receiver. This exact MIR shape (fat pointer →
   vtable load → indexed function pointer → indirect call) was already
   scoped in detail in this project's earlier audit — reuse that design
   rather than re-deriving it.
4. `glyim-mir-interp` and both codegen backends need indirect-call support
   for this to work end to end — check whether that's landed yet (an
   earlier audit flagged `glyim-mir-interp`'s indirect calls as
   unimplemented); if not, that's a prerequisite sub-task.

**Verify:** a `dyn Trait` method call actually invokes the correct
type-specific implementation at runtime for at least two different
concrete types behind the same trait object variable.

---

### 9.C — Closure capture analysis

**Steps:**
1. Find wherever closures are type-checked (`glyim-typeck`) and check
   whether captured-variable information is computed at all today, even
   if unused downstream — grep for "capture" across `glyim-typeck` and
   `glyim-hir` first.
2. If not computed: add capture analysis — for each closure expression,
   walk its body to find every reference to a variable defined outside
   the closure, recording (name, by-value-or-by-reference, source `Ty`).
3. Store the capture list on the closure's `AggregateKind::Closure`-
   producing THIR/MIR construction so `glyim-codegen-llvm`'s
   `build_layout_aggregate` (already generic over any ADT/closure field
   list, per the earlier `AggregateKind` fix) receives the *real* upvalue
   list as its `vals`/field types instead of whatever ad hoc field
   sequence it currently assembles.
4. Update the closure's call-site codegen to pass captured values as part
   of the closure's own struct rather than as ordinary call arguments.

**Verify:** a closure capturing a local by reference observes mutations
to that local made after the closure was created but before it's called;
a closure capturing by value does not.

---

### 9.D — Linker integration beyond bare `cc`

**File:** `glyim-cli/src/linker.rs`.

**Steps:**
1. Add a `LinkerOptions` struct (extra flags, custom linker binary
   override via e.g. a `-C linker=` style CLI flag, link-script path) fed
   from the CLI's existing argument-parsing (check how other CLI options
   are already threaded through — mirror that pattern).
2. Extend `invoke_linker` to append these to the `Command` builder instead
   of the current fixed two-argument invocation.
3. For cross-compilation: accept a target-triple-specific linker binary
   name/path (e.g. `<target>-gcc` or an explicit override) instead of
   always shelling out to plain `cc`.

**Verify:** a build with an extra `-l<somelib>` flag or a custom linker
script actually gets that flag passed through to the underlying `cc`/`ld`
invocation (inspect the actual command run, e.g. via a test that captures
`Command` arguments rather than actually invoking the system linker).

---

## Final checklist

Do not consider this plan complete until all of the following are true:

- [ ] `cargo check --workspace` is clean with zero new warnings versus
      `/tmp/baseline.log` from Phase 0 (ideally *fewer* warnings, since
      several tasks delete genuinely dead code).
- [ ] `grep -rn "tracing::warn!\|STUB\|not yet fully implemented\|not fully implemented" --include="*.rs" .`
      returns **only** hits you've deliberately decided must remain
      (e.g. 9.A–9.D's prerequisites, if you stopped before finishing
      Phase 9) — every one of those remaining hits should point at a
      `stub!()`/`stub_impl!()` macro call (task 8.3), never a bare
      `tracing::warn!` with silent fallback.
- [ ] Every task above has at least one new passing test exercising the
      specific behavior it fixed.
- [ ] No task introduced a new bare `_ => <old-buggy-behavior>` catch-all
      anywhere (rule 3) — grep the diff for `_ => {}` and `_ => ` near any
      file you touched and manually confirm each one is intentional, not
      a shortcut around an exhaustiveness error.
