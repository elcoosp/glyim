Looking at your `streams.json`, I see several structural problems that will cause agents to produce inconsistent or conflicting work. Here are my recommendations, ordered by impact.

---

## 1. Everything is Wave 0 — Defeats the Purpose

U07 depends on U01 and U02, but all three are `wave: 0`. U05 and U06 are both `wave: 0` but U06 depends on U05. The wave field is currently meaningless.

**Fix:** Compute waves from the dependency graph:

```json
Wave 0: U01, M01, M02, M03, M04, M05, M06, M07, M08, M09  (no upstream deps)
Wave 1: U02, U05, M10  (depend on wave 0)
Wave 2: U03, U04, U06  (depend on wave 1)
Wave 3: U07  (depends on U01 + U02)
Wave 4: U08  (depends on U07)
```

**Add to `streams.json` schema:**

```json
"_comment": "wave is auto-calculated from upstream deps. Do not set manually."
```

Or better, compute it in `generate-stream.sh`:

```bash
# Topological sort to compute waves
python3 - << 'PYEOF'
import json, sys
with open('streams.json') as f: streams = json.load(f)
ids = {s['id'] for s in streams}
dep_map = {s['id']: s.get('upstream', []) for s in streams}
wave = {}
def compute(id):
    if id in wave: return wave[id]
    if not dep_map.get(id): 
        wave[id] = 0
    else:
        wave[id] = max(compute(d) for d in dep_map[id]) + 1
    return wave[id]
for s in streams: compute(s['id'])
for s in streams: s['wave'] = wave[s['id']]
with open('streams.json', 'w') as f: json.dump(streams, f, indent=2)
PYEOF
```

---

## 2. Shared-Crate Conflicts Not Detected

U05 and U06 both own `glyim-codegen-llvm` and are both wave 0. If they run in parallel, they'll both modify `src/lib.rs` and `src/tests/mod.rs` in the same crate, causing merge conflicts.

U07 and U08 both own `glyim-lsp`. M09 and M10 both own `glyim-lsp`. U02 owns both `glyim-lower` AND `glyim-pipeline` while M06/M07 own `glyim-lower`.

**Fix: Add `file_ownership` field** that specifies which files/modules each stream exclusively owns within a crate:

```json
{
  "id": "U05",
  "name": "LLVM Backend – Core Types & Aggregates",
  "crate": "glyim-codegen-llvm",
  "owned_files": [
    "src/types.rs",
    "src/lower/aggregate.rs",
    "src/lower/repeat.rs",
    "src/tests/types.rs",
    "src/tests/aggregate.rs"
  ],
  "shared_files": [
    "src/lib.rs",
    "src/tests/mod.rs"
  ]
}
```

```json
{
  "id": "U06",
  "name": "LLVM Backend – Operators & Casts",
  "crate": "glyim-codegen-llvm",
  "owned_files": [
    "src/lower/binary_op.rs",
    "src/lower/unary_op.rs",
    "src/lower/cast.rs",
    "src/tests/operators.rs",
    "src/tests/casts.rs"
  ],
  "shared_files": [
    "src/lib.rs",
    "src/tests/mod.rs"
  ]
}
```

**Add validation to `generate-stream.sh`:**

```bash
python3 - << 'PYEOF'
import json
with open('streams.json') as f: streams = json.load(f)
from collections import defaultdict
file_owners = defaultdict(list)
for s in streams:
    for f in s.get('owned_files', []):
        file_owners[f].append(s['id'])
conflicts = {f: owners for f, owners in file_owners.items() if len(owners) > 1}
if conflicts:
    print("CONFLICT: Files owned by multiple streams:")
    for f, owners in conflicts.items():
        print(f"  {f}: {owners}")
    sys.exit(1)
PYEOF
```

---

## 3. Test Specs Are Too Vague

"lower_type_ref parses Array, Dyn, Slice, FnPtr correctly (unit test)" tells the agent nothing about:
- What input to use
- What the expected output is
- What edge cases exist
- What should fail

The agent will invent test data, and two agents working on related streams will invent *different* test data, leading to inconsistent coverage.

**Fix: Expand `tests` into structured objects with input/output:**

```json
"tests": [
  {
    "id": "U01-T01",
    "name": "lower_type_ref_array",
    "description": "Array type reference lowers correctly",
    "input": "[i32; 4]",
    "expected_hir": "TypeRef::Array { inner: TypeRef::Path(Path::from_single(\"i32\")), len: ConstRef::Literal(Literal::Uint(4, None)) }",
    "category": "unit",
    "edge_cases": ["zero-length array", "nested array [i32; 2]"]
  },
  {
    "id": "U01-T02",
    "name": "lower_type_ref_dyn",
    "description": "Dyn type reference lowers correctly",
    "input": "dyn Foo",
    "expected_hir": "TypeRef::Path with PathKind::Plain and segment 'dyn'",
    "category": "unit",
    "edge_cases": ["dyn with multiple traits", "dyn with associated type"]
  }
]
```

This is more verbose, but it eliminates the #1 source of agent drift: ambiguous specifications.

---

## 4. No Stub Inventory — Agents Will Miss Stubs

The scope says "remove stubs in X" but doesn't enumerate them. The agent has to scan the file and guess which `tracing::warn!("STUB:` calls are theirs. This leads to missed stubs.

**Fix: Add `stub_targets` field:**

```json
{
  "id": "U01",
  "stub_targets": [
    {
      "file": "src/lower.rs",
      "function": "lower_type_ref",
      "pattern": "STUB: lower_type_ref unimplemented",
      "replaces": "Full type reference lowering for Array, Dyn, Slice, FnPtr"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_expr",
      "pattern": "STUB: lower_expr unimplemented",
      "replaces": "Expression lowering for Closure, Struct, Range, Index, Cast"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_bin_op_token",
      "pattern": "STUB: lower_bin_op_token",
      "replaces": "Binary operator token mapping"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_pat",
      "pattern": "STUB: lower_pat unimplemented",
      "replaces": "Pattern lowering with unknown kind fallback"
    }
  ]
}
```

You could even auto-generate this by scanning the codebase:

```bash
grep -rn 'STUB:' crates/glyim-hir/src/ | sed 's/:.*STUB: /|/;s/:/|/' | \
  awk -F'|' '{printf "{\"file\": \"%s\", \"line\": %s, \"message\": \"%s\"},\n", $1, $2, $3}'
```

---

## 5. U02 Owns Two Crates — Needs Module-Level Split

U02 owns both `glyim-lower` and `glyim-pipeline`. This is fine if the agent knows *which files* in each crate to touch. But currently it just says "remove stubs in lower.rs and mono_cache.rs and pipeline_context.rs" — the agent might touch unrelated files.

**Fix: Add `owned_modules` alongside `owned_crates`:**

```json
{
  "id": "U02",
  "owned_crates": ["glyim-lower", "glyim-pipeline"],
  "owned_modules": {
    "glyim-lower": ["src/lower.rs", "src/tests/"],
    "glyim-pipeline": ["src/mono_cache.rs", "src/pipeline_context.rs", "src/tests/"]
  },
  "read_only_modules": {
    "glyim-lower": ["src/lib.rs"],
    "glyim-pipeline": ["src/lib.rs", "src/pipeline.rs"]
  }
}
```

This tells the agent: "you may write to these files; you may only read from these files."

---

## 6. No Acceptance Criteria for "Run-Pass" Tests

"U04-T01: Assign with field projection writes to correct offset (run-pass via bytecode interpreter)" — the agent has no idea what program to compile, what the bytecode should look like, or what "correct offset" means.

**Fix: Add `test_programs` — the actual Glyim source the agent should compile:**

```json
"tests": [
  {
    "id": "U04-T01",
    "name": "assign_field_projection",
    "category": "run-pass",
    "source": "struct Point { x: i32, y: i32 }\nfn main() { let mut p = Point { x: 1, y: 2 }; p.x = 42; assert(p.x == 42); }",
    "expected_behavior": "Program completes without error, p.x == 42",
    "backend": "bytecode"
  }
]
```

This is the single highest-leverage improvement. Without it, every agent invents its own test programs, and you get zero consistency across streams.

---

## 7. M-Streams (Refactoring) Need New File Templates

M01 says "split lib.rs into lower/, types/, passes/, debug/" but doesn't tell the agent what goes in each file. The agent has to guess at the module boundaries.

**Fix: Add `target_structure` for M-streams:**

```json
{
  "id": "M01",
  "name": "Split LLVM Backend",
  "target_structure": {
    "src/lower/mod.rs": "Re-exports from lower submodules",
    "src/lower/mir_lower.rs": "MirLower struct, lower_body, lower_terminator, lower_statement",
    "src/lower/rvalue.rs": "lower_rvalue, lower_aggregate, lower_repeat, lower_len",
    "src/lower/operand.rs": "lower_operand, lower_constant, lower_place",
    "src/types/mod.rs": "Re-exports from types submodules",
    "src/types/primitive.rs": "lower_int_ty, lower_uint_ty, lower_float_ty, lower_bool, lower_char",
    "src/types/aggregate.rs": "lower_adt, lower_tuple, lower_closure, lower_array",
    "src/types/pointer.rs": "lower_ref, lower_raw_ptr, lower_fn_ptr",
    "src/passes/mod.rs": "PassManager, run_passes",
    "src/debug/mod.rs": "DwarfBuilder, emit_debug_info"
  },
  "preserved_exports": [
    "LlvmBackend::new",
    "LlvmBackend::with_target",
    "LlvmBackend::generate",
    "LlvmBackend::generate_function"
  ]
}
```

---

## 8. Add `conflicts_with` Field

Even after file-ownership, some streams might modify the same `Cargo.toml` or `lib.rs`. Make this explicit:

```json
{
  "id": "U05",
  "conflicts_with": ["U06", "M01"],
  "conflict_reason": "Shares glyim-codegen-llvm. U05 must merge before U06 starts. M01 is refactoring-only and should merge first if possible."
}
```

Add to `generate-stream.sh`:

```bash
# Warn if two streams in the same wave own the same crate
python3 - << 'PYEOF'
import json
from collections import defaultdict
with open('streams.json') as f: streams = json.load(f)
wave_crates = defaultdict(lambda: defaultdict(list))
for s in streams:
    for c in s.get('owned_crates', []):
        wave_crates[s['wave']][c].append(s['id'])
for wave, crates in wave_crates.items():
    for crate, owners in crates.items():
        if len(owners) > 1:
            print(f"WARNING: Wave {wave} crate {crate} owned by {owners} — potential conflict")
PYEOF
```

---

## 9. Add `pre_merge_check` Command

Each stream should specify how to verify it hasn't broken anything *outside its crate*:

```json
{
  "id": "U01",
  "pre_merge_check": [
    "cargo test -p glyim-hir",
    "cargo test -p glyim-typeck --lib",
    "cargo check --workspace"
  ]
}
```

This goes beyond the stream's own verification — it ensures downstream crates still compile.

---

## 10. Revised Schema (Complete Example)

Here's what a fully-specified stream entry should look like:

```json
{
  "id": "U01",
  "name": "HIR Lowering – Expressions & Types",
  "crate": "glyim-hir",
  "wave": 0,
  "scope_summary": "Remove stubs in lower.rs for type/expr/pat lowering. Implement Array, Dyn, Slice, FnPtr types and Closure, Struct, Range, Index, Cast expressions.",

  "owned_crates": ["glyim-hir"],
  "owned_files": [
    "src/lower.rs",
    "src/tests/lower_type.rs",
    "src/tests/lower_expr.rs",
    "src/tests/lower_pat.rs"
  ],
  "shared_files": [
    "src/lib.rs",
    "src/tests/mod.rs"
  ],
  "read_only_files": [
    "src/hir_defs.rs",
    "src/name.rs"
  ],

  "locked_interfaces": [
    "glyim_hir::lower_crate_for_pipeline",
    "glyim_hir::CrateHir",
    "glyim_hir::Item",
    "glyim_hir::Body"
  ],

  "stub_targets": [
    {
      "file": "src/lower.rs",
      "function": "lower_type_ref",
      "line_pattern": "STUB: lower_type_ref",
      "replacement_scope": "Array, Dyn, Slice, FnPtr type references"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_expr",
      "line_pattern": "STUB: lower_expr",
      "replacement_scope": "Closure, Struct, Range, Index, Cast expressions"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_bin_op_token",
      "line_pattern": "STUB: lower_bin_op_token",
      "replacement_scope": "All BinOp token mappings"
    },
    {
      "file": "src/lower.rs",
      "function": "lower_pat",
      "line_pattern": "STUB: lower_pat",
      "replacement_scope": "All pattern kinds, unknown fallback to Pat::Err"
    }
  ],

  "tests": [
    {
      "id": "U01-T01",
      "name": "lower_type_ref_array",
      "category": "unit",
      "description": "Array type [T; N] lowers to TypeRef::Array",
      "input_code": "let x: [i32; 4];",
      "expected": "TypeRef::Array { inner: Box<TypeRef::Path(i32)>, len: ConstRef::Literal(Uint(4)) }",
      "edge_cases": ["zero-length [i32; 0]", "nested [[i32; 2]; 3]"]
    },
    {
      "id": "U01-T02",
      "name": "lower_type_ref_dyn",
      "category": "unit",
      "description": "dyn Trait lowers to TypeRef with dyn marker",
      "input_code": "let x: dyn Foo;",
      "expected": "TypeRef::Path with dyn semantics",
      "edge_cases": ["dyn with multiple traits"]
    },
    {
      "id": "U01-T03",
      "name": "lower_type_ref_slice",
      "category": "unit",
      "description": "[T] lowers to TypeRef::Slice",
      "input_code": "let x: [i32];",
      "expected": "TypeRef::Slice(Box<TypeRef::Path(i32)>)"
    },
    {
      "id": "U01-T04",
      "name": "lower_type_ref_fnptr",
      "category": "unit",
      "description": "fn(i32) -> bool lowers to TypeRef::Fn",
      "input_code": "let f: fn(i32) -> bool;",
      "expected": "TypeRef::Fn { params: [Path(i32)], ret: Some(Path(bool)) }"
    },
    {
      "id": "U01-T05",
      "name": "lower_expr_struct",
      "category": "unit",
      "description": "Struct literal lowers to Expr::Struct",
      "input_code": "Point { x: 1, y: 2 }",
      "expected": "Expr::Struct { path: Point, fields: [(x, Literal(1)), (y, Literal(2))], spread: None }"
    },
    {
      "id": "U01-T06",
      "name": "number_suffix_parsing",
      "category": "unit",
      "description": "Integer/float suffixes parsed correctly",
      "input_code": "42i32, 1u64, 3.14f64",
      "expected": "Literal::Int(42, Some(I32)), Literal::Uint(1, Some(U64)), Literal::Float(bits, F64)",
      "edge_cases": ["no suffix (infer)", "invalid suffix (error)"]
    },
    {
      "id": "U01-T07",
      "name": "unknown_pattern_error",
      "category": "unit",
      "description": "Unknown pattern kind produces Pat::Err with diagnostic",
      "input_code": "impossible_pattern_syntax",
      "expected": "Pat::Err + GlyimDiagnostic emitted"
    }
  ],

  "mocking": "No mocking needed. Use glyim_test::assert_ty and snapshot_cst for HIR output verification.",

  "conflicts_with": ["M03"],
  "conflict_note": "M03 splits lower.rs into submodules. If M03 merges first, U01's stub targets will be in different files. Coordinate: M03 should merge BEFORE U01, or U01 should merge first and M03 rebase onto it.",

  "upstream": [],
  "downstream": ["U02", "U03", "U07"],

  "pre_merge_check": [
    "cargo test -p glyim-hir",
    "cargo test -p glyim-typeck --lib",
    "cargo check -p glyim-lower",
    "cargo check --workspace"
  ]
}
```

---

## Summary: New Fields to Add to Schema

| Field | Type | Purpose |
|-------|------|---------|
| `owned_files` | `string[]` | Files this stream exclusively modifies |
| `shared_files` | `string[]` | Files modified using safe-append pattern |
| `read_only_files` | `string[]` | Files the stream may read but NOT modify |
| `stub_targets` | `object[]` | Exact stub functions/patterns to replace |
| `tests` | `object[]` | Structured test specs with input/output/edge cases |
| `conflicts_with` | `string[]` | Stream IDs that may conflict |
| `conflict_note` | `string` | How to resolve the conflict |
| `target_structure` | `object` | (M-streams only) New file → contents description |
| `preserved_exports` | `string[]` | (M-streams only) Public items that must remain unchanged |
| `pre_merge_check` | `string[]` | Commands to run before merging to verify no breakage |

And update `generate-stream.sh` to:
1. Auto-compute waves from dependency graph
2. Validate no file ownership conflicts within the same wave
3. Warn about shared-crate streams in the same wave
4. Include `owned_files`, `stub_targets`, and structured tests in the generated brief
