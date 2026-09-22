# Stream U-Codegen: Unstub LLVM Backend

## Mission
Remove all stubs in `glyim-codegen-llvm` related to ABI enum layout, MIR lowering, drop glue, LLVM pass pipeline mapping, and debug info. Provide fully functional implementations.

## What You Own Exclusively
- `crates/glyim-codegen-llvm/src/abi.rs`
- `crates/glyim-codegen-llvm/src/lower.rs`
- `crates/glyim-codegen-llvm/src/types.rs`
- `crates/glyim-codegen-llvm/src/passes.rs`
- `crates/glyim-codegen-llvm/src/debug.rs`
- `crates/glyim-codegen-llvm/src/tests/u_codegen.rs` (NEW FILE)
- `crates/glyim-codegen-llvm/src/tests/mod.rs` (MODIFY - safe append only)

## Exact Implementation Guide (NO STUBS ALLOWED)

### 1. `abi.rs` (Full Enum Layout)
In `FullLayoutComputer::layout_of` for `TyKind::Adt(adt_id, _)`:
- Fetch `AdtDef`. If multiple variants, compute the layout of each variant's fields. Find max size/align.
- Determine the tag size (smallest integer to fit `variants.len()`).
- Construct `VariantsShape::Multiple { tag_size, tag_align, tag_field: 0, tag_encoding: TagEncoding::Direct, variants }`.
- In `fn_abi_of`, ensure `ArgAbi` contains a flag or attribute indicating `byval` for indirect arguments.

### 2. `types.rs` (Type Mappings)
In `llvm_type_for_ty`:
- Map `TyKind::Adt` and `TyKind::Closure` to `context.struct_type(&field_types, false)`.
- Map `TyKind::Never` to `context.struct_type(&[], false)`.
- Map `TyKind::Slice` to a struct containing `{ptr_type, i64_type}` (fat pointer).

### 3. `lower.rs` (MIR Lowering)
- **`lower_const` (String):** Fix `MirConstKind::String` to return a fat pointer struct `{i8*, i64}`.
- **`lower_const` (ConstRef):** Fix `MirConstKind::ConstRef` to properly initialize the global with `set_initializer`.
- **`place_ptr` (Slice):** Implement `ProjectionElem::Slice` using `build_alloca` + `build_store` instead of returning null.
- **`lower_aggregate` (Enums):** Write the enum discriminant tag at the correct offset.
- **`lower_discriminant` (Enums):** Read and decode niche tags.
- **`lower_call` (Direct Calls):** Resolve `Operand::Constant(MirConstKind::Fn)` to a direct `FunctionValue` and use `build_call`. Add `byval` LLVM attribute for indirect args.
- **`lower_cast` (Pointers):** Emit `builder.build_bit_cast` for `CastKind::PtrToPtr` and `CastKind::FnPtrToPtr`.
- **`lower_drop` (Dealloc):** Pass real layout sizes from `FullLayoutComputer`.
- **`lower_body` & `lower_statement` (Drop Glue):** Generate per-type drop glue functions and call them from `StorageDead`.

### 4. `passes.rs` (Pipeline)
Remove the `eprintln!` debug statement. Map `(opt_level, opt_for_size)` to exact LLVM pipeline strings.

### 5. `debug.rs` (Debug Info)
Implement `declare_local` to emit `llvm.dbg.declare` intrinsics.

## Verification Commands
```bash
cargo test -p glyim-codegen-llvm
cargo clippy -p glyim-codegen-llvm -- -D warnings
cargo check --workspace
```
