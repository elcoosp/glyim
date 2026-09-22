# 2. Semi-Implemented / Placeholder Features

## 2.1 `#[ignore]` Attribute in Tests (heuristic comment parsing)

### Current state

`glyip/src/test_discovery.rs` detects `#[ignore]` by scanning for the
literal comment lines `// #[ignore]` or lines starting with `//` that
contain the word `ignore` (`trimmed.starts_with("//") && trimmed.contains
("ignore")`), tracked via a `pending_ignore_attr` bool consumed by the next
`#[test]`-marked function. This is because glyim source doesn't parse
`#[...]` attribute syntax at all yet (a much larger frontend feature); the
comment convention is a deliberate, documented interim workaround, not an
oversight.

### Target design

Two independent improvements, do both:

1. **Tighten the heuristic** so it doesn't false-positive on ordinary
   comments that happen to contain "ignore" (e.g. `// we ignore errors
   here` directly above a `#[test] fn ...` would currently mark that test
   ignored). Require an *exact* match against a small allow-list of forms
   rather than substring search.
2. **Real attribute parsing**, gated behind a small, additive grammar
   change: glyim's lexer/parser already must skip *something* for lines
   starting with `#` if any attribute-like syntax exists anywhere in the
   language (verify: `grep -rn "'#'" glyim-frontend/src/lexer.rs`). If `#`
   is not yet a recognized token at all, adding full attribute parsing is
   out of scope for this fix — ship improvement (1) alone and leave a
   tracked follow-up for real `#[ignore]` syntax once `glyim-frontend`
   gains attribute grammar (a frontend-team-scoped, larger project, and a
   dependency this test-harness fix should not block on).

### Step-by-step instructions

**Step 0.** `grep -n "pending_ignore_attr\|trimmed.starts_with" glyip/src/test_discovery.rs`
and re-read the full function (it's short, ~127 lines total per the earlier
`wc -l`).

**Step 1. Replace the substring check with an exact-match allow-list:**

```rust
// Before:
if trimmed.starts_with("//") && trimmed.contains("ignore") {
    pending_ignore_attr = true;
}

// After:
const IGNORE_MARKERS: &[&str] = &[
    "// #[ignore]",
    "//#[ignore]",
    "// ignore",
];
if IGNORE_MARKERS.iter().any(|m| trimmed.eq_ignore_ascii_case(m)) {
    pending_ignore_attr = true;
}
```

This still tolerates the two documented forms (`// #[ignore]` and `//
ignore`, per the report) and their common whitespace variants, but no
longer fires on unrelated prose comments.

**Step 2. Also support an inline reason**, matching real Rust's
`#[ignore = "reason"]` convention, since the codebase already uses that form
in `glyip/src/commands.rs` itself (`#[ignore = "native compiled-exec: ..."]`
— found in §1.9) for its *own* Rust-level tests, so mirroring the same
convention for glyim-level `// #[ignore = "..."]` comments is consistent
house style:

```rust
} else if let Some(rest) = trimmed.strip_prefix("// #[ignore").or_else(|| trimmed.strip_prefix("//#[ignore")) {
    if rest.trim_start().starts_with('=') || rest.trim_start().starts_with(']') {
        pending_ignore_attr = true;
    }
}
```
Fold this into Step 1's check rather than as a separate branch in the final
implementation — the above is illustrative of the pattern to support.

### Tests

```rust
#[test]
fn ordinary_comment_mentioning_ignore_does_not_mark_ignored() {
    let src = "// we ignore errors here\n#[test]\nfn t() {}\n";
    let tests = discover_tests(src);
    assert!(!tests[0].ignored);
}

#[test]
fn exact_ignore_marker_still_works() {
    let src = "// #[ignore]\n#[test]\nfn t() {}\n";
    assert!(discover_tests(src)[0].ignored);
}

#[test]
fn ignore_with_reason_comment_works() {
    let src = "// #[ignore = \"flaky on CI\"]\n#[test]\nfn t() {}\n";
    assert!(discover_tests(src)[0].ignored);
}
```

### Acceptance criteria

- [ ] False-positive prose comments no longer mark tests ignored.
- [ ] Both documented marker forms still work, plus reasoned form.
- [ ] A `KNOWN_GAPS.md` entry notes that real `#[ignore]` attribute parsing
      remains blocked on `glyim-frontend` attribute-syntax support and is
      tracked separately (do not silently drop this larger context).

---

## 2.2 `glyim_process_getppid` on Non-Unix

### Current state

`glyim-runtime/src/lib.rs::glyim_process_getppid` calls `libc::getppid()`
under a Unix-only path (confirmed: `unsafe { libc::getppid() as u32 }`) and
returns `0` unconditionally otherwise (POSIX-only stub).

### Target design

Windows has no direct `getppid` syscall, but the parent PID is obtainable
via `CreateToolhelp32Snapshot` + `Process32First`/`Process32Next` (walk the
process snapshot to find the current PID's entry, which carries
`th32ParentProcessID`), or via `NtQueryInformationProcess`
(`ProcessBasicInformation.InheritedFromUniqueProcessId`) if the `windows`
crate is already a dependency elsewhere (check `Cargo.toml`). The
Toolhelp32 approach needs no `ntdll` FFI and is the documented, supported
way to do this on Windows — prefer it.

### Step-by-step instructions

**Step 0.** `grep -n "cfg(unix)\|cfg(windows)\|^use " glyim-runtime/src/lib.rs`
near `glyim_process_getppid` to see the exact cfg-gating style already used
in this file (match it) and whether `windows`/`winapi` is already a dep.

**Step 1.** Add the Windows dependency if absent:
```rust
// glyim-runtime/Cargo.toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_System_Diagnostics_ToolHelp", "Win32_Foundation"] }
```

**Step 2.** Implement:

```rust
// glyim-runtime/src/lib.rs
#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_getppid() -> u32 {
    // SAFETY: getppid is a POSIX function that always succeeds and returns
    // the caller's parent PID with no side effects.
    unsafe { libc::getppid() as u32 }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_getppid() -> u32 {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    let current_pid = unsafe { GetCurrentProcessId() };
    // SAFETY: standard Toolhelp32 snapshot usage; the handle is checked
    // against INVALID_HANDLE_VALUE and always closed before returning.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return 0;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = 0u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == current_pid {
                    found = entry.th32ParentProcessID;
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(not(any(unix, windows)))]
#[unsafe(no_mangle)]
pub extern "C" fn glyim_process_getppid() -> u32 {
    // Genuinely no known primitive on this target family (e.g. wasm);
    // remains an honest 0 rather than a fabricated value.
    0
}
```

### Tests

```rust
#[cfg(windows)]
#[test]
fn getppid_matches_actual_parent_on_windows() {
    // Spawn a child process (e.g. `cmd /C exit 0` wrapped by a tiny test
    // helper binary that calls glyim_process_getppid and prints it), and
    // assert the printed value equals this test process's own PID
    // (`std::process::id()`).
}
```
Add this to the Windows CI job (see §9). On Unix, the existing behavior and
any existing test are unchanged.

### Acceptance criteria

- [ ] Windows returns the real parent PID, not `0`, when a parent process
      genuinely exists.
- [ ] Non-Unix-non-Windows targets keep the honest `0` stub (not silently
      claiming Windows support they don't have).
- [ ] New Windows CI test passes.

---

## 2.3 `glyim_fs_canonicalize` — Path Encoding

### Current state

`glyim-runtime/src/fs.rs::path_from_raw` is correct on Unix (raw bytes →
`OsStr::from_bytes`, exactly matching POSIX path semantics — no change
needed there). On Windows it: (1) tries UTF-8 first (glyim source/string
literals are UTF-8, so this is the common, correct case), then (2), if
UTF-8 decoding fails, **guesses** the bytes are little-endian UTF-16 and
converts blindly, with a comment admitting "this is a fallback; most
real-world Windows paths are UTF-16" — this guess has no way to distinguish
genuinely-UTF-16 bytes from merely-invalid-UTF-8 bytes that aren't UTF-16
either, and a mis-guess produces a garbage path silently. Then (3) an
"ultimate fallback" does lossy UTF-8 conversion, which for actually-UTF-16
input produces mojibake.

### Target design

Glyim's runtime ABI passes path bytes as `(ptr, len)` from Glyim string
values, and Glyim strings are — per the same file's own comment — always
UTF-8 at the language level (there is no UTF-16 string type in Glyim). The
"maybe UTF-16" branch is therefore handling a case that **should never
occur** for well-formed input from the Glyim compiler's own codegen; its
only real job is to be a *safe, honest* fallback for the (unlikely, but
possible via corrupted FFI/unsafe misuse) case of truly invalid UTF-8, not
to guess an unrelated encoding. Replace the UTF-16 guess with `OsString`'s
proper WTF-8-preserving conversion so **no bytes are silently
misinterpreted or lost**, matching how `std::ffi::OsStr`/`OsString` already
solve exactly this problem cross-platform.

### Step-by-step instructions

**Step 0.** `grep -n "fn path_from_raw" -A 50 glyim-runtime/src/fs.rs` and
re-confirm the exact current body (quoted above from the dump) before
editing.

**Step 1.** Replace the Windows branch:

```rust
#[cfg(windows)]
{
    // Glyim strings are UTF-8 at the language level (see module doc); the
    // common, correct case is that `bytes` IS UTF-8 (the caller's Glyim
    // `String`/`&str` bytes verbatim), which converts losslessly and
    // exactly via `OsString::from` for both Unicode and (via WTF-8-style
    // preservation in the FFI layer) any embedded surrogate-adjacent
    // sequences that a prior lossy round-trip may have produced. There is
    // no separate "maybe this is actually UTF-16" branch: Glyim never
    // produces UTF-16 path bytes, and guessing an unrelated encoding for
    // malformed input silently manufactures a wrong path rather than
    // reporting the real problem. Truly invalid UTF-8 is reported as an
    // error to the caller instead of guessed at.
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(PathBuf::from(s)),
        Err(_) => None, // caller sees FS_EIO (or the equivalent existing
                          // error code this function already returns for
                          // None — check call sites) instead of a silently
                          // wrong path.
    }
}
```

This is a **behavior change**: previously-silent wrong-path construction on
malformed input now correctly surfaces as an I/O error to the caller. This
is intentional and matches §0's "no silent wrong-answer fallback" rule —
verify every call site of `path_from_raw` already handles `None` by
returning an error code (per the earlier grep, every call site does: `match
unsafe { path_from_raw(...) } { Some(p) => p, None => return FS_EIO as
isize }` — Unix's null-pointer case already does exactly this, so Windows's
new "invalid UTF-8" case fits the same existing error contract with zero
call-site changes needed).

**Step 2.** Update the function's doc comment to remove the now-inaccurate
"we'll attempt to treat the bytes as UTF-16" language.

**Step 3.** If there turns out to be a *real* need to accept genuinely
non-UTF-8 Windows paths from **outside** the Glyim runtime (e.g.
`glyim_fs_canonicalize` on a path that already exists on disk with a
non-UTF-8 name created by some other program), that's a different, opt-in
API surface (e.g. a `glyim_fs_canonicalize_wide` variant taking explicit
UTF-16 input) — do not conflate it with this function's contract. Leave a
`KNOWN_GAPS.md` note if this broader need is real for the target user base;
do not implement it speculatively.

### Tests

```rust
#[cfg(windows)]
#[test]
fn valid_utf8_path_bytes_convert_correctly() {
    let s = "C:\\Users\\test\\file.txt";
    let p = unsafe { path_from_raw(s.as_ptr(), s.len()) };
    assert_eq!(p, Some(PathBuf::from(s)));
}

#[cfg(windows)]
#[test]
fn invalid_utf8_bytes_return_none_not_a_guessed_path() {
    let bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x00]; // invalid UTF-8
    let p = unsafe { path_from_raw(bytes.as_ptr(), bytes.len()) };
    assert_eq!(p, None);
}
```

### Acceptance criteria

- [ ] Valid UTF-8 (the only case Glyim itself ever produces) still works
      identically to before.
- [ ] Invalid UTF-8 now returns `None`/an error instead of a silently wrong
      guessed path.
- [ ] Doc comment no longer describes the removed UTF-16 heuristic.

---

## 2.4 `glyim_codegen::BytecodeBackend` — Opt-Level No-Op

### Current state

`glyim-codegen/src/lib.rs::BytecodeBackend::with_ty_ctx` constructs a
backend that ignores optimization level entirely (report: emits a warning
that "bytecode backend opt-level currently has no effect; reserved for
future peephole passes"). The struct has no `opt_level` field at all today
(confirmed: `string_table`, `fn_table`, `layout_provider`, `ty_ctx` only).

### Target design

Implement a small, self-contained peephole optimizer over the emitted
bytecode stream, gated by opt level, run as a post-pass after bytecode
emission (not interleaved with emission itself — keeps the emitter simple
and the optimizer independently testable). Minimum viable set (matches what
"peephole" means in the existing comment, and is safely bounded in scope):

1. **Redundant load elimination**: `OP_LOAD_LOCAL_ADDR n; OP_LOAD_LOCAL_ADDR n`
   (load the same address twice in a row with nothing observable between)
   → drop the second.
2. **Dead store elimination** for locals that are `StorageDead`'d
   immediately after a store with no intervening read.
3. **Constant folding** for adjacent `OP_PUSH_CONST a; OP_PUSH_CONST b;
   OP_ADD` (and similarly for other arithmetic ops) → a single
   `OP_PUSH_CONST (a+b)`.

### Step-by-step instructions

**Step 0.** `grep -n "OP_LOAD_LOCAL_ADDR\|OP_PUSH_CONST\|enum.*Op\|const OP_" glyim-codegen/src/lib.rs`
to get the authoritative opcode set and their exact byte encodings (the
peephole matcher must work on the real encoded byte stream or on a
pre-encoding instruction list — check whether this backend has an
intermediate `Vec<Instruction>` representation before final byte encoding;
if it emits raw bytes directly with no intermediate list, add one — a
`Vec<Instruction>` is much easier to peephole-optimize than a raw byte
stream and is a worthwhile small refactor here).

**Step 1.** Add the field and threading:

```rust
pub struct BytecodeBackend {
    string_table: RefCell<Vec<String>>,
    fn_table: RefCell<Vec<(FnDefId, Substitution)>>,
    layout_provider: Box<dyn LayoutProvider>,
    ty_ctx: Option<Arc<TyCtx>>,
    opt_level: u8,
}

impl BytecodeBackend {
    pub fn with_ty_ctx(ctx: Arc<TyCtx>, target: TargetInfo) -> Self {
        Self {
            string_table: RefCell::new(Vec::new()),
            fn_table: RefCell::new(Vec::new()),
            layout_provider: Box::new(GlyimLayoutProvider { ty_ctx: ctx.clone(), target: target.clone() }),
            ty_ctx: Some(ctx),
            opt_level: 0,
        }
    }

    /// Set the bytecode peephole-optimization level (0 = none, 1+ = enable
    /// the peephole pass suite below). Previously a documented no-op; now
    /// wired to `peephole_optimize`.
    pub fn with_opt_level(mut self, level: u8) -> Self {
        self.opt_level = level;
        self
    }
}
```

**Step 2.** Implement the peephole pass as a free function operating on
whatever the emitter's pre-final-encoding representation is (per Step 0):

```rust
// glyim-codegen/src/peephole.rs (new file)

/// Peephole-optimize a bytecode instruction stream. No-op at opt_level 0.
/// Purely local (fixed small window), so it's safe to run unconditionally
/// when opt_level > 0 without any global dataflow analysis.
pub(crate) fn peephole_optimize(instrs: &mut Vec<Instruction>, opt_level: u8) {
    if opt_level == 0 {
        return;
    }
    let mut changed = true;
    while changed {
        changed = false;
        changed |= fold_redundant_loads(instrs);
        changed |= fold_constant_arithmetic(instrs);
        changed |= eliminate_dead_stores(instrs);
    }
}

fn fold_redundant_loads(instrs: &mut Vec<Instruction>) -> bool {
    let mut i = 0;
    let mut changed = false;
    while i + 1 < instrs.len() {
        if let (Instruction::LoadLocalAddr(a), Instruction::LoadLocalAddr(b)) = (&instrs[i], &instrs[i + 1])
            && a == b
        {
            instrs.remove(i + 1);
            changed = true;
            continue;
        }
        i += 1;
    }
    changed
}

fn fold_constant_arithmetic(instrs: &mut Vec<Instruction>) -> bool {
    let mut i = 0;
    let mut changed = false;
    while i + 2 < instrs.len() {
        if let (Instruction::PushConst(a), Instruction::PushConst(b), Instruction::Add) =
            (&instrs[i], &instrs[i + 1], &instrs[i + 2])
        {
            let folded = Instruction::PushConst(a.wrapping_add(*b));
            instrs.splice(i..i + 3, [folded]);
            changed = true;
            continue;
        }
        i += 1;
    }
    changed
}

fn eliminate_dead_stores(instrs: &mut Vec<Instruction>) -> bool {
    let mut i = 0;
    let mut changed = false;
    while i + 1 < instrs.len() {
        if let (Instruction::StoreLocal(a), Instruction::StorageDead(b)) = (&instrs[i], &instrs[i + 1])
            && a == b
        {
            instrs.remove(i);
            changed = true;
            continue;
        }
        i += 1;
    }
    changed
}
```

Adapt `Instruction` variant names/payload types to whatever this backend's
real intermediate representation is named (Step 0's grep result); if no
intermediate representation exists yet, add a minimal one (a plain `enum
Instruction` + `Vec<Instruction> -> Vec<u8>` final-encode step) rather than
peepholing raw bytes, which is much more error-prone (risk of matching
opcode bytes that coincidentally appear inside an operand's encoded bytes).

**Step 3.** Call `peephole_optimize(&mut instrs, self.opt_level)` right
before final byte-encoding, in whatever function currently does that
encoding (found in Step 0).

**Step 4.** Wire `with_opt_level` from wherever `BytecodeBackend::with_ty_ctx`
is constructed by the pipeline driver, sourcing the value from the same
`-O`/`opt-level` CLI flag already used for `glyim-codegen-llvm`
(`grep -rn "opt_level\|with_lto" glyim-pipeline/src` to find that flag's
existing plumbing and mirror it for the bytecode backend).

### Tests

```rust
#[test]
fn opt_level_0_is_still_a_true_noop() {
    let mut instrs = vec![Instruction::PushConst(1), Instruction::PushConst(2), Instruction::Add];
    let before = instrs.clone();
    peephole_optimize(&mut instrs, 0);
    assert_eq!(instrs, before);
}

#[test]
fn constant_folding_at_opt_level_1() {
    let mut instrs = vec![Instruction::PushConst(1), Instruction::PushConst(2), Instruction::Add];
    peephole_optimize(&mut instrs, 1);
    assert_eq!(instrs, vec![Instruction::PushConst(3)]);
}

#[test]
fn redundant_load_elimination() {
    let mut instrs = vec![Instruction::LoadLocalAddr(0), Instruction::LoadLocalAddr(0), Instruction::Add];
    peephole_optimize(&mut instrs, 1);
    assert_eq!(instrs, vec![Instruction::LoadLocalAddr(0), Instruction::Add]);
}

#[test]
fn optimized_and_unoptimized_bytecode_produce_identical_interpreter_results() {
    // Compile a representative test program to bytecode at opt_level 0 and
    // opt_level 1, run BOTH through the interpreter/VM, and assert
    // identical observable results — the correctness contract that matters
    // most: optimization must never change behavior.
}
```

### Acceptance criteria

- [ ] `opt_level` field exists and is threaded from the CLI flag.
- [ ] `opt_level == 0` is provably a no-op (test above).
- [ ] `opt_level >= 1` measurably shrinks/simplifies bytecode for the three
      patterns above without changing program behavior.
- [ ] The "currently has no effect" warning is deleted.

---

## 2.5 `glyim_type::TyCtx::is_sized` — Unknown ADTs

### Current state

`glyim-type/src/ty_ctx.rs::is_sized` is otherwise fully correct (slices,
`dyn`, opaque types unsized; arrays/tuples/known-ADT fields recursively
checked). Only the `else` branch of `TyKind::Adt` — reached when
`self.adt_def(*adt_id)` is `None`, i.e. the `AdtId` isn't registered in this
`TyCtx` — conservatively returns `true` ("assume sized"), which the report
correctly flags as able to hide bugs: an actually-unsized type whose `AdtId`
lookup fails for an unrelated reason (a real bug elsewhere, e.g. a stale
`AdtId` from a different crate's `TyCtx`, or a registration-ordering bug)
would silently be treated as sized instead of surfacing the lookup failure.

### Target design

Distinguish "legitimately not yet known because this is being computed
mid-registration" (a real, expected case during ADT definition processing —
keep returning `true`/deferring here) from "a stale or foreign `AdtId` that
should never have reached this call" (a bug — should be loud). Since
`TyCtx::is_sized` has no way to tell these apart from inside itself (both
present identically as `adt_def(id) == None`), the fix is to make the
*caller* responsible: add a `debug_assert!` at the point where an unknown
`AdtId` is silently accepted, so **debug/test builds catch the bug loudly**
while release builds keep the current conservative (never-panics) fallback
— this is the standard "loud in dev, safe in prod" pattern already visible
elsewhere in this codebase's own `#[cfg(test)]`-only assertions
(`element_size_of` in `glyim-mir-interp`, for instance).

### Step-by-step instructions

**Step 0.** `grep -n "fn adt_def" glyim-type/src/ty_ctx.rs` to see whether
`adt_def` returning `None` is *expected* at any legitimate call time (e.g.
during incremental/out-of-order ADT registration across a crate graph) —
read its doc comment and 2-3 call sites to confirm before adding an
assertion that could be a false positive in a legitimate flow.

**Step 1.** If `None` is never legitimately expected at the point
`is_sized` calls it (the common case for a mature `TyCtx` used post-typeck,
which is when `is_sized` is actually consulted by the trait solver per its
own doc comment "used by the trait solver for `T: Sized` bounds"):

```rust
TyKind::Adt(adt_id, _) => {
    if let Some(adt_def) = self.adt_def(*adt_id) {
        adt_def.variants.iter().all(|v| v.fields.iter().all(|f| self.is_sized(f.ty)))
    } else {
        // An `AdtId` with no registered definition reaching a `Sized`
        // check is unexpected once type-checking has completed (every
        // ADT referenced in checked code must have been registered by
        // then) — in debug/test builds, surface this loudly instead of
        // silently guessing `true`, since a silent wrong `Sized` answer
        // can propagate into a miscompilation far from its root cause.
        // Release builds keep the previous conservative default so an
        // unexpected-but-survivable case never panics in production.
        debug_assert!(
            false,
            "is_sized: unregistered AdtId {adt_id:?} — this indicates a \
             stale/foreign Ty or a registration-ordering bug; treating as \
             Sized only because this is a release build"
        );
        true
    }
}
```

**Step 2.** If Step 0 instead reveals `None` IS legitimately reachable
during a specific known phase (e.g. mid-registration recursive
`Sized`-ness computation for mutually-recursive ADTs being defined in the
same batch), narrow the assertion to exclude that phase specifically — e.g.
by threading a `during_registration: bool` context flag, or by checking a
"currently being defined" set already used elsewhere for cycle detection in
this same file (`grep -n "evaluating\|in_progress\|cycle" glyim-type/src/ty_ctx.rs`)
— reuse that existing cycle-tracking machinery rather than inventing a new
mechanism, if one already exists (the `auto_trait.rs` code we already read
has an `evaluating` cycle-guard parameter — check whether `is_sized` should
gain the same style of guard for genuinely-recursive-during-registration
ADTs, which is a real, separate correctness question from the "unknown
AdtId" one and worth flagging even if out of this section's direct scope).

### Tests

```rust
#[test]
#[should_panic(expected = "unregistered AdtId")]
#[cfg(debug_assertions)]
fn is_sized_on_unregistered_adt_panics_in_debug() {
    let ctx = TyCtx::empty_for_test();
    let fake_id = AdtId::from_raw(999_999); // never registered
    let ty = ctx.mk_adt_ty(fake_id, Substitution::empty());
    ctx.is_sized(ty); // should hit the debug_assert
}

#[test]
fn is_sized_on_registered_recursive_adt_is_correct() {
    // struct Node { next: Option<Box<Node>> } — Sized (Box breaks the
    // recursion); assert `is_sized` returns true and does not stack
    // overflow.
}
```

### Acceptance criteria

- [ ] Unknown `AdtId` reaching `is_sized` now panics in debug/test builds
      (per the codebase's existing dev-loud/prod-safe convention) instead
      of silently returning `true` everywhere.
- [ ] Release behavior is unchanged (still `true`, never panics).
- [ ] No existing test suite run under `debug_assertions` newly fails —
      if one does, that's a **real bug this change found**; fix the actual
      registration-ordering issue rather than loosening the assertion.

---

## 2.6 `glyim_solve` — HRTB `can_coerce` via Structural Equality

### Current state

`glyim-solve/src/hrtb.rs::check_hrtb`'s `Predicate::Coerce` arm is already
correct as designed: it uses `ty_struct_eq` (structural `TyKind` comparison)
specifically *because* HRTB placeholder instantiation re-interns types, so
identity (`Ty ==`) comparison would spuriously fail for structurally
identical types. The report's own characterization — "correct but may be
incomplete for recursive types" — is the only real gap: `ty_struct_eq`
recurses into compound `TyKind`s (`Ref`, `Ptr`, `Slice`, `Array`, `Tuple`,
generic args, trait refs, outlives predicates — confirmed by the earlier
grep of its match arms) with **no cycle/depth guard**, so a pathological
self-referential type shape (however rare given normal ADTs go through
`Box`/`Adt` indirection, which `ty_struct_eq` doesn't appear to recurse
into by field per the grep) could in principle cause unbounded recursion if
such a `Ty` shape is ever constructible (e.g. via a malformed/adversarial
generic-arg substitution during HRTB instantiation itself).

### Step-by-step instructions

**Step 0.** `grep -n "fn ty_struct_eq" -A 60 glyim-solve/src/hrtb.rs` to
confirm the full match (does it recurse into `TyKind::Adt`'s fields, or
just compare `AdtId`+`Substitution` args structurally without unfolding
fields? — the earlier grep output shows it recursing into `GenericArg::Ty`
inside substitutions, which for an `Adt(id, substs)` **does** recursively
compare the generic *arguments*, not the field types themselves, so a
directly-self-referential ADT (`struct Node<T> { next: Node<T> }`, which
wouldn't even type-check/have finite size without indirection) isn't the
actual risk — the real risk is a generic substitution chain like `Foo<Foo<Foo<...>>>`
arbitrarily deep, which HRTB placeholder instantiation could in principle
produce from an adversarial or buggy higher-ranked bound).

**Step 1.** Add a depth guard, matching the depth-limit convention already
used elsewhere in this codebase (`MAX_EVAL_DEPTH` in
`glyim-const-eval` — reuse that same constant-and-error-on-exceed pattern
for consistency rather than inventing a new limit/style):

```rust
const MAX_STRUCT_EQ_DEPTH: u32 = 256;

fn ty_struct_eq(a: Ty, b: Ty, ctx: &TyCtx) -> bool {
    ty_struct_eq_bounded(a, b, ctx, 0)
}

fn ty_struct_eq_bounded(a: Ty, b: Ty, ctx: &TyCtx, depth: u32) -> bool {
    if depth >= MAX_STRUCT_EQ_DEPTH {
        // Treat pathologically deep/cyclic type shapes as NOT structurally
        // equal rather than stack-overflowing: this makes `check_hrtb`
        // fall through to `Ambiguous` (never falsely `Proven`), which is
        // the same "when in doubt, don't discharge the obligation" policy
        // this function already applies to every other open/unresolved
        // case in `check_hrtb`.
        return false;
    }
    match (ctx.ty_kind(a), ctx.ty_kind(b)) {
        // <every existing match arm, with every recursive call rewritten
        //  from `ty_struct_eq(x, y, ctx)` to
        //  `ty_struct_eq_bounded(x, y, ctx, depth + 1)`>
        ...
    }
}
```

Every internal recursive call site inside the existing match arms must be
updated to pass `depth + 1` through `ty_struct_eq_bounded` — do this
mechanically (find-and-replace `ty_struct_eq(` → `ty_struct_eq_bounded(...,
depth + 1)` within the function body only, not at its two call sites in
`check_hrtb`, which should still call the public `ty_struct_eq` wrapper
entry point).

### Tests

```rust
#[test]
fn deeply_nested_but_finite_generic_args_still_compare_correctly() {
    // Vec<Vec<Vec<Vec<i32>>>> vs the same shape — well within
    // MAX_STRUCT_EQ_DEPTH — must still return true, proving the depth
    // counter doesn't false-negative on ordinary deep-but-finite types.
}

#[test]
fn pathologically_deep_substitution_does_not_stack_overflow() {
    // Construct (via direct TyCtxMut calls, not real source) a
    // substitution chain deeper than MAX_STRUCT_EQ_DEPTH and assert
    // `ty_struct_eq` returns `false` (not a panic/overflow) — this is the
    // regression test that actually exercises the fix; without it, the
    // fix is unverified.
}
```

### Acceptance criteria

- [ ] `ty_struct_eq` is depth-bounded, matching the existing
      `MAX_EVAL_DEPTH` convention used elsewhere.
- [ ] Ordinary deep-but-finite generic nesting still compares correctly.
- [ ] A pathological depth returns `false` (→ `Ambiguous` in `check_hrtb`),
      never panics/overflows the stack.

---

## 2.7 Auto-Trait Computation for `Projection` Types

### Current state

`glyim-type/src/auto_trait.rs::compute_auto_traits_for_kind`'s
`TyKind::Opaque(_, _) | TyKind::Projection(_)` arm already correctly
resolves `Opaque` (`impl Trait`) types by recursing into the registered
hidden type via `lookup.opaque_hidden_ty`. `Projection` (an unnormalized
generic associated-type reference, e.g. `<T as Iterator>::Item`) falls
through the same arm's guard (`if let TyKind::Opaque(id, _) = ...` doesn't
match a `Projection`) straight to `AutoTraitFlags::empty()`, exactly as the
doc comment states: normalization requires the trait solver, which this
function doesn't have access to.

### Target design

Normalize the projection to its concrete type **before** computing
auto-traits, using the trait solver's existing projection-normalization
entry point (the same one typeck itself uses to resolve `<T as
Trait>::Assoc` during type-checking) — do not re-derive normalization logic
inside `auto_trait.rs`.

### Step-by-step instructions

**Step 0.** `grep -rn "fn normalize_projection\|fn normalize\b" glyim-solve/src glyim-type/src`
to find the canonical normalization entry point (likely in `glyim-solve`,
given `hrtb.rs` lives there and typeck's projection resolution almost
certainly routes through the same solver crate).

**Step 1.** Check what `compute_auto_traits`/`compute_auto_traits_for_kind`
currently receive as context (`lookup: &dyn TypeLookup`, `registry`,
`adt_reprs`, `cache`, `evaluating` — per the signatures grepped earlier).
Normalizing a projection needs a full trait solver + inference table, which
this function's current signature doesn't carry. Two options:

- **(A) Thread solver access through `TypeLookup`.** Extend the
  `TypeLookup` trait (used for the existing `Opaque` case) with a new
  method `fn normalize_projection(&self, proj: ProjectionTy) -> Option<Ty>`,
  implemented by whichever concrete type already implements `TypeLookup`
  for real compilation (find it: `grep -rn "impl TypeLookup for"
  glyim-type/src glyim-typeck/src`) by delegating to the solver it already
  has access to at that call site.
- **(B) Leave unresolvable at this layer, cache `Ambiguous` instead of
  `empty()`.** If (A)'s plumbing is too invasive for this pass (auto-trait
  computation may run in contexts with no live solver, e.g. very early
  incremental queries), at minimum stop asserting a **wrong, confident
  answer** (`empty()` == "definitely implements no auto traits", which is
  false whenever the normalized type in fact does) — instead return a
  distinguishable "unknown, don't cache" flags value the caller can react to
  by deferring rather than baking in a false negative.

Prefer (A); it directly fixes the report's stated impact ("generic
associated types may appear not to implement auto-traits, causing false
negatives") rather than just making the failure mode more honest.

**Step 2 (implementing A).**

```rust
// glyim-type/src/auto_trait.rs
TyKind::Opaque(_, _) | TyKind::Projection(_) => {
    if let TyKind::Opaque(id, _) = lookup.ty_kind(ty)
        && let Some(hidden) = lookup.opaque_hidden_ty(*id)
    {
        return compute_auto_traits_recursive(hidden, lookup, registry, adt_reprs, cache, evaluating);
    }
    if let TyKind::Projection(proj) = lookup.ty_kind(ty)
        && let Some(normalized) = lookup.normalize_projection(*proj)
    {
        // Guard against normalization producing the SAME unnormalized
        // projection back (a solver that can't make progress yet, e.g. a
        // generic `T::Item` with no concrete `T`) — without this check,
        // `compute_auto_traits_recursive` would infinitely recurse. Reuse
        // the `evaluating` cycle-guard the recursive function already
        // threads through (per its existing signature) exactly as it must
        // already do for ordinary recursive ADTs.
        if normalized != ty {
            return compute_auto_traits_recursive(normalized, lookup, registry, adt_reprs, cache, evaluating);
        }
    }
    AutoTraitFlags::empty()
}
```

**Step 3.** Add `normalize_projection` to the `TypeLookup` trait definition
(`grep -n "trait TypeLookup" glyim-type/src`) and implement it on the
concrete production implementor found in Step 1, delegating to
`glyim-solve`'s normalization entry point from Step 0. For any *other*
`TypeLookup` implementor used only in tests/limited contexts with no solver
available, implement it as `fn normalize_projection(&self, _: ProjectionTy)
-> Option<Ty> { None }` (safe default: falls through to the existing
`empty()` behavior, no regression for those contexts).

### Tests

```rust
#[test]
fn projection_normalizing_to_send_adt_is_send() {
    // trait Iterator { type Item; } impl Iterator for Foo { type Item = Bar; }
    // where Bar: Send. Compute auto-traits for the *projection* type
    // `<Foo as Iterator>::Item` and assert AutoTraitFlags::SEND is set —
    // this is the exact false-negative the report describes, now fixed.
}

#[test]
fn unnormalizable_projection_still_returns_empty_not_infinite_loop() {
    // A generic `T::Item` with no concrete `T` in scope: normalize_projection
    // returns None (or the same projection back) — assert this returns
    // empty flags promptly, not a stack overflow/infinite recursion.
}
```

### Acceptance criteria

- [ ] `<Concrete as Trait>::Assoc` projections now report accurate
      auto-trait flags via normalization, not a blanket false `empty()`.
- [ ] Non-normalizable projections still safely degrade to `empty()`, with
      an explicit cycle guard (test above) proving no infinite recursion.
- [ ] `TypeLookup` implementors outside the main compilation path (tests,
      limited contexts) get a safe default and are not forced to wire a
      real solver.

---
