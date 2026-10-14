# HANDOFF — session 12 (builtin dispatch + closures) — COMPLETE

**HEAD**: `595abdc3`
**Working tree**: clean.
**Matrix**: 4599/4599 workspace tests pass (2 skipped).
**Differential**: **0 disagreements** on the run-pass corpus
(120 compared, 0 interp-only, 4 skipped = `// ignore` library fixtures).
Session started at 4 disagreements; ends at 0.

---

## 0. What landed (in order)

| Commit | Fix |
|---|---|
| `a5a7d72f` | Closure calls lowered inline; `zonk_closure_sigs`; `MirConstKind::Fn` falls back to `closure_sigs`. Plus the accumulated enum/`Vec` builtin work. |
| `9766b87b` | `lower_body` shares closure param locals; `[T; N]::len` constant-folded; un-ignore 3 closure fixtures. |
| `1921092d` | Closure value layout is `{fn_ptr, captures}`; `Place::ty` closure arm; captures read via projected places. Adds `closure_fnptr.g`. |
| `cedf14ca` | `KNOWN_GAPS.md`: closures marked fixed; combinators still open. |

### The four closure bugs (all fixed)

1. **Signatures never zonked.** `closure_sigs` stored `Infer`-carrying
   `FnSig`s; `zonk_bodies` didn't touch them → `fn_abi_of` ICE. Fixed by
   `glyim-typeck::zonk::zonk_closure_sigs`.
2. **Declared void.** `MirConstKind::Fn` looked up `fn_sigs` only, missing
   `closure_sigs`, so closures got a placeholder `void` type. Fixed by the
   fallback.
3. **Param locals allocated twice.** `lower_closure` and `lower_body` each
   allocated one → prologue wrote slot A, body read slot B. Fixed by sharing
   via `local_var_map`.
4. **No fn-pointer in the layout.** Only captures were counted, so a
   non-capturing closure was zero-sized and stored through a null alloca
   (SIGBUS in a `fn(..)` position). Fixed in `glyim-layout`, plus a
   `Place::ty` closure arm and projected-place capture reads.

## 1. Process-leak hazard (READ THIS)

Running the differential via bare `cargo test` and then `kill -9`-ing the
cargo parent **orphans its descendants** — the per-fixture `glyim-cli`
compiles that spawn `cc`/`ld`. These accumulate and can OOM the machine.

Use the `/tmp` runners (recreate them if macOS pruned `/tmp`):

- `/tmp/run_fixture.py <fixture.g> [--with-stdlib]` — one fixture, its own
  `setsid` process group, `killpg` on timeout.
- `/tmp/run_differential.py` — the whole differential, 20-min cap, `killpg`.

Both `setsid` the child so a timeout kills the **entire tree**, which a
plain `kill -9 $pid` cannot do.

## 2. Environment gotchas

- `RUSTC_WRAPPER=kache` is fine — leave it on. `touch` a source file to
  force a rebuild if a change seems not to take.
- Read-only `.rmeta` in `target/` → `chmod -R u+w target`.
- **Compile cache** can hide stale results: the CLI prints
  `cache hit — skipping compilation` and skips typeck/codegen. Set
  `GLYIM_CACHE_DIR=$(mktemp -d)` to force a fresh compile when probing.
- No `timeout(1)` on macOS.

## 3. Debugging method that worked

For an ICE in `fn_abi_of` / `llvm_fn_type_from_sig_inner`:

1. Add a `std::env::var("GLYIM_TMP_W")`-gated probe at the panic site that
   prints the `FnSig` (output + each input, with `ty_kind`). Far faster than
   guessing.
2. `strings target/debug/glyim-cli | grep -c 'GLYIM_TMP_W'` before trusting
   a probe's *absence* — 0 means the build is stale.
3. Remove the probe before committing.

For closure bugs specifically, `glyim-cli --emit llvm-ir -o out.ll <fixture>`
dumps the IR — the fastest way to see a wrong slot or a null-alloca store.

## 4. Remaining known gaps (see `KNOWN_GAPS.md`)

- **Closure-passing combinators** — `Option::map`, `Result::{map, map_err,
  map_or}`, `Option::unwrap_or_else`. Neither backend implements them
  (interp has no arms; codegen whitelist lacks them). A real feature on both
  backends, not a one-liner.
- **`Vec::pop().unwrap()` (KNOWN_GAPS 1f)** — the receiver is typed
  `Vec<Error>`, so `pop`'s `Option<T>` payload never resolves. A typeck
  inference-ordering issue, not codegen.
- **`compare_exchange`** — needs `Result` aggregate construction + a
  `glyim_atomic_usize_cas` helper.
- **Async / `block_on`** — gated to Linux; not runnable on this host.

## 5. Resume

```bash
cd /Users/adm/Documents/Repos/glyim-v2
chmod -R u+w target
cargo build -p glyim-cli
# recreate /tmp/run_differential.py if needed, then:
python3 /tmp/run_differential.py
```
