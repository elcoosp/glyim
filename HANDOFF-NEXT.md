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
