# Glyim justfile
# Usage: just <recipe>
#        just --list

set positional-arguments

# ─── Build ───────────────────────────────────────────────────

# Build the CLI in release mode
build:
    cargo build --release -p glyim-cli

# Build the CLI in debug mode (fast)
build-debug:
    cargo build -p glyim-cli

# Check all crates compile (no codegen)
check:
    cargo check --workspace

# ─── v0.1.0 Demo ─────────────────────────────────────────────

# Run the v0.1.0 demo: compile and execute `main = () => 42`
demo: build
    #!/usr/bin/env bash
    set -e
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT
    echo 'main = () => 42' > "$TMPDIR/main.xyz"
    echo "── Source ──────────────────────────"
    cat "$TMPDIR/main.xyz"
    echo "── Compile & Run ───────────────────"
    ./target/release/glyim run "$TMPDIR/main.xyz"
    EXIT_CODE=$?
    echo "── Exit Code ───────────────────────"
    echo "$EXIT_CODE"

# Run the demo with verbose IR output
demo-ir: build
    #!/usr/bin/env bash
    set -e
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT
    echo 'main = () => 42' > "$TMPDIR/main.xyz"
    echo "── Source ──────────────────────────"
    cat "$TMPDIR/main.xyz"
    echo "── LLVM IR ─────────────────────────"
    ./target/release/glyim ir "$TMPDIR/main.xyz"

# Run a more complex v0.1.0 demo (arithmetic, precedence)
demo-math: build
    #!/usr/bin/env bash
    set -e
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT
    cat > "$TMPDIR/math.xyz" << 'EOF'
    main = () => 1 + 2 * 3
    EOF
    echo "── Source ──────────────────────────"
    cat "$TMPDIR/math.xyz"
    echo "── Compile & Run ───────────────────"
    ./target/release/glyim run "$TMPDIR/math.xyz"
    EXIT_CODE=$?
    echo "── Expected: 7 (1 + (2*3)), Got: $EXIT_CODE ──"

# ─── Watch Mode (v0.1.0 Demo) ───────────────────────────────

# Watch all Rust source and re-run demo on save
watch-src: build
    watchexec -w crates/ --clear -r "just demo"

# ─── Testing ─────────────────────────────────────────────────

# Run all tests with nextest (pretty output, JUnit-like grouping)
test:
    cargo nextest run --workspace

# Run tests for a specific crate
test-crate CRATE:
    cargo nextest run -p {{ CRATE }}

# Run only unit tests (skip integration/fuzz)
test-unit:
    cargo nextest run --workspace --lib --bins

# Run only integration tests
test-integration:
    cargo nextest run -p glyim-cli --test integration

# Run tests with verbose output (see full test names)
test-verbose:
    cargo nextest run --workspace --success-output immediate-final

# Run tests and stop on first failure
test-fail-fast:
    cargo nextest run --workspace --fail-fast

# Run tests, then show a summary of slow tests (>100ms)
test-slow:
    cargo nextest run --workspace --status-level fail
    cargo nextest run --workspace --status-level slow 2>&1 | tail -20

# Re-run only tests that failed last time
test-retry:
    cargo nextest run --workspace --run-ignored=only

# Run a single test by exact name
test-one NAME:
    cargo nextest run -p glyim-cli --exact '{{ NAME }}'

# Run all tests with JSON output (for CI)
test-json:
    cargo nextest run --workspace --format json

# ─── Fuzzing ────────────────────────────────────────────────

# Run the lexer fuzzer (requires cargo-fuzz)
fuzz-lexer:
    cd fuzz && cargo fuzz run fuzz_lexer

# Run the parser fuzzer (requires cargo-fuzz)
fuzz-parser:
    cd fuzz && cargo fuzz run fuzz_parser

# ─── Quality ────────────────────────────────────────────────

# Check for cyclic dependencies
check-dag:
    #!/usr/bin/env bash
    cargo metadata --format-version=1 | python3 -c '
    import json, sys
    data = json.load(sys.stdin)
    pkgs = {p["name"]: [d["name"] for d in p["dependencies"]] for p in data["packages"] if p["source"] is None}
    glyim = {k: [d for d in v if d.startswith("glyim-")] for k, v in pkgs.items() if k.startswith("glyim-")}
    visiting, visited = set(), set()
    def dfs(n):
        if n in visited: return True
        if n in visiting: return False
        visiting.add(n)
        for d in glyim.get(n, []):
            if not dfs(d): return False
        visiting.remove(n); visited.add(n); return True
    for n in glyim:
        if not dfs(n): print(f"CYCLE: {n}"); sys.exit(1)
    print("✅ DAG verified: no cycles")
    '

# Check tier constraints (no cross-tier violations)
check-tiers:
    #!/usr/bin/env bash
    cargo metadata --format-version=1 | python3 -c '
    import json, sys
    data = json.load(sys.stdin)
    tiers = {
        "glyim-interner": 1, "glyim-diag": 1, "glyim-syntax": 1,
        "glyim-lex": 2, "glyim-parse": 2,
        "glyim-hir": 3, "glyim-typeck": 3, "glyim-macro-core": 3, "glyim-macro-vfs": 3,
        "glyim-codegen-llvm": 4,
        "glyim-cli": 5,
    }
    pkgs = {p["name"]: [d["name"] for d in p["dependencies"]] for p in data["packages"] if p["source"] is None}
    violations = []
    for crate, deps in pkgs.items():
        if crate not in tiers: continue
        my_tier = tiers[crate]
        for dep in deps:
            if dep in tiers and tiers[dep] > my_tier:
                violations.append(f"  {crate} (tier {my_tier}) → {dep} (tier {tiers[dep]})")
    if violations:
        print("❌ Tier violations:")
        for v in violations: print(v)
        sys.exit(1)
    else:
        print("✅ Tier constraints verified: no cross-tier violations")
    '

# Run all quality checks
qa: check check-dag check-tiers test
    @echo "✅ All quality checks passed"

# ─── Counting ───────────────────────────────────────────────

# Count total tests across workspace
count-tests:
    #!/usr/bin/env bash
    cargo nextest run --workspace --dry-run 2>&1 | grep -oP '\d+ test(s)?' | tail -1
    echo ""
    cargo test --workspace -- --list 2>/dev/null | grep -c ' tests$'
    echo " tests listed (binary count)"

# Count lines of code per crate
loc:
    #!/usr/bin/env bash
    echo "Lines of code per crate:"
    echo "──────────────────────────────"
    for crate in crates/*/; do
        name=$(basename "$crate")
        count=$(find "$crate/src" -name '*.rs' -exec cat {} \; 2>/dev/null | wc -l)
        if [ "$count" -gt 0 ]; then
            printf "  %-25s %5d\n" "$name" "$count"
        fi
    done | sort -t' ' -k2 -rn
    echo "──────────────────────────────"
    total=$(find crates -name '*.rs' -exec cat {} \; | wc -l)
    printf "  %-25s %5d\n" "TOTAL" "$total"

# ─── CI Simulation ───────────────────────────────────────────

# Simulate what CI would run
ci: check check-dag check-tiers build test-unit test-integration
    @echo "✅ CI simulation passed"

# ─── Workspace ──────────────────────────────────────────────

# Show dependency graph as ASCII
graph:
    cargo tree --workspace --depth 1 --edges normal

# Show dependency graph for a specific crate
graph-crate CRATE:
    cargo tree -p {{ CRATE }} --edges normal

# Clean all build artifacts
clean:
    cargo clean

# Clean and rebuild from scratch
rebuild: clean build
    @echo "✅ Full rebuild complete"

# ─── Help ───────────────────────────────────────────────────

# Show this help
default:
    just --list
wr:
    watchexec -w ./wr.sh --clear -r "sh ./wr.sh"
