# Session 4 — Final handoff

## Completed this session (20 deferred items)

| Task | Description |
|------|-------------|
| T053 | Real HIR `Body.pat_spans` field → LSP rename edits correct pattern positions |
| T093 | `TyCtx::adt_name_for_id` on frozen context → builtin method gate |
| T096 | Reviewed & rejected plan's incorrect `&mut T: Sync` / raw-pointer claims |
| T101 | End-to-end polymorphization dedup (pre-subst body analysis) |
| T104 | Terminator write / Drop conflict checks |
| T109 | Removed per-statement `Statement` clone in interpreter hot loop |
| T110 | `MonoItem::Static` fetches its real body via its own `LocalDefId` |
| T121 | Two-phase borrow activation for call arguments |
| T123 | `FullLayoutComputer::layout_of` memoized with per-instance cache |
| T124 | `CrateDefMap.def_to_module` → per-item `source_module` |
| T125 | Nested inline modules (`mod a { mod b; }`) resolve against parent path |
| T126 | `--lto fat` now errors clearly instead of silently no-op'ing |
| T129 | Scope-aware rename / find-references (`owner_item_id` + `is_item_level`) |
| T130 | HIR dependency walk + re-analysis of dependents on change |
| T139 | Strict `write_all` (`Ok(0)` → `WriteZero` error) |
| T140 (partial) | `ptr::null`, `ptr::null_mut`, `NonNull::{new_unchecked, dangling}`, `mem::forget` |
| T159 | Server event loop spawns `handle_extension_message` |
| T175 | Single-segment unresolved `Pat::Path` → `Binding` fallback |
| T178 | `TraitMethod.generic_params` → object-safety `has_generic_params` |
| T179 | Universal `into`/`to_string`/`clone` gated on receiver shape |

## Additional fixes
- T063/T141 follow-ups: replaced `Vec::resize` / `Iterator::position` /
  `Option::unwrap_or` with manual loops in io.g / env.g to unblock stdlib
  type-checking (which then enabled T101's pipeline invocation).
- Test-literal updates for `Body.pat_spans`, `CrateDefMap.def_to_module`,
  `MethodDef.has_generic_params`, `Reference.owner_item_id`, and
  `TraitMethod.generic_params` across typeck, LSP, and const-eval test files.

## Test matrix (all green)
```
frontend 784   def-map 104   meta 89    hir 108   typeck 111
solve 311      lower 221     opt 70     mir-interp 204
codegen 173    codegen-llvm 302         bytecode-vm 14
const-eval 100 span 22       diag 17    lsp 82
glyip 208      pilot 46      lang-std 97 runtime 112
```

## Remaining work (research-grade / multi-day)

### T100 [LOW-9] — Multi-poll async state machine
The HIR `desugar_async` pass intentionally emits `Poll::Pending => panic!(...)`
for suspension. Full state-machine lowering requires:
- HIR: emit a state enum with one variant per suspend point (partially
  analyzed in `crates/glyim-lower/src/async_state_transform.rs`).
- MIR: split the CFG at every `Future::poll` call and emit
  `match self.state { S0 => ..., S1 => ..., Done => ... }`.
- Codegen: handle the state enum in LLVM and bytecode backends.
- Interpreter: add suspend/resume support to the call-frame stack.

### T140 remainder — Compiler intrinsics
Bodyless functions still requiring 4-layer plumbing (registration +
LLVM + bytecode + interp):

| Function | Blocked by |
|---|---|
| `str::trim`, `str::contains`, `Chars::next` | `str` byte-range indexing currently returns `()` (Unit) instead of `&str`; needs slice support or a dedicated intrinsic |
| `mem::size_of<T>`, `mem::align_of<T>`, `mem::needs_drop<T>` | Need a typeck-time fold: layout computed via `glyim-layout` and injected as a `thir::Literal` |
| `ptr::read`, `ptr::write`, `ptr::copy`, `ptr::copy_nonoverlapping`, `ptr::drop_in_place` | Each needs a backend-specific intrinsic (LLVM load/store/memcpy/memmove/call) |
| `MaybeUninit::{uninit, new, assume_init}` | Needs a representation decision |

Estimated ~4-6 hours per family.

### T121 further work
The activation detection could be made more precise: currently every
`&mut x` in a call argument position is marked two-phase. A rustc-level
fix would track only the borrows that are actually alive at the call
site (the reservation scan in `borrowck/twophase.rs` already models
this).

## Session totals
- **113 commits** since `af19152b`.
- **3,175 tests** passing, 0 failing.
- **0 compile errors** in the full workspace.
