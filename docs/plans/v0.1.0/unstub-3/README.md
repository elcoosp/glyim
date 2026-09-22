# Glyim Stub Remediation — Plan Index

Read `00_ground_rules.md` first. Then work the tiers in order:

| File | Tier | Items | Depends on |
|---|---|---|---|
| `00_ground_rules.md` | 0 | 0.1–0.5: interpreter soundness bugs (`get_element_size`, `ConstantIndex`/`Subslice` write panics, `PtrToPtr` no-op *(confirmed non-bug)*, `Call`/`Drop` scope note, `Len` fallback) | none |
| `01_tier1_core_semantics.md` | 1 | 1.1 closures/captures, 1.1b closure nominal type, 1.2 vtables (+ `ImplDef.items` schema change), 1.3 `Iterator::next` real resolution, 1.4 `Range` lowering bug, 1.5 const-eval expression coverage, 1.6 drop elaboration per-projection (soundness bug, dependency-ordered here for `fixedbitset`), 1.7 dynamic range slicing | Tier 0 |
| `02_tier2_trait_system.md` | 2 | 2.1 coherence overlap ignores generics, 2.2 HRTB conservative-but-provable cases, 2.3 object safety associated-types/supertraits | 1.2.a, 1.2.d |
| `03_tier3_build_tool.md` | 3 | 3.1 transitive dependency resolution, 3.2 `glyip test` doesn't execute tests, 3.3 registry-disabled error message | none (independent of 1/2) |
| `04_tier4_macro_system.md` | 4 | 4.1 fragment-spec matching (Stage A + Stage B), 4.2 `line!`/`column!` approximation, 4.3 `include!` CWD-relative, 4.4 `stringify!` normalization | none |
| `06_tier6_lsp_and_tier7_harness.md` | 6+7 | 6.1–6.5 LSP reference graph / rename / completion / unused-imports; 7.1–7.4 test harness real linking+execution, mock wiring | 1.1 (for 6.2), 1.2.a (for 6.4) |

`05_tier5_codegen_debuginfo.md` (Tier 5: over-alignment fallback, DWARF
pointer/slice debug types, `fn_sig` fallback → hard error, bytecode-backend
`Subslice`/`ConstantIndex` scaling) has no cross-tier dependencies and can
be done any time after Tier 0.

## Items explicitly confirmed as *not* bugs (don't "fix" these — noted per-item so they aren't re-flagged)
- `glyim-mir-interp` `CastKind::PtrToPtr`/`FnPtrToPtr` no-op (0.3)
- `glyim-type/src/ty_ctx_mut.rs` `register_builtin_ranges` (1.4 — already complete; the bug is in `lower_rvalue.rs`, not here)
- `glyim-codegen/src/vtable.rs` index constants (1.2 / 5.4 — already correct; consumer of the layout-side fix)
- `glyim-test/src/harness/strategy.rs` `RunPassStrategy`/`RunFailStrategy` (7.2 — already correct; blocked only on `executable_path` being supplied)
- `glyip` `HttpRegistryClient` (3.3 — correctly implemented behind an optional feature; not a stub)

## Schema/plumbing changes that ripple across multiple items (do these once, early)
1. `ImplDef` gains `items: Vec<(Name, FnDefId)>` (`glyim-solve/src/solver.rs`) — needed by 1.2, 1.3, 2.x, 6.4.
2. `TraitContext::trait_defs()`/`impl_defs()` lose their `#[cfg(test)]` gate — needed by 1.2.c.
3. `glyim-cli`'s `mod linker;` becomes `pub mod linker;` — needed by 3.2, 7.2.
4. `glyim-mir-interp` and `glyim-codegen` both gain a `glyim-layout` dependency for real element sizing — needed by 0.1, 5.4.
5. `LocalEnv::next_var_id()` accessor (`glyim-typeck/src/env.rs`) — needed by 1.1.
6. `glyim-frontend` exposes `try_parse_fragment` — needed by 4.1 Stage B only (not required for Stage A).

## What this plan deliberately does not attempt
- Full general HRTB/region-inference solver (2.2 scopes down to "cheap
  provable cases", not a complete implementation — flagged explicitly in
  that section).
- Byte-exact `stringify!`/macro source fidelity requiring spans on every
  `TokenTree` (4.4 — scoped to deterministic re-pretty-printing instead).
- Diamond-dependency semver unification in `glyip` (3.1 — explicitly out of
  scope, notes why).
- Full unwind/cleanup-block semantics in the tree-walking interpreter (0.4
  — documented as a scope boundary, not implemented).
These are called out inline in their sections so nobody mistakes "scoped
down" for "still a stub."
