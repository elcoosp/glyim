#!/usr/bin/env bash
# ============================================================================
# GLYIM COMPREHENSIVE PROBE — writes everything to a log file.
#
# Usage:
#   chmod +x probe.sh && ./probe.sh
#
# Output:
#   ./glyim-probe-<UTC-timestamp>.log        full log (view with: less, grep, etc.)
#   ./glyim-probe-<UTC-timestamp>.summary    short summary printed to terminal
#
# The terminal sees only the summary.  Everything else lands in the log.
# ============================================================================

set -uo pipefail

cd "$(dirname "$0")" || exit 1
ROOT="$PWD"

TS=$(date -u +%Y%m%dT%H%M%SZ)
LOG="$ROOT/glyim-probe-$TS.log"
SUMMARY="$ROOT/glyim-probe-$TS.summary"

# -- Redirect ALL stdout+stderr of the body into the log; keep the terminal
#    only for the summary we write at the end. -------------------------------
exec 3>&1                    # fd 3 = original terminal stdout
exec >"$LOG" 2>&1            # fd 1,2 = log file

echo "glyim probe started: $(date -u +%FT%TZ)"
echo "log file: $LOG"
echo "root:     $ROOT"
echo

HOST_TRIPLE=$(rustc -vV 2>/dev/null | awk '/^host:/{print $2}')
echo "host triple: $HOST_TRIPLE"
echo

# Summary accumulator --------------------------------------------------------
SUMMARY_DATA=""
sum() { SUMMARY_DATA+="$1"$'\n'; }

hr() { printf '\n=======================================================================\n%s\n=======================================================================\n' "$1"; }

# ============================================================================
hr "SECTION A — Repo state"
git log --oneline -20 2>/dev/null || true
echo "--- git status ---"
git status --short 2>/dev/null | head -40 || true
echo "--- rust-toolchain.toml ---"
cat rust-toolchain.toml 2>/dev/null || echo "(none)"
echo "--- Cargo.toml ---"
cat Cargo.toml

# ============================================================================
hr "SECTION B — Language spec, contracts, README"
head -300 README.md 2>/dev/null || true
echo "--- TECHSTACK ---"
cat docs/TECHSTACK.md 2>/dev/null || true
echo "--- docs/specs/v0.1.0.md ---"
cat docs/specs/v0.1.0.md 2>/dev/null || true
echo "--- docs/contracts/v0.1.0.md: headings ---"
grep -n "^#" docs/contracts/v0.1.0.md 2>/dev/null | head -200 || true
echo "--- docs/contracts/v0.1.0.md: first 400 lines ---"
head -400 docs/contracts/v0.1.0.md 2>/dev/null || true

# ============================================================================
hr "SECTION C — Test suite ground truth (counts)"
echo "--- per-crate inline #[test] count ---"
TOTAL_TESTS=0
for d in crates/*/; do
  name=$(basename "$d"); src="$d/src"
  [ -d "$src" ] || continue
  tf=$(grep -rn "#\[test\]" "$src" 2>/dev/null | wc -l | tr -d ' ')
  TOTAL_TESTS=$(( TOTAL_TESTS + tf ))
  printf '%5s  %s\n' "$tf" "$name"
done | sort -rn
echo "--- workspace total ---"
grep -rn "#\[test\]" crates tools --include='*.rs' 2>/dev/null | wc -l
sum "Tests (inline #[test]):          $TOTAL_TESTS"
echo "--- real #[ignore] attributes ---"
IGNORED=$(grep -rn "^\s*#\[ignore" crates tools --include='*.rs' 2>/dev/null | wc -l | tr -d ' ')
echo "$IGNORED attributes found"
sum "Tests marked #[ignore]:          $IGNORED"

# ============================================================================
hr "SECTION D — Cargo.lock / dependency surface"
PKGS=$(grep -c "^\[\[package\]\]" Cargo.lock 2>/dev/null)
echo "total packages in Cargo.lock: $PKGS"
sum "Cargo.lock packages:             $PKGS"
echo "--- sensitive deps present ---"
grep -E '^name = "(reqwest|hyper|native-tls|openssl|rustls|ring|tokio|libloading|sha2|rand|semver|lasso|rowan|inkwell|regalloc2|miette|petgraph|dashmap|lsp-types)"' Cargo.lock 2>/dev/null | sort -u
echo "--- deny.toml / audit config ---"
if [ -f deny.toml ]; then echo "deny.toml: present"; else echo "deny.toml: MISSING"; sum "deny.toml:                       MISSING"; fi
if [ -f .cargo/audit.toml ]; then echo "audit.toml: present"; else echo "audit.toml: MISSING"; sum "audit.toml:                      MISSING"; fi

# ============================================================================
hr "SECTION E — Build + clippy warning surface"
echo "--- cargo check --workspace --all-targets ---"
cargo check --workspace --all-targets 2>&1 | tail -20
echo "--- cargo build: repeated warning summary ---"
cargo build --workspace 2>&1 | grep -E "^warning" | sort | uniq -c | sort -rn | head -20
echo "--- clippy: repeated warning summary ---"
cargo clippy --workspace --all-targets 2>&1 | grep -E "^warning" | sort | uniq -c | sort -rn | head -30
echo "--- missing documentation: per crate ---"
cargo doc --workspace --no-deps 2>&1 | grep "missing documentation" | sed 's/.*crates\///' | cut -d/ -f1 | sort | uniq -c | sort -rn | head -20
MISSING_DOCS=$(cargo doc --workspace --no-deps 2>&1 | grep -c "missing documentation")
echo "total missing docs: $MISSING_DOCS"
sum "Missing docs (warn):             $MISSING_DOCS"

# ============================================================================
hr "SECTION F — Grammar: SyntaxKind enum, keywords"
SK_FILE=$(grep -rl "pub enum SyntaxKind" crates/glyim-syntax/src 2>/dev/null | head -1)
echo "--- SyntaxKind in: $SK_FILE ---"
[ -n "$SK_FILE" ] && awk '/pub enum SyntaxKind/,/^}/' "$SK_FILE"
echo "--- keyword tokens ---"
grep -rhoE "Kw[A-Z][a-zA-Z]+" crates/glyim-syntax/src --include='*.rs' 2>/dev/null | sort -u
KW_COUNT=$(grep -rhoE "Kw[A-Z][a-zA-Z]+" crates/glyim-syntax/src --include='*.rs' 2>/dev/null | sort -u | wc -l | tr -d ' ')
sum "Language keywords (Kw tokens):   $KW_COUNT"

# ============================================================================
hr "SECTION G — Fixture corpus inventory"
echo "--- all fixture roots ---"
find crates tests -type d \( -name 'compile-pass' -o -name 'compile-fail' -o -name 'run-pass' -o -name 'run-fail' -o -name 'ui' -o -name 'mir' -o -name 'runtime' \) 2>/dev/null | sort
echo
echo "--- count per root ---"
for d in $(find crates tests -type d \( -name 'compile-pass' -o -name 'compile-fail' -o -name 'run-pass' -o -name 'run-fail' -o -name 'ui' -o -name 'mir' -o -name 'runtime' \) 2>/dev/null | sort); do
  n=$(find "$d" -maxdepth 3 -name '*.g' 2>/dev/null | wc -l | tr -d ' ')
  echo "$n  $d"
done
echo
echo "--- every fixture: file / mode / exit / stdout / stderr / only-target ---"
RUNPASS_STDOUT=0
RUNPASS_EXIT=0
RUNPASS_TOTAL=0
for f in $(find crates tests -name '*.g' -not -path './target/*' 2>/dev/null | sort); do
  mode=$(grep -m1 "test-mode:" "$f" 2>/dev/null | sed 's/.*test-mode: *//;s/ *$//')
  exit_code=$(grep -m1 "exit-code:" "$f" 2>/dev/null | sed 's/.*exit-code: *//;s/ *$//')
  stdout=$(grep -c "check-stdout:" "$f" 2>/dev/null)
  stderr=$(grep -c "check-stderr:" "$f" 2>/dev/null)
  only=$(grep -m1 "only-target:" "$f" 2>/dev/null | sed 's/.*only-target: *//;s/ *$//')
  if [ "$mode" = "run-pass" ]; then
    RUNPASS_TOTAL=$(( RUNPASS_TOTAL + 1 ))
    if [ "$stdout" -gt 0 ]; then RUNPASS_STDOUT=$(( RUNPASS_STDOUT + 1 )); fi
    if [ -n "$exit_code" ]; then RUNPASS_EXIT=$(( RUNPASS_EXIT + 1 )); fi
  fi
  printf '%-72s mode=%-14s exit=%-4s stdout=%s stderr=%s only=%s\n' \
    "${f#./}" "${mode:-—}" "${exit_code:-—}" "$stdout" "$stderr" "${only:-—}"
done
sum "run-pass fixtures:               $RUNPASS_TOTAL"
sum "  … that check-stdout:           $RUNPASS_STDOUT"
sum "  … that only check exit-code:   $RUNPASS_EXIT"

# ============================================================================
hr "SECTION H — Stdlib source"
for f in \
  crates/glyim-lang-alloc/lib/vec.g \
  crates/glyim-lang-alloc/lib/string.g \
  crates/glyim-lang-alloc/lib/boxed.g \
  crates/glyim-lang-alloc/lib/rc.g \
  crates/glyim-lang-alloc/lib/raw_vec.g \
  crates/glyim-lang-alloc/lib/alloc.g \
  crates/glyim-lang-core/lib/option.g \
  crates/glyim-lang-core/lib/result.g \
  crates/glyim-lang-core/lib/iter.g \
  crates/glyim-lang-core/lib/ops.g \
  crates/glyim-lang-core/lib/slice.g \
  crates/glyim-lang-core/lib/str.g \
  crates/glyim-lang-core/lib/future.g \
  crates/glyim-lang-core/lib/panic.g \
  crates/glyim-lang-std/lib/fs.g \
  crates/glyim-lang-std/lib/io.g \
  crates/glyim-lang-std/lib/thread.g \
  crates/glyim-lang-std/lib/sync.g \
  crates/glyim-lang-std/lib/env.g \
  crates/glyim-lang-std/lib/net.g \
  crates/glyim-lang-std/lib/process.g
do
  if [ -f "$f" ]; then
    echo "===== $f ($(wc -l <"$f") lines) ====="
    cat "$f"
    echo
  fi
done

# ============================================================================
hr "SECTION I — glyip test command"
echo "--- commands.rs (test region) ---"
sed -n '200,400p' crates/glyip/src/commands.rs 2>/dev/null
echo "--- test_discovery.rs ---"
cat crates/glyip/src/test_discovery.rs 2>/dev/null
echo "--- test_cmd.rs ---"
cat crates/glyip/src/tests/test_cmd.rs 2>/dev/null

# ============================================================================
hr "SECTION J — LSP capabilities + handlers"
echo "--- handler.rs ---"
cat crates/glyim-lsp/src/handler.rs 2>/dev/null
echo "--- all Request/Notification matches ---"
grep -rn "Request::\|Notification::\|ServerRequest" crates/glyim-lsp/src --include='*.rs' 2>/dev/null | head -60
echo "--- integration tests ---"
cat crates/glyim-lsp/src/tests/lsp_integration_tests.rs 2>/dev/null

# ============================================================================
hr "SECTION K — Fuzz / mutation / property testing"
echo "--- fuzz target dirs ---"
find . -path ./target -prune -o -name 'fuzz_targets' -type d -print 2>/dev/null
find . -path ./target -prune -o -name 'fuzz' -type d -print 2>/dev/null | head -10
echo "--- mutants state ---"
ls -la mutants.out*/ 2>/dev/null || true
wc -l mutants.out/caught.txt mutants.out/missed.txt 2>/dev/null || true
echo "--- proptest/quickcheck/arbitrary usage ---"
grep -rn "proptest\|quickcheck\|arbitrary::" crates --include='*.rs' 2>/dev/null | head -20
echo "--- property/mod.rs ---"
cat crates/glyim-test/src/property/mod.rs 2>/dev/null

# ============================================================================
hr "SECTION L — Production panic surface, per crate"
echo "--- per-crate count of panic!/unreachable!/bug! NOT under src/tests/ ---"
TOTAL_PANICS=0
for d in crates/*/; do
  name=$(basename "$d"); src="$d/src"
  [ -d "$src" ] || continue
  n=$(grep -rn "panic!(\|unreachable!(\|bug!(" "$src" --include='*.rs' 2>/dev/null | grep -v '/tests/' | wc -l | tr -d ' ')
  TOTAL_PANICS=$(( TOTAL_PANICS + n ))
  printf '%5s  %s\n' "$n" "$name"
done | sort -rn
sum "Production panics (non-test):    $TOTAL_PANICS"
echo
echo "--- full listing in the top crates ---"
for c in glyim-codegen-llvm glyim-hir glyim-opt glyim-type glyim-solve glyim-borrowck glyim-lower glyim-mir; do
  echo "=== $c ==="
  grep -rn "panic!(\|unreachable!(\|bug!(" "crates/$c/src" --include='*.rs' 2>/dev/null | grep -v '/tests/' || true
done

# ============================================================================
hr "SECTION M — docs/issues backlog"
for f in docs/issues/*.md; do
  [ -f "$f" ] || continue
  echo "===== $f ====="
  cat "$f"
  echo
done
echo "--- docs/design/ ---"
find docs/design -type f 2>/dev/null | sort
for f in docs/design/*.md; do
  [ -f "$f" ] || continue
  echo "===== $f ====="
  head -200 "$f"
done
echo "--- docs/agent-kit/ ---"
find docs/agent-kit -type f 2>/dev/null | sort

# ============================================================================
hr "SECTION N — Actual test execution"
if command -v cargo-nextest >/dev/null 2>&1; then
  echo "--- nextest list count ---"
  cargo nextest list --workspace 2>&1 | tail -10
  echo "--- running full suite (this is the long one) ---"
  cargo nextest run --workspace --no-fail-fast 2>&1 | tee /tmp/glyim-nextest-raw.log | tail -60
  echo "--- nextest final summary lines ---"
  grep -E "Summary|FAIL|TIMEOUT|Starting" /tmp/glyim-nextest-raw.log | tail -40
  PASSED=$(grep -oE "[0-9]+ passed" /tmp/glyim-nextest-raw.log | tail -1)
  FAILED=$(grep -oE "[0-9]+ failed" /tmp/glyim-nextest-raw.log | tail -1)
  sum "nextest result:                  ${PASSED:-?} / ${FAILED:-?}"
else
  echo "(nextest not installed — using cargo test --no-run to at least count binaries)"
  cargo test --workspace --no-run 2>&1 | tail -20
  sum "nextest:                         NOT INSTALLED"
fi

# ============================================================================
hr "SECTION O — End-to-end probe battery"
cargo build -p glyim-cli 2>&1 | tail -3
GLYIM="$ROOT/target/debug/glyim-cli"
if [ ! -x "$GLYIM" ]; then
  echo "glyim-cli not built — skipping probe battery"
  sum "Probe battery:                   SKIPPED (glyim-cli not built)"
else
  W=$(mktemp -d)
  cd "$W"
  echo "probe workdir: $W"
  printf '%-30s | %-7s | %-7s | %-5s | %s\n' "PROBE" "COMPILE" "RUN" "EXIT" "NOTE"
  printf -- '-------------------------------+---------+---------+-------+------\n'

  PROBE_PASS=0; PROBE_DIFF=0; PROBE_CFAIL=0; PROBE_TOTAL=0
  p() {
    local n="$1" xcode="$2" xout="$3" src="$4"
    PROBE_TOTAL=$(( PROBE_TOTAL + 1 ))
    printf '%s\n' "$src" > "$n.g"
    local out c compile_ok="FAIL" run_ok="-" ec="-" note=""
    out=$("$GLYIM" "$n.g" --emit=exec --target="$HOST_TRIPLE" -o "$n.bin" 2>&1)
    c=$?
    if [ $c -eq 0 ] && [ -x "$n.bin" ]; then
      compile_ok="OK"
      local so
      so=$("$n.bin" 2>&1); ec=$?
      if [ "$ec" = "$xcode" ] && { [ -z "$xout" ] || [ "$so" = "$xout" ]; }; then
        run_ok="PASS"; PROBE_PASS=$(( PROBE_PASS + 1 ))
      else
        run_ok="DIFF"; PROBE_DIFF=$(( PROBE_DIFF + 1 ))
        note="got exit=$ec stdout=$(printf '%s' "$so" | head -c 50 | tr '\n' '|')"
      fi
    else
      PROBE_CFAIL=$(( PROBE_CFAIL + 1 ))
      note=$(printf '%s' "$out" | head -1 | head -c 90)
    fi
    printf '%-30s | %-7s | %-7s | %-5s | %s\n' "$n" "$compile_ok" "$run_ok" "$ec" "$note"
  }

  p empty             0 ""    'fn main() {}'
  p println           0 "hi"  'fn main() { println("hi"); }'
  p println_fmt       0 "n: 42" 'fn main() { println("n: {}", 42); }'
  p arith             42 ""   'fn main() -> i32 { 40 + 2 }'
  p let_chain         10 ""   'fn main() -> i32 { let a = 3; let b = 4; let c = 3; a + b + c }'
  p if_else           7 ""    'fn main() -> i32 { if 1 < 2 { 7 } else { 0 } }'
  p while_loop        10 ""   'fn main() -> i32 { let mut i = 0; let mut s = 0; while i < 5 { s = s + i; i = i + 1; } s }'
  p loop_break        3 ""    'fn main() -> i32 { let mut i = 0; loop { if i == 3 { break; } i = i + 1; } i }'
  p for_range         3 ""    'fn main() -> i32 { let mut s = 0; for i in 0..3 { s = s + 1; } s }'
  p nested_call       10 ""   'fn d(x: i32) -> i32 { x + x } fn a(x: i32, y: i32) -> i32 { x + y } fn main() -> i32 { a(d(3), 4) }'
  p recursion         120 ""  'fn f(n: i32) -> i32 { if n <= 1 { 1 } else { n * f(n - 1) } } fn main() -> i32 { f(5) }'
  p generic_identity  42 ""   'fn id<T>(x: T) -> T { x } fn main() -> i32 { id(42) }'
  p turbofish         42 ""   'fn id<T>(x: T) -> T { x } fn main() -> i32 { id::<i32>(42) }'
  p struct_field      20 ""   'struct P { x: i32, y: i32 } fn main() -> i32 { let p = P { x: 10, y: 20 }; p.y }'
  p struct_method     7 ""    'struct S { v: i32 } impl S { fn get(&self) -> i32 { self.v } } fn main() -> i32 { let s = S { v: 7 }; s.get() }'
  p enum_match        42 ""   'enum E { A(i32), B } fn main() -> i32 { let e = E::A(42); match e { E::A(v) => v, E::B => 0 } }'
  p option_match      5 ""    'enum Opt<T> { Some(T), None } fn main() -> i32 { let o = Opt::Some(5); match o { Opt::Some(v) => v, Opt::None => 0 } }'
  p trait_static      3 ""    'trait T { fn f(&self) -> i32; } struct S; impl T for S { fn f(&self) -> i32 { 3 } } fn main() -> i32 { let s = S; s.f() }'
  p trait_dyn         4 ""    'trait T { fn f(&self) -> i32; } struct S; impl T for S { fn f(&self) -> i32 { 4 } } fn main() -> i32 { let s = S; let t: &dyn T = &s; t.f() }'
  p closure_direct    5 ""    'fn main() -> i32 { let f = |x: i32| x + 1; f(4) }'
  p closure_capture   7 ""    'fn main() -> i32 { let y = 3; let f = |x: i32| x + y; f(4) }'
  p string_println    0 "abc" 'fn main() { println("abc"); }'
  p vec_push_len      3 ""    'fn main() -> i32 { let mut v = Vec::new(); v.push(1); v.push(2); v.push(3); v.len() }'
  p vec_pop           1 ""    'fn main() -> i32 { let mut v = Vec::new(); v.push(1); v.push(2); let _ = v.pop(); v.len() }'
  p box_alloc         42 ""   'fn main() -> i32 { let b = Box::new(42); *b }'
  p drop_order        0 "drop 2
drop 1" 'struct D { id: i32 } impl Drop for D { fn drop(&mut self) { println("drop {}", self.id); } } fn main() { let _a = D { id: 1 }; let _b = D { id: 2 }; }'
  p result_question   42 ""   'enum R<T, E> { Ok(T), Err(E) } fn g() -> R<i32, i32> { R::Ok(42) } fn f() -> R<i32, i32> { let x = g()?; R::Ok(x) } fn main() -> i32 { match f() { R::Ok(v) => v, R::Err(_) => 0 } }'

  mkdir -p mf
  printf 'mod helper;\nfn main() -> i32 { helper::add(40, 2) }\n' > mf/main.g
  printf 'pub fn add(a: i32, b: i32) -> i32 { a + b }\n' > mf/helper.g
  PROBE_TOTAL=$(( PROBE_TOTAL + 1 ))
  if "$GLYIM" mf/main.g --emit=exec --target="$HOST_TRIPLE" -o mf.bin >/dev/null 2>&1 && [ -x mf.bin ]; then
    ./mf.bin; ec=$?
    if [ "$ec" = "42" ]; then
      printf '%-30s | %-7s | %-7s | %-5s | %s\n' "multi_file_mod" "OK" "PASS" "$ec" ""
      PROBE_PASS=$(( PROBE_PASS + 1 ))
    else
      printf '%-30s | %-7s | %-7s | %-5s | %s\n' "multi_file_mod" "OK" "DIFF" "$ec" ""
      PROBE_DIFF=$(( PROBE_DIFF + 1 ))
    fi
  else
    printf '%-30s | %-7s | %-7s | %-5s | %s\n' "multi_file_mod" "FAIL" "-" "-" ""
    PROBE_CFAIL=$(( PROBE_CFAIL + 1 ))
  fi

  echo
  echo "--- same set, emit=llvm-ir only ---"
  for f in *.g; do
    "$GLYIM" "$f" --emit=llvm-ir --target="$HOST_TRIPLE" -o "/tmp/$f.ll" >/dev/null 2>&1 \
      && echo "OK   $f" || echo "FAIL $f"
  done

  echo
  echo "--- emit=asm + emit=mir sanity ---"
  printf 'fn main() -> i32 { 40 + 2 }\n' > asm.g
  "$GLYIM" asm.g --emit=asm --target="$HOST_TRIPLE" -o asm.s 2>&1 | head -5
  ls -la asm.s 2>/dev/null || echo "no asm.s"
  "$GLYIM" asm.g --emit=mir --target="$HOST_TRIPLE" -o asm.mir 2>&1 | head -5
  ls -la asm.mir 2>/dev/null || echo "no asm.mir"

  cd "$ROOT"

  sum "Probe battery:                   $PROBE_PASS/$PROBE_TOTAL PASS, $PROBE_DIFF DIFF, $PROBE_CFAIL compile-fail"
fi

# ============================================================================
hr "SECTION P — Linker + EmitKind surface"
echo "--- linker.rs ---"
cat crates/glyim-cli/src/linker.rs 2>/dev/null
echo "--- EmitKind + run_with_args signature ---"
grep -n "enum EmitKind\|pub fn run_with_args\|pub fn run" crates/glyim-cli/src/lib.rs 2>/dev/null | head -20
echo "--- supported target triples referenced ---"
grep -rhoE '"[a-z0-9_]+-[a-z0-9_.-]+"' crates/glyim-cli crates/glyim-codegen-llvm --include='*.rs' 2>/dev/null | sort -u | head -30

# ============================================================================
hr "PROBE COMPLETE"
echo "date: $(date -u +%FT%TZ)"
echo "log size: $(wc -c <"$LOG") bytes"

# ----------------------------------------------------------------------------
# Write summary and print it to the terminal (fd 3)
# ----------------------------------------------------------------------------
{
  echo "============================================================="
  echo " GLYIM PROBE — SUMMARY"
  echo "============================================================="
  echo " Full log:     $LOG"
  echo " Log size:     $(wc -c <"$LOG") bytes"
  echo
  printf '%s\n' "$SUMMARY_DATA"
  echo "============================================================="
  echo " Next: grep for interesting lines in the log:"
  echo "   grep -n '^PROBE\\|^FAIL\\|^DIFF\\|^OK ' $LOG | head -60"
  echo "   grep -n 'SECTION' $LOG"
  echo "   less $LOG"
  echo "============================================================="
} > "$SUMMARY"

cat "$SUMMARY" >&3
echo
echo "Summary also saved to: $SUMMARY"
echo
echo "Fast peeks:"
echo "  less $LOG"
echo "  grep -n 'DIFF\\|FAIL' $LOG | head -40"
echo "  grep -n 'SECTION O' $LOG"
echo "  grep -c 'missing documentation' $LOG"
