## TIER 5 — Codegen / debug-info polish

### 5.1 Alignment > 16 fallback uses an `i8` array — `glyim-codegen-llvm/src/types.rs`

**Current (`opaque_sized_type`, line ~153-173, confirmed):**
```rust
_ => {
    // Fallback to i8 array, alignment might be wrong but at least size is correct
    return context.i8_type().array_type(size as u32).into();
}
```

**Root cause / correct fix (important nuance — this is not a "pick a wider
LLVM type" problem):** LLVM types above 16 bytes don't have a way to
request arbitrary natural alignment purely through the type itself
(there's no "i256-aligned-to-64" type). The correct fix in real LLVM
codegen is: **the type only needs to be size-correct; alignment is enforced
separately at every use site** (`alloca`, `GlobalVariable`) via an explicit
alignment attribute, which `inkwell` exposes as `.set_alignment(u32)` on
`PointerValue`/`GlobalValue` (check the exact inkwell API surface this
codebase's `inkwell` version exposes — grep `set_alignment` elsewhere in
`glyim-codegen-llvm` for existing usage patterns to match).

1. **Leave `opaque_sized_type`'s >16 fallback as an `i8` array** (it's
   already correct for *size*) but rename the misleading comment:
```rust
_ => {
    // Size-correct only. Callers MUST explicitly set the alignment on the
    // alloca/global instruction that uses this type via `.set_alignment`;
    // an i8-array LLVM type is always naturally 1-aligned, so relying on
    // the type's own alignment here is wrong for any align > 16 — see
    // `alloca_for_layout` / `global_for_layout` below.
    return context.i8_type().array_type(size as u32).into();
}
```
2. **Audit every call site of `opaque_sized_type`** (`grep -rn
"opaque_sized_type" glyim-codegen-llvm/src`) and, at each one that emits an
`alloca` or a `GlobalVariable` for a type whose `Layout::align` exceeds 16
bytes, add the explicit alignment call:
```rust
let alloca = builder.build_alloca(llvm_ty, name)?;
if layout.align.0 > 16 {
    alloca.as_instruction_value().unwrap().set_alignment(layout.align.0 as u32)
        .map_err(|e| /* this crate's error type */)?;
}
```
Do the equivalent for `GlobalVariable::set_alignment` at global-constant
emission sites (vtables, string literals, static data — anywhere
`opaque_sized_type` might back a global rather than a stack slot).

**Verify:** an over-aligned type (e.g. a struct requiring 32-byte alignment
for SIMD) — assert the generated `.ll`/`.o`'s alloca has `align 32`, not
the default natural alignment of an i8 array (`align 1`).

---

### 5.2 `DebugInfoCtx::debug_type_for_ty` — opaque types for `Slice`/`Ref`/`RawPtr`

**Current (confirmed structure, lines 161+):** builds real
`DIBasicType`s for `Bool`/`Int`/etc. but falls back to a generic opaque
`DIBasicType` for pointer-shaped types instead of proper
`DIDerivedType`(pointer)/composite debug types.

**Fix — extend the match with real DWARF shapes**, using inkwell's
`create_pointer_type` / `create_struct_type` (both standard
`DebugInfoBuilder` methods; check exact names against this codebase's
inkwell version — `create_pointer_type` is stable across recent inkwell
releases):
```rust
TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
    let pointee = self.debug_type_for_ty(context, *inner, ty_ctx); // recursive — cache already guards against infinite recursion for recursive types via type_cache lookup at top
    self.builder
        .create_pointer_type(
            "", // anonymous, matches how DWARF represents raw pointer types
            pointee,
            target.ptr_size_bits(), // whatever this crate's TargetInfo exposes for pointer width — confirm exact accessor
            0x04, // DW_ATE_unsigned or the same address-space flag used elsewhere in this file — match the Bool/Int arms' literal above rather than inventing a new constant
            AddressSpace::default(),
        )
        .as_type()
}
TyKind::Slice(inner) => {
    // A slice is `{ ptr: *T, len: usize }` at the ABI level (matches
    // glyim-layout's FieldsShape for slices, see SimpleLayoutComputer's
    // TyKind::String/slice handling) — represent it as a two-member
    // DWARF struct, not an opaque blob.
    let ptr_ty = self.debug_type_for_ty(context, ty_ctx.mk_ptr_ty(*inner) /* or however this crate constructs a *T Ty for debug purposes — may need to build the pointer DIType directly without round-tripping through a real Ty, i.e. reuse the Ref/RawPtr arm's body directly */, ty_ctx);
    let len_ty = self.debug_type_for_ty(context, /* usize Ty */, ty_ctx);
    let members = [
        self.builder.create_member_type(file, "ptr", file, 0, target.ptr_size_bits(), target.ptr_align_bits(), 0, 0x0, ptr_ty),
        self.builder.create_member_type(file, "len", file, 0, /* usize bit width */, /* usize align */, target.ptr_size_bits(), 0x0, len_ty),
    ];
    self.builder
        .create_struct_type(file, "&[T]", file, 0, target.ptr_size_bits() * 2, target.ptr_align_bits(), 0x0, None, &members, 0, None, "")
        .as_type()
}
```
This is illustrative shape, not copy-paste-ready — the exact
`DebugInfoBuilder` method signatures (parameter order/count) depend on the
pinned `inkwell` version in `glyim-codegen-llvm/Cargo.toml`; look up that
exact version's `create_member_type`/`create_struct_type`/
`create_pointer_type` signatures before writing this (they've changed
across inkwell releases) rather than trusting the shape above literally.

**Verify:** compile a program with a `&[i32]` local, run under `gdb`/`lldb`
with `-g` debug info enabled, `print` the local — it should show a
`ptr`/`len` struct instead of an opaque blob.

---

### 5.3 `fn_sig` fallback produces empty `FnSig` — `glyim-codegen-llvm/src/lower.rs`

**Current (confirmed, `LoweringCtx::lower_body`):** when `ty_ctx.fn_sig(id)`
returns `None`, an empty `FnSig` (no params, presumably unit return) is
substituted silently, which can produce a wrong LLVM function signature
(wrong arg count) that later either miscompiles or crashes at the call
site.

**Fix:** this should never be a silent substitution — a missing `FnSig` for
a function actually being lowered to LLVM IR is an internal-compiler-error,
not a recoverable case (by the time codegen runs, every function that will
be called must have already had its signature computed during typeck/HIR
lowering). Replace the fallback:
```rust
let fn_sig = match self.ty_ctx.fn_sig(*fn_def_id) {
    Some(sig) => sig.clone(),
    None => {
        // This is a compiler bug, not a user error — every function reaching
        // codegen must have a resolved signature. Fail loudly with enough
        // context to find the missing-signature bug upstream, rather than
        // emitting a function with the wrong arity that will crash far away
        // from the actual cause.
        return Err(CodegenError::InternalError(format!(
            "no FnSig registered for {:?} — this indicates a bug in typeck/mono, not a user-facing error",
            fn_def_id
        )));
    }
};
```
Check `CodegenError`'s actual variants in this crate/`glyim-codegen` before
using `InternalError` literally — use whichever existing variant means
"compiler bug, not user error" (if none exists, add one; don't reuse a
user-diagnostic variant for this).

**Verify:** existing test suite should be unaffected (this only changes
behavior for a case that was already a latent bug); add a targeted unit
test that intentionally omits a `FnSig` registration and asserts codegen
now returns `Err(InternalError(..))` instead of silently emitting a
zero-arg function.

---

### 5.4 `glyim-codegen` (bytecode backend) — `Subslice`/`ConstantIndex` scaling & `vtable.rs`

**`emit_place_address` `ProjectionElem::Subslice`** (confirmed lacks
element-size scaling) and **`ConstantIndex` `from_end` for slices**
(confirmed falls back to `*offset`, ignoring the slice's actual runtime
length) — same root cause as Tier 0.1 (`get_element_size`), same fix
shape: this backend needs a `LayoutComputer` too. Check whether
`BytecodeBackend` already holds one (`with_ty_ctx` constructor at
`glyip/src/commands.rs` line ~453 suggests it holds a `TyCtx`, likely also
already has layout access — grep `LayoutComputer`/`SimpleLayoutComputer`
in `glyim-codegen/src/lib.rs` to check before adding a redundant one).

For `Subslice`: multiply both the base-pointer offset and the resulting
`len` field by the real element size (from `layout_of(elem_ty).size`)
instead of treating offsets as already-byte-scaled.

For `ConstantIndex` `from_end` on a slice (not array): a slice's length is
a **runtime value** (the `len` field of its fat pointer), not a compile-time
constant — `min_length` alone can't resolve `from_end` the way it can for a
fixed-size array. The fix must emit a runtime subtraction
(`actual_offset = runtime_len - offset`) instead of trying to fall back to
`*offset` as if it were `from_end: false`:
```rust
ProjectionElem::ConstantIndex { offset, min_length, from_end } => {
    let index_val = if *from_end {
        match place_ty_kind {
            TyKind::Array(_, _) => min_length_derived_constant_minus_offset, // existing array path, presumably already correct — confirm
            TyKind::Slice(_) => {
                // NEW: runtime length, not min_length.
                let runtime_len = self.emit_slice_len(base_place)?; // read the fat-pointer's len field — reuse whatever the Subslice/emit_place_address code already does to read a slice's len
                self.emit_sub(runtime_len, self.emit_const_usize(*offset))
            }
            _ => return Err(/* unsupported */),
        }
    } else {
        self.emit_const_usize(*offset)
    };
    // ...proceed with existing scaled-address computation using get_element_size...
}
```

**Verify:** bytecode-backend test for a slice-pattern binding on a runtime
slice of length 7, matching `[.., last]` (a `from_end` `ConstantIndex`) —
must read the correct last element regardless of the slice's actual
runtime length, not just when length happens to equal whatever `min_length`
implies.

**`glyim-codegen/src/vtable.rs`** needs no changes — confirmed it's just
index constants, already correct (see Tier 1.2's note); it's a consumer of
the fix in Tier 1.2, not something to touch here.
