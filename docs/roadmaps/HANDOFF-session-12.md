# HANDOFF — session 12 (LLVM builtin dispatch) — COMPLETE

**HEAD**: `a5a7d72f`
**Working tree**: clean (except this file until it is committed).
**Matrix**: 4599/4599 workspace tests pass.
**Differential**: **0 disagreements** (LLVM backend vs MIR interpreter,
run-pass corpus). Session started at 4; ended at 0.

---

## 0. What landed

| Commit | Fix |
|---|---|
| `a5a7d72f` | Closure calls lowered inline; `zonk_closure_sigs` folds `Infer` out of closure signatures; `MirConstKind::Fn` falls back to `closure_sigs` when declaring a callee. |
| (accumulated) | `Option`/`Result`/`Vec::push` inline builtin dispatch; receiver-inference fallback; dest-type override for `Infer` outputs; String/Vec layout fixes. |

The commit `a5a7d72f` is a single squash of everything that had piled
up uncommitted across the session: the enum-payload builtins
(`is_some`/`is_none`/`is_ok`/`is_err`/`unwrap`/`expect`/`unwrap_err`/
`expect_err`/`unwrap_or`/`unwrap_or_default`), `Vec::push`, the
receiver-inference fallback in `lower_call`, and the closure fixes.

## 1. Environment gotchas (still true)

- **`RUSTC_WRAPPER=kache`** — fine, leave it on. `touch` a source file
  to force a rebuild if a change seems not to take.
- **Read-only `.rmeta` in `target/`** — `chmod -R u+w target` fixes it.
  Re-check after every failed build.
- **No `timeout(1)` on macOS.**

## 2. Process-leak hazard (IMPORTANT)

Running the differential via bare `cargo test` and then `kill -9`-ing
the cargo parent **orphans its descendants** — the per-fixture
`glyim-cli` compiles that spawn `cc`/`ld`. These accumulate and can OOM
the machine. Two mitigations, both used successfully this session:

- **Per-fixture**: `/tmp/run_fixture.py` — runs `glyim-cli --emit=exec`
  on one `.g` file inside a `setsid` process group; on timeout it
  `killpg`s the whole group. Never invoke `glyim-cli` in a shell loop.
- **Full differential**: `/tmp/run_differential.py` — same technique,
  but wrapping the whole `cargo test … differential` invocation with a
  20-minute cap and `killpg` on timeout.

Both scripts are on the machine but in `/tmp`, which macOS prunes —
recreate them from the definitions below if missing.

## 3. Debugging method that worked

For an ICE inside `llvm_fn_type_from_sig_inner` / `fn_abi_of`:

1. Add a `std::env::var("GLYIM_TMP_W")`-gated probe at the panic site
   that prints the offending `FnSig` (output + each input, with
   `ty_kind`). This is far faster than guessing.
2. `strings target/debug/glyim-cli | grep -c 'GLYIM_TMP_W'` before
   trusting a probe's *absence* — 0 means the build is stale.
3. Remove the probe before committing.

## 4. Remaining known issues (unchanged / not addressed this session)

- `probe_string_push_str` — verify it is still green (the `as_bytes`
  shape was fixed in an earlier session).
- Async / `block_on` codegen — documented in
  `crates/glyim-test/Cargo.toml` as a known-broken path; not touched.

## 5. Resume

```bash
cd /Users/adm/Documents/Repos/glyim-v2
chmod -R u+w target
cargo build -p glyim-cli
# recreate the runner scripts (see §2) if /tmp was pruned, then:
python3 /tmp/run_differential.py
