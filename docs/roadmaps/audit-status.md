# Audit status tracker

Companion to `glyim-bug-and-performance-audit.md`. Records which findings have
been **verified fixed** or **verified stale** (not reproducible), so future
sessions do not re-investigate them. Ground truth is a freshly built CLI +
`--emit=mir`/`--emit=llvm-ir` + the full workspace suite (currently
**4530/4530 pass, 2 skipped**).

Rebuild before verifying: `cargo build -p glyim-runtime -p glyim-cli`
(the runtime staticlib is required for `--emit=exec`).

## Criticals verified FIXED (behavior-confirmed this session)

| ID | Evidence |
|----|----------|
| HIR-1 | `$()*` empty repetition terminates (matcher `new_i == i` guard). |
| HIR-2 | depth-aware bindings; `stmts!(1+2, 3)` expands to 2, not 6. |
| HIR-3 | nested repetition `$( $( $x ),* ),*` expands. |
| HIR-4 | multi-metavar patterns match (`consume_fragment`). |
| HIR-10 | async tail rewrites the outermost block. |
| HIR-11 | statements between/after awaits run (interp → 103). |
| HIR-29 | `match f()` calls `f` once (MIR has 1 `Call`). |
| HIR-30 | `loop { break 42; }` writes `break_place` (MIR has `Int(42)`). |
| HIR-31 | `&mut` capture aliases (interp `c == 2`). |
| FE-6 | deep unary chain / array nesting → `P0001` (no hang). |
| SOLVE-1 | int-var cycle repro no longer hangs. |
| SOLVE-7 | `can_coerce` checks pointees with correct precedence. |
| SOLVE-8 | blanket `impl<T: Foo> Foo for T {}` compiles clean. |
| MIR-1 | const-prop uses a worklist; no stale block-exit substitution. |
| MIR-6 | DCE counts `Drop` terminators as uses. |
| MIR-10 | drop-elaboration emits `new_blocks` without clobbering CFG. |
| MIR-13 | interpreter truncates to the dest width (`trunc_to_ty`). |
| MIR-14 | `IntToInt` casts truncate/sign-extend correctly. |
| MIR-17 | enum field reads/writes both account for the tag. |
| MIR-21 | `places_conflict` handles `Index` projections. |
| RT-3 | VM `Drop` wire format matches the emitter. |
| RT-4 | `OP_LEN` operand format consistent. |
| RT-11 | bytecode bool switch branches not inverted (e2e exit 0). |
| RT-12 | `block_offsets` table produced. |
| RT-31 | proc-macro tempdir persisted past dlopen. |
| LL-1 | signed/unsigned compare picks `SLT`/`ULT` by operand type. |
| LL-2 | widening uses `build_int_s_extend` (sext), not zext. |
| LL-7 | all allocas emitted in `entry`, not per-loop. |
| LL-9 | ZST args (`fn f(u: (), x: i32)`) compile. |
| LL-10 | `&&`/`||` short-circuit (no eager div-by-zero). |
| INF-11 | LSP source map uses UTF-16 columns. |
| INF-12 | identifier extraction uses `char_indices`. |
| INF-13 | rename edits only the identifier. |
| INF-16 | LSP document sync (`didOpen`/`didChange`) wired. |
| INF-23 | array/slice drop glue emits the per-element loop. |

## Criticals verified STALE / not reproducible

| ID | Why |
|----|-----|
| LL-6 | struct literal operands already in declaration order. |
| LL-11 | `resolve_trait_method_fn` probe rollback present. |
| SOLVE-2 | scoped fix via SOLVE-1; blanket chain-follow intentionally absent. |
| MIR-11 | MAY-init consumed safely (tested `if c { s = make(); }`). |
| MIR-24 | audit's move-analysis fix produces false positives; reverted. |
| RT-13 | interpreter `--backend=bytecode` array index works (stride ok). |
| HIR-4 | (also listed fixed) multi-metavar works. |

## How to re-verify

```
cargo build -p glyim-runtime -p glyim-cli
target/debug/glyim-cli <prog>.g --emit=mir      # writes <prog>.mir
target/debug/glyim-cli <prog>.g --emit=llvm-ir -o <prog>.ll
cargo nextest run --workspace
```

## Remaining unverified (mostly High/Medium)

The ~120 High/Medium findings in MIR / RT / INF / HIR / LL have not all been
individually re-verified; treat the audit's severity labels as unconfirmed until
reproduced. Several "Critical" labels were already stale, so re-verify before
fixing.
