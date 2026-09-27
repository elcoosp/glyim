# Handoff (updated) — `glyim-v2` stdlib hello-world milestone

## Current status

**Primary objective achieved and committed.** `glyim-cli --with-stdlib
--emit=obj` on `fn main() { println("hello"); }` produces a valid 2112-byte
Mach-O arm64 relocatable object. **All 4172/4172 workspace tests pass.**

Commit: `db7cd2d3  feat(type,typeck): canonical io.g ADTs + deterministic
impl FnDefId — hello world --emit=obj produces object`

## What changed (5 files, +245/-9)

### 1. `crates/glyim-type/src/builtin_adts.rs`
Added `Error`, `ErrorKind`, `BufReader`, `BufWriter` variants with reserved
`AdtId`s **1070..1073**. Rationale documented inline: without a canonical
fixed id, these io.g ADTs split between a def-map `LocalDefId` reached via the
crate-root "walk every module" pre-pass (frequently a *re-export* module,
e.g. `Adt39`) and the definition module's own def-map id (`Adt224`/`Adt227`).
The split produced `Adt39 vs Adt227` at the struct-literal level in
`Error::last_os_error` / `BufReader::with_capacity` /
`BufWriter::with_capacity`, which cascaded into `Ty::ERROR` at codegen and
tripped the `v15_t25_drop_error_type` contract.

### 2. `crates/glyim-type/src/ty_ctx_mut.rs`
`register_builtin_ranges` now pre-registers placeholder `AdtDef`s for the four
new ids and their name→id entries. Pass 1 / Pass 2 in `glyim-typeck` overwrite
the placeholders with the real `io.g` bodies at the same ids.

### 3. `crates/glyim-typeck/src/tyconv.rs`
`resolve_name_to_adt_ty` now canonicalizes any name that lives in `BuiltinAdt`
**after** the type-alias expansion block but **before** the def-map fallbacks.
The existing canonical lookup re-interns through ctx's resolver (a different
rodeo than the HIR-interned path name), so `ctx.adt_id_by_name(ctx_name)`
silently missed for stdlib names and fell through to the def-map path.

**Critical ordering constraint**: the canonicalization block MUST run *after*
alias expansion. `type Result<T> = Result<T, Error>` expands 1-arg
`Result<usize>` to 2-arg `Result<usize, Error>`; if canonicalization ran first,
the padded `E` slot got a fresh inference var and `Result::Err(ref e)` bound
`e: Infer(..)` instead of `&Error`, breaking `.kind()` in fs.g/net.g's
`read_to_end` overrides.

### 4. `crates/glyim-typeck/src/lib.rs`
- The const pre-registration pass now re-interns the name through ctx's
  resolver (`ctx_name`) before calling `adt_id_by_name`, and canonicalizes
  through `BuiltinAdt` first. Previously it passed the raw HIR `first.name`
  and always missed, silently overwriting the canonical entries.
- New `pre_allocate_impl_method_ids` function (called from `typeck_crate`
  before `check_fn_items_in_module`). It walks `ItemKind::Impl` arms
  depth-first, allocates a `LocalDefId` for every method with a body, inserts
  `BodyId → LocalDefId` into `body_owner_map`, and pre-registers the method's
  `FnSig`. This closes the source-order gap: `impl Error { fn last_os_error }`
  calls `ErrorKind::from_raw_os_error` but `impl ErrorKind` is declared later
  in io.g; without pre-allocation the callee's id wasn't assigned yet when the
  caller's body was checked, so `check_path` returned `Ty::ERROR`.

### 5. `crates/glyim-typeck/src/unify.rs`
`check_path` step 2c.1 now looks up the callee's `LocalDefId` from
`body_owner_map` (guaranteed populated by `pre_allocate_impl_method_ids`).

## NOT done (documented follow-ups)

### A. `--emit=exec` fails with `PtrToPtr cast on non-pointer`
`cargo run -p glyim-cli -- --with-stdlib --emit=exec` produces the object but
the pipeline emits `[X0000] PtrToPtr cast on non-pointer @0..0` before
linking. Root cause is presumably an `as` cast from a non-pointer type; the
diagnostic site is `crates/glyim-codegen-llvm/src/lower.rs:2352` but the
producing MIR is in `crates/glyim-lower/src/lower_rvalue.rs:723`
(`_ => CastKind::PtrToPtr` fallback). The **object** is fine — this only
blocks the link step.

### B. Object contains undefined `___glyim_fn_*` symbols
`nm h.o` shows undefined `___glyim_fn_1073741879`, `___glyim_fn_9104`, etc.
These correspond to `FnDefId`s not emitted in this translation unit. Likely
they are monomorphized-extern references that the object writer currently
treats as `declare` rather than emitting a body (e.g. cross-crate runtime
`extern "C"` functions that should resolve against `glyim-runtime`), or
mono roots missing from `discover_mono_roots`. Investigate
`crates/glyim-lower/src/mono.rs` and the LLVM backend's `declare_function`
usage.

### C. `def_map.max_local_def_id` starts near `0x4000_0000` (2^30)
`nm` output shows method `FnDefId`s like `1073741879 = 0x4000_0000 + 55`.
Confirm this is intentional (a reserved high range) and not a stale
hard-coded value.

## Diagnostic env vars still in the tree (removed; re-add if needed)

`GLYIM_DBG_T0001`, `GLYIM_DBG_KIND`, `GLYIM_DBG_PAT`, `GLYIM_DBG_REGISTER_ADT`,
`GLYIM_DBG_STDADT`, `GLYIM_DBG_ADT_REG`, `GLYIM_DBG_SYNTH` — none are in the
committed tree; the session used them ad-hoc and reverted.

## Useful commands

```bash
# Smoke test — produces a valid 2112-byte Mach-O arm64 object
TMPD=$(mktemp -d)
cat > "$TMPD/h.g" << 'GEOF'
fn main() { println("hello"); }
GEOF
cargo run -q -p glyim-cli -- --with-stdlib --emit=obj -o "$TMPD/h.o" "$TMPD/h.g"
file "$TMPD/h.o"
nm "$TMPD/h.o"

# Full test suite
cargo nextest run --workspace
