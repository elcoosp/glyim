# Handoff — `glyim-v2` stdlib hello-world (updated)

## Status

**Object path (`--emit=obj`): fully working.** `glyim-cli --with-stdlib
--emit=obj` on `fn main() { println("hello"); }` produces a valid 2112-byte
Mach-O arm64 relocatable object. **All 4172/4172 workspace tests pass.**

**Executable path (`--emit=exec`): produces a runnable 2.2 MB Mach-O arm64
binary, but it segfaults at runtime before printing `hello`.** The link now
succeeds and the runtime hooks resolve (`_glyim_stdout_write` / `_glyim_errno`
are defined `T` symbols in the binary), so control reaches the runtime; the
segfault happens at or before the first `eprintln!` inside `glyim_stdout_write`,
which points at caller-side ABI (arguments `fd` / `buf` / `len`, or the
return-value convention) rather than the runtime body itself.

## Commits landed this session (7 commits ahead of origin/main)

    fdf692e6  feat(type,typeck): canonical io.g ADTs + pre-allocated impl-method ids
    4f6211ed  fix(lower): classify Int/Uint-to-Int/Uint as IntToInt (was PtrToPtr)
    91f1e23b  fix(codegen-llvm): emit extern C symbol names for extern-fn FnDefs
    e5a8d4f2  fix(codegen-llvm): inline builtin intrinsics (str/String as_bytes, as_ptr)
    4c6e1570  feat(runtime): add io FFI (glyim_stdout_write, glyim_errno, …)
    28a3f644  feat(cli): link glyim-runtime into --emit=exec binaries
    3dc1dbb5  docs: update handoff

### What each fixes

- **`fdf692e6`** — the original `--emit=obj` blocker. Two root causes:
  (1) io.g's `Error` / `ErrorKind` / `BufReader` / `BufWriter` weren't in
  `BuiltinAdt`, so they split between a def-map LocalDefId reached via the
  crate-root "walk every module" pre-pass (Adt39, a re-export) and the
  definition module's own id (Adt224/Adt227) → `Adt39 vs Adt227` mismatch at
  struct-literal level in `Error::last_os_error`.
  (2) `check_path`'s `Type::method` resolution looked up the callee's body in
  `body_owner_map`, which is populated **in source order** — so an early impl
  (`impl Error`) calling a method on a later impl (`impl ErrorKind`) missed
  the lookup and silently produced `Ty::ERROR` (no diagnostic, hence the ICE).
  Fixed by canonicalizing the four names via `BuiltinAdt` at reserved ids
  1070–1073, canonicalizing at the top of `resolve_name_to_adt_ty` (after
  alias expansion — order matters for `Result<T>`), and adding
  `pre_allocate_impl_method_ids`, which pre-populates `body_owner_map` for
  every impl method before any body check runs.

- **`4f6211ed`** — `lower_expr_to_rvalue`'s Cast arm classified only same-
  signedness integer casts as `IntToInt`; `Uint->Uint`, `Int->Uint`,
  `Uint->Int` fell into `_ => PtrToPtr`. LLVM then rejected
  `Uint(Usize) -> Uint(U64)` with `[X0000] PtrToPtr cast on non-pointer`,
  blocking `--emit=exec` for any program whose stdlib does `usize as u64`
  (io.g's `total += n as u64`). Fixed by classifying all four signedness
  combinations as `IntToInt` and falling back to a conservative `Rvalue::Use`
  (no-op) instead of `PtrToPtr` for anything unrecognized.

- **`91f1e23b`** — codegen used `__glyim_fn_{id}` for every `MirConstKind::Fn`
  reference, ignoring the `extern_fns` registry. Result was `___glyim_fn_298`
  / `___glyim_fn_301` in the object even for
  `extern "C" { fn glyim_stdout_write / glyim_errno }`. Fixed by consulting
  `TyCtx::extern_fn_name(def_id)` in both `lower_call` and the `MirConstKind::Fn`
  value lowering.

- **`e5a8d4f2`** — builtin methods (`str::as_bytes` → FnDefId 9104, `str::as_ptr`
  → 9102) have synthetic ids in the 9_000+ range with no MIR body, but
  codegen lowered calls to them as ordinary `__glyim_fn_{id}` calls. The
  resulting `___glyim_fn_9102` / `___glyim_fn_9104` undefined symbols blocked
  linking. Fixed by a new `try_lower_builtin_intrinsic` dispatcher at the top
  of `lower_call`: for `as_bytes` / `as_ptr` / `as_mut_ptr` on a fat-pointer
  or thin-pointer receiver, emit an LLVM-level pass-through / field-0 extract
  / bitcast, so no `__glyim_fn_*` symbol is referenced.

- **`4c6e1570`** — nothing in the workspace defined `glyim_stdout_write`,
  `glyim_stderr_write`, `glyim_stdin_read`, `glyim_stdout_flush`, or
  `glyim_errno`, even though the assembled stdlib declares them
  `extern "C"`. Added `crates/glyim-runtime/src/io.rs` with thin wrappers over
  `File::from_raw_fd` + `read`/`write`, returning the byte count or `-errno`.
  `glyim_errno` returns `std::io::Error::last_os_error().raw_os_error()` so
  `ErrorKind::from_raw_os_error` maps `WouldBlock` correctly.

- **`28a3f644`** — the CLI's linker step never linked the runtime crate.
  Declared `crate-type = ["staticlib", "rlib"]` on `glyim-runtime`, added
  `linker::find_runtime_staticlib` (searches `GLYIM_RUNTIME_LIB`,
  `target/<profile>/`, and ancestors of the current executable), and routed
  both link sites (fresh-compile + cache-hit) through `link_with_args` with
  the runtime archive as an extra object.

## Progress since the last handoff: Slice fat pointers landed

Commit (this session) fixed the **`&[T]`** half of the fat-pointer gap:

- `glyim-layout::layout_of` — `Ref(_, Slice(_), _)` / `RawPtr(Slice(_), _)`
  now returns a 16-byte `{ptr, i64}` layout (was 8-byte scalar).
- `glyim-codegen-llvm::types::llvm_type_for_ty` — same arm produces
  `struct { ptr, i64 }` (was bare `ptr`).
- `glyim-codegen-llvm::abi::classify_arg` — same arm returns
  `PassMode::Direct` (correct for two-register passing on AArch64 / SysV).
- `glyim-codegen-llvm::lower::place_ptr`'s `ProjectionElem::Deref` — when
  the loaded value is a struct (fat pointer), extract field 0 (the data
  pointer) rather than calling `into_pointer_value()` (which panicked).

Full test suite stays green (4172/4172). The `--emit=exec` ICE is gone; the
binary now runs and exits 0, but still prints nothing (see below).

## Precise remaining blocker: `&buf[written..]` lowers to an element pointer

The `--emit=exec` binary now:
- links cleanly against the runtime staticlib;
- reaches `glyim_stdout_write(fd=1, buf=…)` with the *correct first call*;
- and codegen's `try_lower_builtin_intrinsic` handles `str::len` /
  `str::as_bytes` / `str::as_ptr` correctly (verified via
  `GLYIM_DBG_INTR`).

But the `len` argument to the second call is still garbage. The cause is in
`crates/glyim-lower/src/lower_rvalue.rs`:

**`&buf[written..]` (open-ended range slice) is lowered to an element
`Index` projection, not a sub-slice.**

`lower_expr_to_place`'s `Index { base, index }` arm (around line 1069) always
allocates a local for `index.ty` and emits
`ProjectionElem::Index(index_local)`. When `index` is a `Range` — the
`buf[written..]` form — `index.ty` is `Range<usize>` (builtin ADT 1000), so
the resulting place is `base[Deref, Index(Range<usize>)]`. Codegen interprets
that as **single-element** indexing (a `&u8`), not a sub-slice, so the
`&[u8]` the caller expects is actually a pointer to one byte.

The MIR for `Stdout::write_all` shows this directly (from
`--with-stdlib --emit=mir`):

    Assign($9, Aggregate(Adt(1000), [Copy($4), Constant(MirConst {
        kind: Error, ty: Ty(8), span: 63111..63120 })]))
    Assign($10, Ref(Place { local: $2, projection: [Deref, Index($9)] }, Shared))
    Call { Fn(FnDefId(477)), args: [$1, $10], ... }

`$10` is the `&buf[written..]` argument to `self.write(...)` — a `Ref` of an
element projection whose index is a `Range`. That should be a fat-pointer
`&[u8]` (field 1 = `len - written`).

### The correct fix (needs a careful session)

`Place` cannot represent a sub-slice (its type would be the unsized `[T]`),
so the `Ref { Index { Range } }` pattern must be lowered directly to a
fat-pointer `Rvalue` — bypassing `Place` entirely. Concretely:

1. In `lower_expr_to_rvalue`'s `Ref { operand, .. }` arm, detect
   `operand.kind == Index { index: Range, .. }`, call
   `lower_dynamic_range_slice` to produce the `{data_ptr, new_len}` pair,
   and return that as the rvalue directly (the caller then stores it into a
   fat-pointer local).

2. `lower_dynamic_range_slice`'s return value must be the `{ptr, len}`
   tuple — check whether it currently returns that or a bare pointer; the
   `--emit=exec` trace shows the *first* call (from `println`'s internal
   `write_all` on a whole string literal, where no range-slice is involved)
   is correct, and the second (from `write_all`'s `self.write(&buf[written..])`)
   is not, so the range-slice path is where the corruption lives.

3. The already-landed fat-pointer work (`0ee6e4b3`, `f2a830e0`) makes the
   `{ptr, i64}` local representable; the missing piece is only the lowering
   of `&base[a..b]` to that shape.

### Verified facts (do not re-derive)

- `lower_expr_to_rvalue`'s `Index { base, index: Range }` arm (around line
  671) *already* calls `lower_dynamic_range_slice` and produces the correct
  fat-pointer rvalue. The bug is only in the *sibling* `Ref`-wrapped path
  through `lower_expr_to_place`.
- `str::as_bytes` / `str::as_ptr` / `str::len` intrinsics are correct
  (verified via `GLYIM_DBG_INTR`).
- The three `v15_t*` drop tests pass with `&str` as a fat pointer now.

## Older blocker notes (kept for reference)

## Root cause found: `llvm_type_for_ty(&[T])` ≠ `llvm_type_for_ty(Tuple[ptr, usize])`

After landing the `place_ptr` `Field`-Slice fix (`3c66984c`), I re-tried
routing `&buf[written..]` through `lower_dynamic_range_slice` (the `Ref`
arm of `lower_expr_to_rvalue`). The **MIR it produces is correct** — verified
by diffing `--emit=mir` before/after:

    Assign($9,  Len(Place{local:$1, projection:[Deref]}))         ; len
    Assign($10, Add($14, $15))                                     ; data_ptr + start*1
    Assign($11, Sub($9, $7))                                       ; len - start
    Assign($16, Aggregate(Tuple, [$10, $11]))                      ; {ptr, i64}
    Assign($17, Aggregate(Tuple, [$6, $16]))                       ; (fd, slice)
    Assign($4,  Aggregate(Adt(1010, Err), [$17]))                  ; Err((fd, slice))

But codegen **ICEs** at `lower.rs:185` (`llvm_type_for_ty` → `TyKind::Error`),
because the tuple value `{ptr, i64}` is assigned into `$16`, whose *declared
Ty* is `&[T]`. Two different LLVM types are in play:

- `llvm_type_for_ty(&[T])` → the fat-pointer arm added this session:
  `context.struct_type(&[ptr, i64], false)` — a **literal** struct type.
- `llvm_type_for_ty(Tuple[ptr, usize])` → falls into the `TyKind::Tuple`
  arm, which goes through `SimpleLayoutComputer::layout_of` and
  `opaque_sized_type(size, align)` — a **different** LLVM type (an
  `[N x i8]` struct / opaque block) with the same 16-byte size.

Storing an aggregate of one into a local declared as the other is an LLVM
type mismatch; codegen does not insert a `bitcast` for it, and eventually
`llvm_type_for_ty` is called on a `Ty::ERROR` that falls out of the failed
resolution.

### The correct fix (needs a fresh, focused session)

Make the two representations produce the **identical** LLVM type. Two
candidate approaches:

**(A) Prefer the literal-struct arm for tuple-shaped fat pointers.**
In `llvm_type_for_ty`, before the general `TyKind::Tuple` arm, detect a
2-element tuple whose elements are `(Ref(_, X, _), Uint(Usize))` and return
`context.struct_type(&[ptr, i64], false)` — the same value the `Ref(Slice)`
/ `Ref(String)` arm returns. Requires `mk_ref`-interned tuples to be
recognisable (they are: `lower_dynamic_range_slice`'s `mk_fat_ptr` builds
exactly `Tuple[Ref(elem, Not), Usize]`).

**(B) Return the reference-typed value from `lower_dynamic_range_slice`.**
Change the helper's `Rvalue::Aggregate(Tuple, …)` to produce a value whose
`Ty` is `&[T]` rather than a tuple. Since the aggregate's *value* shape is
already `{ptr, i64}`, and `llvm_type_for_ty(&[T])` is `{ptr, i64}`,
this makes the assignment type-consistent. This is the smaller change but
touches the helper's signature (`result_ty` is currently `&[T]` already —
it is only the produced `AggregateKind` that is wrong).

**(B) is the recommended first attempt**: change
`Rvalue::Aggregate(glyim_mir::AggregateKind::Tuple, slice_operands)` at the
end of `lower_dynamic_range_slice` (around line 2010 of
`lower_rvalue.rs`) to a single-field aggregate that keeps the `&[T]` type,
or have the caller's `Assign` wrap it. Then re-apply the `Ref`-arm
delegation (`return self.lower_expr_to_rvalue(operand);` for
`Ref { Index { Range } }`) — that patch was already verified to produce
correct MIR and only failed at codegen because of the LLVM-type mismatch.

### Verified-green state at time of writing

- `HEAD` = `974c4a73`; working tree clean.
- `--emit=obj` on hello world → valid 5008-byte Mach-O arm64 object.
- Full test suite: 4172/4172 pass.
- 5 commits ahead of `origin/main` (see the log section below).

The `Ref`-arm patch was tried, produced correct MIR, failed at codegen for
the LLVM-type-mismatch reason above, and was reverted to preserve the green
state.

## Older analysis (kept for reference)



**What works:**
- `--emit=obj` produces a valid object (primary objective).
- `--emit=exec` links and runs to exit 0.
- The MIR for `Stdout::write` (`fn 477`) is **correct**:
  `Call(glyim_stdout_write, [$4=self._fd (i32), $6=buf.as_ptr() (*const u8), $8=buf.len() (usize)])`.
- `str::len` / `str::as_ptr` / `str::as_bytes` intrinsics fire correctly at
  codegen (verified via `GLYIM_DBG_INTR`; each extracts the right fat-pointer
  field).
- `place_ptr`'s `Field` arm now resolves `Slice`/`String` fields (needed by
  `lower_dynamic_range_slice`). `commit 3c66984c`.

**What's broken:** the runtime receives garbage for the `buf` **and** `len`
arguments of `glyim_stdout_write`. A trace inside the runtime shows:

    [IO] stdout_write fd=1 buf=0xa0070da06dc68 len=6631507592
    [IO] stdout_write fd=1 buf=0xa len=0

`fd=1` is right; both `buf` and `len` are wrong. `buf` looks like a stack
address (not a data pointer), and `len` looks like a pointer value cast to
`usize` — i.e. the `(data_ptr, i64 len)` fat-pointer *pair* is being passed
with the wrong register/field mapping somewhere between the caller and the
callee.

`fn 477`'s own argument setup (from `otool -tvV`) *looks* correct
(`w0 = fd`, `x1 = buf`, `x2 = len`, then `bl glyim_stdout_write`), so the
corruption is likely **upstream**: the caller's parameters `$1: &mut Stdout`
and `$2: &[u8]` are stored into locals by the prologue, and if the prologue's
byval/`Indirect` detection mis-classifies the `&[u8]` parameter, the fat
pointer lands in the wrong slot — after which every field read is offset by
8 bytes.

### Next diagnostic step

Add a codegen trace at the top of `lower_call` for `FnDefId(298)` that prints
each argument's `Operand`, `Operand::ty`, and lowered LLVM value. Then look at
the prologue in `lower_body` (`crates/glyim-codegen-llvm/src/lower.rs`, the
`for i in 1..=body.arg_count` loop) — specifically whether the
`fn_abi.args[i].mode` for the `&[u8]` parameter is `Indirect` (in which case
the prologue dereferences a byval pointer that is actually a two-register fat
pointer, corrupting every subsequent read) or `Direct` (correct).

### Commits landed this session (8)

    3c66984c  fix(codegen-llvm): resolve Slice/String Field projections in place_ptr
    fc9235e7  fix(typeck,codegen): prefer builtin len/is_empty for slice/str receivers
    f2a830e0  fix(layout,codegen): &str is also a 16-byte fat pointer
    0ee6e4b3  fix(layout,codegen): &[T] is a 16-byte fat pointer, not an 8-byte scalar
    ...

## Older analysis (kept for reference)



**`&str` is still a thin pointer.** The `as_bytes` intrinsic needs its
*receiver* (`&str` / `String`) to carry a length; because `Ref(_, String,
_)` still lowers to a bare `ptr`, codegen cannot synthesize the fat-pointer
`&[u8]` destination and reports:

    [X0000] unsupported intrinsic `as_bytes` receiver/destination shape

The three `v15_t*` drop tests in `glyim-codegen-llvm::drop_dealloc`
(`v15_t05_drop_mut_ref_non_copy_emits_dealloc`,
`v15_t10_drop_raw_ptr_non_copy_type`,
`v15_t24_drop_mut_ref_with_cleanup_dealloc`) construct `Ref(String, Mut)` /
`RawPtr(String, Mut)` and assert drop-glue behaviour assuming a thin
pointer. They must be updated in lockstep with the codegen change, in this
order:

1. Extend the three codegen sites (`layout.rs`, `types.rs`, `abi.rs`) to
   also cover `Ref(_, String, _)` / `RawPtr(String, _)`.
2. Extend `place_ptr`'s `Deref` arm — it already handles the struct case
   (field 0 extract), so this should just work once `types.rs` returns a
   struct for `&str`.
3. Update the three drop tests: their `TerminatorKind::Drop` now drops a
   16-byte local; the IR assertion (`call void @glyim_drop_in_place` /
   `@glyim_dealloc`) stays, but the pre-drop `load` changes shape.
4. Extend `try_lower_builtin_intrinsic`'s `as_bytes` arm to handle a
   `&str` receiver by taking `(data_ptr, len)` from the fat pointer and
   storing them as the `{ptr, i64}` destination. For `String::as_bytes`
   (receiver `&String`, an ADT), the length comes from the inner
   `Vec`'s len field — a separate small lowering.

## Older diagnosis (kept for reference)

## The remaining blocker for `--emit=exec`

`./h` (the produced binary) segfaults. Evidence:
- `nm h` shows `_glyim_stdout_write`, `_glyim_errno`, `_glyim_stdout_flush`
  defined as `T` (text) — linking is complete.
- Running under a debug trace added to `glyim_stdout_write` crashes before
  the first `eprintln!` inside it — so the crash is not in the runtime's Rust
  body; it is in the caller-side argument setup, or the callee returns to a
  corrupt address.
- No output is produced (`println("hello")` never emits its bytes).

### Precise diagnosis (this session, code-referenced)

**Root cause: `llvm_type_for_ty` collapses `&[u8]` / `&str` to a bare
pointer.** `crates/glyim-codegen-llvm/src/types.rs:39`:

    TyKind::Ref(..) | TyKind::RawPtr(..) => {
        context.ptr_type(inkwell::AddressSpace::default()).into()
    }

That arm matches **every** reference, including `&[u8]` and `&str`, which are
*unsized* pointees and must lower to a **fat pointer** `{ data_ptr, i64 len }`
(the same shape `TyKind::Slice` / `TyKind::String` already produce at
`types.rs:72`). Because the local slot for `$2: &[u8]` is allocated as a bare
8-byte pointer while the *value* stored into it is a 16-byte fat pointer, the
prologue's `build_store(local_ptr, param_val)` writes 16 bytes into an 8-byte
slot (or, in the byval path, `build_load(local_ptr, llvm_ty)` reads only 8),
and every subsequent read of `buf` gets garbage.

The MIR itself is correct — `fn crate[0]::477` (`Stdout::write_all`)
emits:

    Call { func: Fn(FnDefId(298)),                 // = glyim_stdout_write
           args: [Move($4),   // self._fd  : i32
                  Move($6),   // buf.as_ptr() : *const u8
                  Move($8)] } // buf.len()    : usize

so the typeck side of `as_ptr()` / `len()` already produces the right
three-argument shape. The corruption is purely in how codegen materializes
the `&[u8]` receiver into its local slot and then reads the pointer out of it.

**Confirmed symptom:** running the `--emit=exec` binary under lldb prints the
process environment block (`PATH=…`, `HOME=…`, `executable_path=…`) instead
of `hello`. That is exactly what a call to `glyim_stdout_write(1, environ,
huge_len)` looks like, i.e. `buf` was read from the wrong slot (it landed on
the env block on the stack) and `len` was a garbage size.

**A one-line fix to `types.rs` is NOT sufficient.** Changing the `Ref` arm to
produce a fat pointer for slice/str pointees ICEs immediately, because the
rest of the LLVM backend still assumes `Ref` = bare pointer at ~11 other
sites in `crates/glyim-codegen-llvm/src/lower.rs`:

   462, 2199, 2703, 2763, 2900, 2971   // `TyKind::Ref(_, inner, _) => *inner`
   3701                                // `Ref(_, inner, Mut) => ...`

Each of those dereferences/loads/field-projections must be taught about the
fat-pointer representation in lockstep. The full set of affected places:

1. `types.rs` — `TyKind::Ref(_, inner, _)` when `inner` is `Slice`/`String`
   → lower to `{ ptr, i64 }` (the change attempted and reverted this session).
2. `lower.rs:place_ptr` / `ProjectionElem::Deref` — when dereferencing a
   `&[u8]` / `&str` local, produce a pointer to the **data field** (GEP 0,
   0), not to the fat-pointer struct.
3. `lower.rs:lower_operand` for `Place` whose type is `&[u8]` — load the
   full `{ptr, i64}` struct, not a bare pointer.
4. `lower.rs:lower_call` — when an argument's type is a fat-pointer `Ref`,
   split it into `(data_ptr, len)` for ABI purposes (the extern fn
   `glyim_stdout_write` takes them as two separate scalar parameters, which
   is why the MIR already has three args).
5. `lower.rs` prologue (`~line 3976`) — the byval/Indirect detection and
   the store of `param_val` into the local slot must use the fat-pointer
   type for unsized `Ref` params.
6. `abi.rs` (`FullLayoutComputer::fn_abi_of`) — must classify unsized-`Ref`
   parameters as `PassMode::Pair` (or equivalent) so the ABI is 16 bytes,
   not 8.

Because (6) is what tells the calling convention how many registers to use,
getting (1)-(5) right without (6) still produces a mismatched ABI — so the
correct fix is to start from `abi.rs`, decide the canonical fat-pointer ABI
for unsized references, then propagate that decision outward through the
five codegen sites above.

### First concrete step for the next session

Add a diagnostic to `abi.rs`'s `fn_abi_of` (or the codegen-local
`llvm_fn_type_from_sig`) that prints, for each argument, its `Ty` and the
resulting `PassMode`. Compile `fn main() { println("hello"); }` and check
what mode the `&[u8]` argument to `Stdout::write_all` gets today — if it is
`PassMode::Direct { ty: &[u8] }` with a bare-pointer LLVM type, that is the
single fact the fix hinges on.

### Diagnostic commands to start with

    # Produce the binary and inspect it with lldb
    TMPD=$(mktemp -d)
    cat > "$TMPD/h.g" << 'GEOF'
    fn main() { println("hello"); }
    GEOF
    cargo run -q -p glyim-cli -- --with-stdlib --emit=exec -o "$TMPD/h" "$TMPD/h.g"
    lldb --batch -o run -o bt -o 'register read' "$TMPD/h"

    # Inspect the object's __glyim_fn_477 (Stdout::write_all)
    otool -tvV "$TMPD/h" | grep -A 40 __glyim_fn_477

## Follow-ups (priority order)

1. **`--emit=exec` segfault** — see above. The last mile for running hello
   world.
2. **`--emit=llvm-ir` ICEs** with `fn_abi_of failed: UnknownType(Ty(36317))`.
   Blocks the IR-level debugging above. Same class of `Param`/`Infer`
   leakage that the object path already fixed; inspect `lower_body`'s
   `fn_sig` construction (`crates/glyim-codegen-llvm/src/lower.rs`).
3. **THIR `Err` nodes reaching MIR.** `[X0000] Err expression in THIR during
   lowering @N..M` fires ~10 times per hello-world compile. Each is a silent
   `thir::Expr::err(span)` that was never paired with a diagnostic — the very
   pattern that hid the original `--emit=obj` blocker. Audit every
   `thir::Expr::err` call site and pair it with `diagnostics.push(…)`.
4. **Runtime staticlib is ~26 MB (debug)** — strip / release-profile handling
   is a polish item.
5. **`def_map.max_local_def_id` starts near 2^30** — confirm that is
   intentional (`crates/glyim-def-map/src/lib.rs:576`).

## Working style reminder

Hard constraints from the original handoff remain in force:
- **Never** default `Ty::ERROR` to `i8` at codegen (`v15_t25_drop_error_type`
  is a `#[should_panic]` contract).
- Prefer fixing type resolution / registration once, everywhere.
- Every `thir::Expr::err(span)` call should be paired with a
  `diagnostics.push(...)`.
- Keep commits conventional-prefixed and document remaining blockers.
