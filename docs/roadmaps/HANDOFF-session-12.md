# HANDOFF — session 12 (builtin dispatch, closures, atomics, for-loops)

**HEAD**: the tip of `main` when this was written.
**Working tree**: clean.
**Matrix**: 4599/4599 workspace tests pass (2 skipped).
**Differential**: **0 disagreements** on the run-pass corpus
(126 compared, 0 interp-only, 4 skipped = `// ignore` library fixtures).
Session started at 4 disagreements.

---

## 0. What landed (in order)

| Commit | Fix |
|---|---|
| `a5a7d72f` | Closure calls lowered inline; `zonk_closure_sigs`; `MirConstKind::Fn` closure-sig fallback. |
| `9766b87b` | `lower_body` shares closure param locals; `[T; N]::len` constant-folded; un-ignore 3 closure fixtures. |
| `1921092d` | Closure value layout is `{fn_ptr, captures}`; `Place::ty` closure arm; projected-place capture reads. |
| `cedf14ca`, `1cc14abf`, `94c6115e`, `be396ce4`, `0e559ba7` | `KNOWN_GAPS.md` updates (closures, atomics, combinators, `Vec::new`, iterator protocol). |
| `2c59c67f` | `AtomicBool`/`AtomicUsize::compare_exchange` inline on both backends. |
| `c2f56d8c` | `String::from_utf8` inline on both backends. |
| `fc338328` | `from_utf8` byte-GEP scale fix + interp `Vec<u8>` extraction fix. |
| `2171bde9` | For-loop over `Range<usize>` takes the multi-iteration path (`SwitchInt` on the discriminant; `impl Iterator for Range<usize>`). |

## 1. Process-leak hazard (READ THIS)

Running the differential via bare `cargo test` and then `kill -9`-ing the
cargo parent **orphans its descendants**. Use the `/tmp` runners (recreate
if macOS pruned `/tmp`):

- `/tmp/run_fixture.py <fixture.g> [--with-stdlib]` — one fixture, `setsid`
  process group, `killpg` on timeout.
- `/tmp/run_differential.py` — the whole differential, 20-min cap.

## 2. Environment gotchas

- `RUSTC_WRAPPER=kache` is fine. `touch` a source file to force a rebuild.
- Read-only `.rmeta` in `target/` → `chmod -R u+w target`.
- **Compile cache** hides stale results: set `GLYIM_CACHE_DIR=$(mktemp -d)`
  to force a fresh compile when probing.
- `glyim-cli --emit llvm-ir -o out.ll <fixture>` dumps IR — fastest way to
  see a mis-scaled GEP or a wrong slot.

## 3. Debugging method

`std::env::var("GLYIM_TMP_W")`-gated probes that print the offending
`Ty`/`FnSig` with `ty_kind`. `strings target/debug/glyim-cli | grep -c
GLYIM_TMP_W` before trusting a probe's *absence*. **Remove probes before
committing** — a botched sed/python removal once deleted a live `Ty::ERROR`
arm and a real `unify` call; verify with `git diff` and a build.

## 4. Remaining known gaps (see `KNOWN_GAPS.md`)

- **`for i in 0..5` (Range<i32>)** — needs an `into_iter()` desugar in
  typeck's `For` arm. Adding `impl Iterator for Range<i32>` alone does not
  work: the impl scan binds the range's int var to `usize` first.
- **Closure-passing combinators** — `Option::map`, `Result::{map,map_err,map_or}`.
  Multi-layer (typeck dispatch + both backends).
- **`Vec::new()` element unification (1f)** — `Vec<Infer>` never unifies with
  `push(5)`'s `i32`; cross-namespace `Infer(Int)` vs `Infer(Ty)`.
- **Slice iteration** — `slice::iter`/`as_slice` on a bare array.
- **Async / `block_on`** — Linux-gated.

## 5. Resume

```bash
cd /Users/adm/Documents/Repos/glyim-v2
chmod -R u+w target
cargo build -p glyim-cli
python3 /tmp/run_differential.py   # recreate if /tmp was pruned
