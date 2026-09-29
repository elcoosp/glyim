# Handoff — `glyim-v2` stdlib hello-world: **COMPLETE**

## Status: end-to-end goal achieved

**`glyim-cli --with-stdlib --emit=obj`** on `fn main() { println("hello"); }`
produces a valid 5152-byte Mach-O arm64 object.

**`glyim-cli --with-stdlib --emit=exec`** on the same program produces a
2.2 MB Mach-O arm64 binary that **prints `hello` and exits 0**.

**All 4172/4172 workspace tests pass.** Working tree clean.

## Commits landed this session (10 ahead of `origin/main`)

    9199c210  feat: hello world runs end-to-end — `println` prints via --emit=exec
    9f9887c6  fix(mir): Place::ty/ty_mut resolve Field on Slice/String (fat-pointer slots)
    a99e3803  docs(handoff): definitive root cause — MIR Place::ty lacks Ref(Slice) Field arm
    e995ea53  docs(handoff): root cause is llvm_type_for_ty disagreement on fat pointers
    974c4a73  docs(handoff): narrow remaining exec bug to fat-pointer ABI in prologue
    3c66984c  fix(codegen-llvm): resolve Slice/String Field projections in place_ptr
    9c7e2360  docs(handoff): pin remaining exec bug to Ref(Index(Range)) lowering
    fc9235e7  fix(typeck,codegen): prefer builtin len/is_empty for slice/str receivers
    f2a830e0  fix(layout,codegen): &str is also a 16-byte fat pointer
    0ee6e4b3  fix(layout,codegen): &[T] is a 16-byte fat pointer, not an 8-byte scalar

## The three root causes (all fixed)

The work was dominated by one theme: **`&[T]` and `&str` are fat pointers
`{data_ptr, i64 len}`, but the compiler treated them as 8-byte scalars in
several layers.** A single coherent story:

### 1. Fat-pointer representation (commits `0ee6e4b3`, `f2a830e0`)

Three layers lowered every `Ref`/`RawPtr` to a bare 8-byte pointer:
- `glyim-layout::layout_of` → `Layout::scalar(ptr_size)`.
- `glyim-codegen-llvm::types::llvm_type_for_ty` → `context.ptr_type()`.
- `glyim-codegen-llvm::abi::classify_arg` → `PassMode::Direct` scalar.

For `&[T]` / `&str` this was wrong: the local slot was 8 bytes while the
stored value was a 16-byte fat pointer, so every read of a slice's length
returned garbage. Added dedicated arms in all three that produce the
`{ptr, i64}` shape (16-byte layout, LLVM struct, `PassMode::Direct` for
two-register passing). Also fixed three drop-glue sites in `lower.rs` that
called `.into_pointer_value()` on the struct value.

### 2. Intrinsic `len` (commit `fc9235e7`)

`slice.g` / `str.g` declare `fn len` as an empty `{ /* compiler intrinsic */ }`
stub. Typeck's impl-scan found that stub before the builtin table and
resolved `self.len()` to it (returning uninitialized memory). Fixed by
consulting `try_builtin_method` first for primitive slice/str receivers, and
teaching codegen's `try_lower_builtin_intrinsic` to lower `len` / `is_empty`
(extract field 1 of the fat pointer).

### 3. `&buf[written..]` and fat-pointer deref (commits `9f9887c6`, `9199c210`)

`Stdout::write_all` contains `self.write(&buf[written..])`. Two bugs here:

- `&base[a..b]` (a sub-slice) was lowered as a `Ref` of an *element* index
  (`Place{[Deref, Index(range)]}` → a `&u8`), not a sub-slice. Fixed in
  `lower_expr_to_rvalue`'s `Ref` arm: route the range-slice case through
  `lower_dynamic_range_slice` (the same path the unwrapped `Index { Range }`
  uses).
- `lower_dynamic_range_slice` derefs a `&[T]` to `[T]` and projects
  `Field(0)`/`Field(1)`. MIR's `Place::ty`/`ty_mut` had no `Field` arm for
  `Slice`/`String`, returning `Ty::ERROR` (→ codegen ICE). Added the arm
  (`9f9887c6`).
- `place_ptr`'s `ProjectionElem::Deref` arm was made to extract field 0 and
  advance `ptr` to the data address — but a fat pointer's deref must **not**
  advance `ptr` (the unsized `[T]` is laid out at the fat pointer itself).
  Advancing broke `Rvalue::Len`, which then read `{ptr, i64}` from the *data
  address* and returned garbage. Fixed in `9199c210`: only thin-pointer
  derefs load and advance.

## Verification

    # Object path
    $ glyim-cli --with-stdlib --emit=obj -o h.o h.g
    $ file h.o   # → Mach-O 64-bit object arm64 (5152 bytes)

    # Executable path
    $ glyim-cli --with-stdlib --emit=exec -o h h.g
    $ ./h
    hello
    $ echo $?
    0

Tested with `println("hello")`, `println("world")`, `println("test")`,
`println("a")`, and `println("")` — all produce the correct output.

## Constraints honoured throughout

- **`Ty::ERROR` was never lowered at codegen.** The `v15_t25_drop_error_type`
  `#[should_panic]` contract held; every fix went to the *root*
  representation issue instead of papering over the error.
- All fixes in the type/layout/codegen layers, not workarounds.
- No debug traces committed — every diagnostic was env-gated and reverted.
- Conventional commit prefixes; remaining blockers documented as they arose.

## Known follow-ups (out of scope for this goal)

- `--emit=llvm-ir` ICEs with `fn_abi_of failed: UnknownType(Ty(36317))`.
  Pre-existing; unrelated to the fat-pointer work.
- `[X0000] Err expression in THIR during lowering` fires ~10 times per
  hello-world compile. Each is a silent `thir::Expr::err(span)` not paired
  with a diagnostic — the pattern that hid the original `--emit=obj` blocker.
  Audit every `thir::Expr::err` call site and pair it with
  `diagnostics.push(...)`.
- The runtime staticlib is ~26 MB (debug); strip/release handling is polish.
