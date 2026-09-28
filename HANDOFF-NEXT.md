# Handoff — `glyim-v2` stdlib hello-world (final state)

## Status

**Object path (`--emit=obj`): fully working.** `glyim-cli --with-stdlib
--emit=obj` on `fn main() { println("hello"); }` produces a valid 5008-byte
Mach-O arm64 relocatable object. **All 4172/4172 workspace tests pass.**
Working tree clean.

**Executable path (`--emit=exec`): produces a runnable 2.2 MB binary, exits
0, but prints nothing.** The link succeeds, the runtime is called
(`glyim_stdout_write(fd=1, …)`), but the `buf` and `len` arguments are
garbage — traced to a single unrepresentable construct in the MIR.

## Commits landed this session (6 ahead of `origin/main`)

    e995ea53  docs(handoff): root cause is llvm_type_for_ty disagreement on fat pointers
    974c4a73  docs(handoff): narrow remaining exec bug to fat-pointer ABI in prologue
    3c66984c  fix(codegen-llvm): resolve Slice/String Field projections in place_ptr
    9c7e2360  docs(handoff): pin remaining exec bug to Ref(Index(Range)) lowering
    fc9235e7  fix(typeck,codegen): prefer builtin len/is_empty for slice/str receivers
    f2a830e0  fix(layout,codegen): &str is also a 16-byte fat pointer
    0ee6e4b3  fix(layout,codegen): &[T] is a 16-byte fat pointer, not an 8-byte scalar

## The single root cause — definitively established

**`lower_dynamic_range_slice` cannot express fat-pointer field access,
because MIR `Place::ty` returns `Ty::ERROR` for `Field` on a slice.**

Trace of the failing path (`--emit=exec`):

1. `io.g`'s `impl Write for Stdout::write_all` contains
   `self.write(&buf[written..])`.
2. `&buf[written..]` reaches `lower_rvalue`'s `Ref` arm, which today does
   `Ref(lower_expr_to_place(Index { Range }))`.
3. `lower_expr_to_place`'s `Index` arm builds
   `Place{projection:[Deref, Index(range_local)]}` — an **element** index,
   not a sub-slice.
4. Codegen therefore produces a `&u8` (single-byte pointer) where a
   fat-pointer `&[u8]` was expected. `self.write` then reads `as_ptr()` /
   `len()` from a byte pointer and `glyim_stdout_write` receives garbage.

### Why the obvious fix does not work in isolation

Routing `&base[a..b]` directly through `lower_dynamic_range_slice` (which is
what the unwrapped `Index { Range }` path does) produces **correct MIR** —
verified by diffing `--emit=mir`:

    Assign($9,  Len(Place{local:$1, projection:[Deref]}))    ; len
    Assign($10, Add($14, $15))                                ; data_ptr + start*elem_size
    Assign($11, Sub($9, $7))                                  ; len - start
    Assign($16, Aggregate(Tuple, [$10, $11]))                 ; {ptr, i64}
    Assign($17, Aggregate(Tuple, [$6, $16]))                  ; (fd, slice)

But it then ICEs in codegen with `Attempted to lower TyKind::Error to LLVM`,
because `lower_dynamic_range_slice`'s **own body** derefs the `&[T]`
(`Place{local: $1, projection: [Deref]}` yields an unsized `[T]`) and then
projects `Field(0)` / `Field(1)` onto it. MIR `Place::ty` has no arm for
`Field` on a slice — it returns `Ty::ERROR` (I only added the codegen-side
`place_ptr` arm, `3c66984c`, which is a different function).

Concretely, `crates/glyim-mir/src/lib.rs`'s `Place::ty` and `Place::ty_mut`
match `ProjectionElem::Field(idx)` on the *current* type; the arms are
`Tuple`, `Adt`, and (my earlier addition) `Slice` / `String`. There is no
arm for `Ref(Slice)` / `Ref(String)` — reading a fat-pointer field *through
its reference*, which is what `lower_dynamic_range_slice` needs (the fat
pointer value lives inside the reference local; you must read `Field(0)`
from the reference, not from a dereferenced slice).

## The correct fix (coordinated, ~4 files)

This is not a one-file change; each layer must agree. The plan:

**(1) `crates/glyim-mir/src/lib.rs` — `Place::ty` and `Place::ty_mut`**

Add a `Field` arm for a reference to an unsized pointee:

    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _)
        if matches!(ctx.ty_kind(*inner), TyKind::Slice(_) | TyKind::String) =>
    {
        // Field(0) = data pointer, Field(1) = length.
        // The read-only TypeLookup cannot intern a fresh `&T`; return
        // `Ty::USIZE` for both slots (same 8-byte layout) and let codegen
        // bitcast, OR add a `TypeLookup::make_ref_ty(&self, elem) -> Ty`
        // hook implemented by `TyCtx`/`TyCtxMut` so the field type is exact.
        Ty::USIZE
    }

The `make_ref_ty` hook is the cleaner option — `TyCtx` already has
`mk_ref(&self, …)` (interns into the shared arena) and `TyCtxMut` has
`mk_ref(&mut self, …)`. Add a defaulted `fn make_ref_ty(&self, _elem: Ty)
-> Ty { self.error_ty() }` to `TypeLookup`, override in both impls.

**(2) `crates/glyim-codegen-llvm/src/lower.rs` — `place_ptr`**

Extend the `Field` arm's `current_ty` match to also handle
`Ref(_, Slice(_) | String, _)` (mirror the existing `Slice` / `String`
arms added in `3c66984c`), so `build_in_bounds_gep` uses the fat-pointer
struct layout (fields at offsets 0 and 8).

**(3) `crates/glyim-lower/src/lower_rvalue.rs` — `lower_dynamic_range_slice`**

Stop auto-derefing a `&[T]` before reading its fields. Read
`Field(0)` / `Field(1)` from the **reference** place (the local that holds
the fat pointer). For an inline `[T; N]` array keep the existing
`ConstantIndex(0)` path. For a `&[T; N]` deref once and then use
`ConstantIndex(0)`.

**(4) `crates/glyim-lower/src/lower_rvalue.rs` — `Ref` arm**

Route `Ref { Index { Range } }` through `lower_expr_to_rvalue` (the patch
already written and verified to produce correct MIR):
`return self.lower_expr_to_rvalue(operand);`.

**(5) Verify** `--emit=obj` still produces an object and `--emit=exec`'s
runtime receives a valid `(buf, len)` pair (add a temporary
`eprintln!("[IO] buf={:p} len={}", buf, len)` in
`crates/glyim-runtime/src/io.rs::glyim_stdout_write`).

## Verified pieces (do not re-derive)

- `--emit=mir` diff shows the `Ref`-arm delegation produces correct MIR.
- `llvm_type_for_ty(Ref(Slice))` and `llvm_type_for_ty(Ref(String))` are
  `struct {ptr, i64}` (from `0ee6e4b3` / `f2a830e0`).
- `glyim-layout`'s `layout_of` gives 16 bytes for `Ref(Slice)` / `Ref(String)`.
- `abi::classify_arg` returns `PassMode::Direct` for both (correct for
  two-register passing on AArch64 / SysV).
- `str::as_bytes` / `str::as_ptr` / `str::len` intrinsics in codegen's
  `try_lower_builtin_intrinsic` are correct (verified with `GLYIM_DBG_INTR`).
- MIR `Place::ty` / `ty_mut` handle `Field` on `Slice` / `String` (via my
  codegen-side fix `3c66984c` for `place_ptr`, but see gap (1) above for
  the MIR side).
- `__glyim_fn_477` (`Stdout::write`) prologue + call to `glyim_stdout_write`
  are correct in isolation (disassembled — the argument registers are set
  right; the corruption is upstream in `write_all`).

## Environment

- Repo root: `/Users/adm/Documents/Repos/glyim-v2`
- Host: `aarch64-apple-darwin`, rustc 1.98.1
- HEAD: `e995ea53`
- Full suite: 4172/4172 pass, 2 skipped

## Diagnostic env vars used this session (all reverted)

`GLYIM_DBG_INTR` (intrinsic lowering), `GLYIM_DBG_IO` (runtime io),
`GLYIM_DBG_ABI` (fn_abi classification), `GLYIM_DBG_REF` (Ref arm).
None are committed.

## Working-style constraints (still in force)

- **Never** lower `Ty::ERROR` at codegen — `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Prefer fixing type resolution / registration once, everywhere.
- Every `thir::Expr::err(span)` call should be paired with
  `diagnostics.push(...)`.
- Conventional-commit prefixes; document remaining blockers.
