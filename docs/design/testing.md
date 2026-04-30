# Glyim Test Improvement Plan

## Executive Summary

After a full audit of the codebase, I identified **237 existing tests** across 7 crates and **5 crates with zero coverage**. The most critical gap is `glyim-typeck` (0 tests) — the type checker is the largest untested surface. Additionally, **6 of 15 UI snapshots are empty** (passing trivially instead of catching errors), and the AST→HIR lowering pipeline has only ad-hoc debug tests. This plan is organized into 7 phases, ordered by regression risk.

---

## 0. Current State Audit

### Coverage Matrix

| Crate | Unit | Integration | UI/Snapshot | Fuzz | Coverage Grade |
|-------|------|-------------|-------------|------|----------------|
| `glyim-interner` | 10 | 0 | 0 | 0 | **A** |
| `glyim-diag` | 0 | 0 | 0 | 0 | **F** |
| `glyim-syntax` | 4 | 0 | 0 | 0 | **C** |
| `glyim-lex` | 47 | 0 | 0 | 0 | **A** |
| `glyim-parse` | 0 | 30 | 0 | 0 | **B+** |
| `glyim-hir` | 6 | 0 | 0 | 0 | **D** |
| `glyim-typeck` | **0** | **0** | **0** | 0 | **F** |
| `glyim-codegen-llvm` | 20 | 0 | 0 | 0 | **B** |
| `glyim-cli` | 8 | 40 | 15 | 0 | **B-** |
| `glyim-pkg` | 0 | 18 | 0 | 0 | **B** |
| `glyim-macro-vfs` | 4 | 3 | 0 | 0 | **B** |
| `glyim-macro-core` | 0 | 0 | 0 | 0 | N/A (stub) |
| `glyim-cas-server` | **0** | **0** | **0** | 0 | **F** |

### Known Broken Snapshots (passing with empty output)

These UI tests produce empty stderr because `compile_stderr()` only reports parse errors — type errors are silently dropped:

- `ui__bool_mismatch.snap` → empty
- `ui__type_mismatch.snap` → empty
- `ui__duplicate_param.snap` → `"error: no 'main' function"` (wrong error)
- `ui__assign_immutable.snap` → `"error: no 'main' function"` (wrong error)
- `ui__unexpected_token.snap` → `"error: no 'main' function"` (wrong error)
- `ui__unterminated_string.snap` → `"error: no 'main' function"` (wrong error)
- `ui__empty_source.snap` → `"error: no 'main' function"` (wrong error)
- `ui__if_missing_brace.snap` → `"error: no 'main' function"` (wrong error)
- `ui__missing_main.snap` → `"error: no 'main' function"` (correct)

### Known `#[ignore]` Tests (deferred features)

| Test | Reason | Risk if un-ignored |
|------|--------|-------------------|
| `e2e_tuple` | Tuple field GEP not implemented | Crash/segfault |
| `e2e_impl_method` | Method name mangling broken | Wrong codegen |
| `e2e_generic_edge` | Monomorphization not implemented | Wrong codegen |
| `parse_unit_literal` | Parser doesn't handle `()` | Compile error |
| `parse_raw_pointer` | Pointer syntax partial | Parse error |
| `parse_let_with_type_annotation` | Type annotation in let partial | Parse error |
| `e2e_type_error_int_plus_bool` | Operand compatibility unchecked | Wrong result |

---

## Phase 1: Critical Regression Prevention

**Goal**: Eliminate zero-coverage crates that are core compiler infrastructure.
**Effort**: 2–3 days
**Priority**: 🔴 P0 — blocks all other phases

### 1.1 `glyim-diag` — Span & Source Span Tests

Create `crates/glyim-diag/src/lib.rs` test module:

```
#[cfg(test)]
mod tests {
    // Span construction
    - span_new_valid          // Span::new(0, 5) ok
    - span_new_equal_bounds   // Span::new(3, 3) is empty
    - span_new_panic_on_inverted  // #[should_panic] Span::new(5, 0)
    - span_len                // span.len() == 5
    - span_is_empty_true      // Span::new(3,3).is_empty()
    - span_is_empty_false     // Span::new(3,4).is_empty()

    // Source span conversion
    - into_source_span_basic  // Span{0,5} → SourceSpan(0..5)
    - into_source_span_empty  // Span{3,3} → SourceSpan(3..3)
    - into_source_span_large  // Span{0, usize::MAX} doesn't overflow
}
```

### 1.2 `glyim-typeck` — Type Checker Tests

Create `crates/glyim-typeck/src/typeck/tests.rs` (new file):

**Scope & Binding:**
```
- lookup_unbound_returns_none
- let_binding_is_visible_in_same_scope
- let_binding_is_not_visible_in_parent_scope
- nested_scopes_shadow
- with_scope_restores_after_pop
```

**Struct Registration:**
```
- register_struct_fields_indexed
- register_struct_field_map_lookup
- register_struct_empty
- register_struct_duplicate_field_uses_last_index
```

**Enum Registration:**
```
- register_enum_variants_indexed
- register_enum_variant_map_lookup
- register_enum_empty
```

**Expression Inference:**
```
- infer_int_lit_is_int
- infer_float_lit_is_float
- infer_bool_lit_is_bool
- infer_str_lit_is_str
- infer_unit_lit_is_unit
- infer_ident_returns_binding_type
- infer_ident_unbound_returns_int_fallback
- infer_binary_int
- infer_unary_neg
- infer_unary_not
- infer_block_returns_last_expr_type
- infer_block_empty_returns_unit
- infer_if_returns_then_type
- infer_if_else_branches
- infer_if_without_else_returns_unit
- infer_struct_lit_returns_named_type
- infer_enum_variant_returns_named_type
- infer_tuple_lit_returns_tuple_type
- infer_size_of_returns_int
```

**Function Checking:**
```
- check_fn_params_bound_in_body
- check_fn_body_type_matches_return_annotation_pass
- check_fn_body_type_mismatch_return_pushes_error
- check_fn_no_return_annotation_ok
```

**Struct Literal Checking:**
```
- check_struct_lit_valid_fields_ok
- check_struct_lit_unknown_field_pushes_error
- check_struct_lit_missing_field_pushes_error
- check_struct_lit_extra_field_pushes_error
- check_struct_lit_unregistered_struct_no_error
```

**Field Access Checking:**
```
- check_field_access_known_field_ok
- check_field_access_unknown_field_pushes_error
- check_tuple_field_access__0_ok
- check_tuple_field_access__1_ok
- check_tuple_field_access_out_of_bounds_pushes_error
- check_tuple_field_access_non_numeric_pushes_error
```

**Enum Checking:**
```
- check_enum_variant_known_ok
- check_enum_variant_unknown_pushes_error
```

**Match Exhaustiveness:**
```
- match_exhaustive_with_wildcard_ok
- match_exhaustive_all_variants_ok
- match_non_exhaustive_pushes_error
- match_on_non_enum_no_error
- match_on_option_some_none_exhaustive
- match_on_result_ok_err_exhaustive
```

**Cast Validation:**
```
- check_as_int_to_float_valid
- check_as_float_to_int_valid
- check_as_int_to_str_invalid_pushes_error
- check_as_same_type_valid
```

**Call Checking:**
```
- check_call_known_fn_returns_return_type
- check_call_unknown_fn_returns_int_fallback
- check_call_extern_fn_returns_extern_return_type
- check_call_impl_method_returns_method_return_type
```

**Error Accumulation:**
```
- multiple_errors_accumulate_without_panic
- check_returns_ok_when_no_errors
- check_returns_err_when_errors_exist
```

### 1.3 `glyim-hir/lower` — Dedicated Lowering Tests

Create `crates/glyim-hir/src/lower/tests.rs` (new file):

```
- lower_int_lit
- lower_float_lit
- lower_bool_lit
- lower_str_lit
- lower_ident
- lower_binary_expr
- lower_unary_neg
- lower_unary_not
- lower_block_with_stmts
- lower_block_with_trailing_expr
- lower_if_without_else
- lower_if_with_else
- lower_let_stmt
- lower_let_mut_stmt
- lower_assign_stmt
- lower_fn_def_no_params
- lower_fn_def_with_params
- lower_fn_def_with_return_type
- lower_struct_def
- lower_struct_def_with_type_params
- lower_enum_def
- lower_enum_def_with_variants
- lower_impl_block_mangles_method_names
- lower_match_with_wildcard
- lower_match_with_enum_patterns
- lower_some_expr
- lower_none_expr
- lower_ok_expr
- lower_err_expr
- lower_try_expr_desugars_to_match
- lower_macro_call_identity_passes_through
- lower_macro_call_unknown_returns_zero
- lower_as_expr
- lower_struct_literal
- lower_field_access
- lower_enum_variant_construction
- lower_tuple_literal
- lower_size_of
- lower_extern_block
- lower_let_pattern_var
- lower_let_pattern_wildcard
- lower_match_arm_with_guard
- lower_fn_binding_shorthand
- lower_fn_with_test_attribute
- lower_fn_with_ignore_attribute
- expr_ids_are_monotonic  // fresh_id produces 0,1,2,...
- lower_empty_source Produces empty Hir
```

---

## Phase 2: UI Test Overhaul

**Goal**: Fix all broken snapshots, add type-error UI tests, expand error coverage.
**Effort**: 1–2 days
**Priority**: 🔴 P0

### 2.1 Fix `compile_stderr` to Run Full Pipeline

The root cause: `compile_stderr()` in `tests/ui.rs` stops after parsing. It must run type checking and codegen too.

```rust
fn compile_stderr(source: &str, file_path: &str) -> String {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        // existing parse error formatting...
        return output;
    }

    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let mut typeck = glyim_typeck::TypeChecker::new(interner);
    if let Err(errs) = typeck.check(&hir) {
        for e in &errs {
            // format type errors with miette
        }
    }

    // Try codegen
    match glyim_codegen_llvm::compile_to_ir(source) {
        Ok(_) => {}
        Err(e) => { /* format codegen error */ }
    }

    if output.is_empty() && !parse_out.errors.is_empty() {
        // fallback
    }
    output
}
```

### 2.2 Update Existing Snapshots

After fixing the harness, re-run `cargo insta test` and review every snapshot:

| File | Expected Error (after fix) |
|------|---------------------------|
| `assign_immutable.g` | Type error: cannot assign to immutable binding `x` *(or parse error if not implemented)* |
| `bool_mismatch.g` | Type error: expected `bool`, found `i64` |
| `type_mismatch.g` | Type error: expected `bool`, found `i64` |
| `duplicate_param.g` | Parse error: duplicate parameter name `a` *(or type error)* |
| `empty_source.g` | Error: no `main` function |
| `if_missing_brace.g` | Parse error: expected `{` |
| `unexpected_token.g` | Parse error: unexpected `$` |
| `unterminated_string.g` | Parse error: unterminated string |

**Action**: Run `cargo insta test --accept` after the harness fix, then manually audit each accepted snapshot for correctness.

### 2.3 New UI Test Cases

Add to `tests/ui/`:

```
// Parse errors
missing_fn_body.g          // "fn foo() " — no body
missing_struct_name.g      // "struct { x }" — no name
missing_enum_name.g        // "enum { A }" — no name
invalid_attr_syntax.g      // "#[test(" — unclosed
attr_on_wrong_item.g       // "#[test]\nstruct S {}" — test on struct

// Type errors
type_error_let_mismatch.g       // "main = () => { let x: bool = 42; x }"
type_error_return_mismatch.g   // "fn foo() -> bool { 42 }\nmain = () => foo()"
type_error_call_wrong_args.g   // (when arity checking exists)
type_error_non_exhaustive.g    // "enum C { A, B }\nmain = () => match C::A { C::A => 1 }"
type_error_invalid_cast.g      // "main = () => 42 as Str"  (existing but snapshot was empty)

// Codegen errors
missing_main_function.g   // "fn other() { 1 }" (existing, verify snapshot)
```

### 2.4 Add `.g` File Header Convention

Each `.g` file should start with a comment indicating expected error category:

```glyim
// expect: type_error
main = () => { let x: bool = 42; x }
```

Update the harness to verify the comment matches the actual error category (future work, not blocking).

---

## Phase 3: Codegen Hardening

**Goal**: Increase confidence in LLVM IR generation correctness.
**Effort**: 2 days
**Priority**: 🟡 P1

### 3.1 IR Snapshot Tests

Create `crates/glyim-codegen-llvm/tests/ir_snapshots.rs`:

Use `insta` to snapshot the full LLVM IR for key programs. This catches unintended IR changes.

```
- ir__minimal_main         // "main = () => 42"
- ir__arithmetic           // "main = () => 1 + 2 * 3"
- ir__let_binding          // "main = () => { let x = 10; x + 1 }"
- ir__if_else              // "main = () => { if 1 { 10 } else { 20 } }"
- ir__match_basic          // "main = () => match 1 { 1 => 10, _ => 20 }"
- ir__struct_literal       // "struct P { x, y }\nmain = () => P { x: 1, y: 2 }"
- ir__field_access         // "struct P { x, y }\nmain = () => { let p = P { x: 1, y: 2 }; p.x }"
- ir__enum_variant         // "enum C { R, G }\nmain = () => C::G"
- ir__option_some          // "main = () => Some(42)"
- ir__result_ok            // "main = () => Ok(42)"
- ir__println_int          // "main = () => { println(42) }"
- ir__println_str          // "main = () => { println(\"hi\") }"
- ir__assert_pass          // "main = () => { assert(1) }"
- ir__function_call        // "fn add(a, b) { a + b }\nmain = () => add(1, 2)"
- ir__no_std               // "no_std\nmain = () => 42"
- ir__test_harness         // "#[test]\nfn a() { 0 }"
```

### 3.2 Verification Tests

Test that `module.verify(true)` passes for valid programs:

```
- verify_minimal_main_passes
- verify_arithmetic_passes
- verify_struct_passes
- verify_enum_passes
- verify_match_passes
- verify_nested_calls_passes
```

### 3.3 Runtime Shims Tests

Expand `runtime_shims` testing (currently no dedicated tests):

```
- runtime_shims_declares_printf
- runtime_shims_declares_write
- runtime_shims_declares_abort
- runtime_shims_println_int_emits_call
- runtime_shims_println_str_emits_call
- runtime_shims_assert_fail_emits_write_and_abort
```

### 3.4 Type-to-LLVM Mapping Tests

```
- hir_type_to_llvm_int_is_i64
- hir_type_to_llvm_bool_is_i64  // current representation
- hir_type_to_llvm_float_is_f64
- hir_type_to_llvm_str_is_struct_ptr_len
- hir_type_to_llvm_unit_is_i64
- hir_type_to_llvm_named_struct_lookup
- hir_type_to_llvm_tuple_of_ints
- hir_type_to_llvm_raw_ptr_is_pointer
```

---

## Phase 4: Package Manager Testing

**Goal**: Harden `glyim-pkg` edge cases and `glyim-cli` manifest/lockfile integration.
**Effort**: 1–2 days
**Priority**: 🟡 P1

### 4.1 Manifest Parsing Edge Cases

Create `crates/glyim-pkg/tests/manifest_edge_tests.rs`:

```
- parse_empty_toml_fails
- parse_missing_package_name_fails
- parse_missing_package_section_but_has_workspace_ok
- parse_full_manifest_all_sections
- parse_dependency_with_version
- parse_dependency_with_path
- parse_dependency_with_registry
- parse_dependency_workspace_flag
- parse_macro_dependency
- parse_dev_dependency
- parse_target_config
- parse_cache_config
- parse_features_config
- parse_workspace_members
- parse_workspace_dependencies
- parse_empty_dependencies_ok
- parse_extra_unknown_fields_ignored  // toml is permissive
- serialize_roundtrip
- load_manifest_from_file
- load_manifest_missing_file_errors
```

### 4.2 Lockfile Edge Cases

Expand `crates/glyim-pkg/tests/lockfile_tests.rs`:

```
- parse_lockfile_with_path_source
- parse_lockfile_with_local_source
- parse_lockfile_with_deps_list
- parse_lockfile_with_macros
- generate_lockfile_preserves_is_macro
- generate_lockfile_deps_sorted
- serialize_includes_all_fields
- compute_content_hash_sha256_known_vector
- parse_lockfile_missing_packages_key_fails
- parse_lockfile_extra_fields_ignored
```

### 4.3 Resolver Edge Cases

```
- resolve_diamond_dependency
- resolve_same_dep_multiple_constraints
- resolve_with_lockfile_uses_locked_version
- resolve_caret_major_boundary  // ^1.99.9 should not match 2.0.0
- resolve_empty_available_errors
- resolve_circular_deps_detected_or_errors  // current impl may infinite loop
```

### 4.4 CAS Client Edge Cases

```
- store_large_content
- store_binary_content_with_null_bytes
- retrieve_nonexistent_returns_none
- register_name_overwrites
- resolve_unregistered_name_returns_none
- has_blobs_all_present_returns_empty
- has_blobs_none_present_returns_all
- new_with_remote_bad_auth_still_works_locally
```

### 4.5 CLI Manifest Integration

Expand `crates/glyim-cli/src/lockfile_integration.rs` tests:

```
- resolve_and_write_with_path_dep
- resolve_and_write_with_existing_lockfile_reuses
- resolve_and_write_manifest_parse_error_propagates
- read_lockfile_packages_nonexistent_dir_returns_empty
- read_lockfile_packages_invalid_toml_errors
- read_lockfile_packages_valid_returns_list
- compute_path_hash_deterministic
- compute_path_hash_excludes_dot_git
```

---

## Phase 5: Integration & E2E Expansion

**Goal**: Cover more language features end-to-end.
**Effort**: 2–3 days
**Priority**: 🟢 P2

### 5.1 New E2E Tests

Add to `crates/glyim-cli/tests/integration.rs`:

```
// Arithmetic edge cases
- e2e_subtraction
- e2e_multiplication
- e2e_division
- e2e_modulo
- e2e_negative_numbers       // "main = () => 0 - 5"
- e2e_integer_overflow_safe  // "main = () => 9223372036854775807" (i64 max)

// Comparison operators
- e2e_eq_true
- e2e_eq_false
- e2e_neq_true
- e2e_lt
- e2e_gt
- e2e_lte
- e2e_gte

// Logical operators
- e2e_and_true
- e2e_and_false
- e2e_or_true
- e2e_or_false
- e2e_not_true
- e2e_not_false

// String operations
- e2e_string_equality       // when implemented
- e2e_empty_string

// Control flow
- e2e_nested_if
- e2e_elif_chain
- e2e_if_in_let
- e2e_match_with_guard      // when guard evaluation works at runtime

// Functions
- e2e_fn_call_no_args
- e2e_fn_call_with_args
- e2e_fn_recursive          // "fn fib(n) -> i64 { if n < 2 { n } else { fib(n-1) + fib(n-2) } }"
- e2e_fn_mutual_recursion   // when supported
- e2e_fn_higher_order       // when closures work

// Struct operations
- e2e_struct_field_multiple_access
- e2e_struct_shorthand_field  // "struct P { x }\nmain = () => { let x = 1; P { x } }"
- e2e_nested_struct

// Enum operations
- e2e_enum_match_all_variants
- e2e_enum_match_with_payload
- e2e_nested_enum_match

// Error handling
- e2e_try_ok_returns_value
- e2e_try_err_aborts
- e2e_nested_try

// Prelude types
- e2e_option_match_some
- e2e_option_match_none
- e2e_result_match_ok
- e2e_result_match_err
- e2e_option_none_no_match

// Misc
- e2e_multiple_let_bindings
- e2e_reassignment
- e2e_block_nesting_deep
- e2e_comment_in_source
- e2e_line_comment_between_tokens
- e2e_block_comment_mid_expression

// no_std
- e2e_no_std_basic
- e2e_no_std_no_println_allowed  // should still compile but println is UB
```

### 5.2 Pipeline Edge Cases

```
- pipeline_run_nonexistent_file_errors
- pipeline_check_valid_passes
- pipeline_check_parse_error_fails
- pipeline_check_type_error_fails
- pipeline_build_creates_output_file
- pipeline_build_with_custom_output_path
- pipeline_init_creates_directory_structure
- pipeline_init_existing_dir_errors
- pipeline_find_package_root_from_nested_file
- pipeline_find_package_root_no_toml_returns_none
- pipeline_run_tests_no_tests_errors
- pipeline_run_tests_all_pass
- pipeline_run_tests_some_fail
- pipeline_run_tests_filter_single
- pipeline_run_tests_include_ignored
```

### 5.3 Dump Output Tests

Create `crates/glyim-cli/src/dump.rs` tests:

```
- dump_tokens_produces_output
- dump_tokens_empty_source
- dump_ast_produces_output
- dump_ast_with_errors_still_outputs_partial
- dump_hir_produces_output
- dump_hir_with_struct
- dump_hir_with_enum
```

---

## Phase 6: Fuzzing & Property Testing

**Goal**: Catch edge cases that human-written tests miss.
**Effort**: 2–3 days
**Priority**: 🟢 P2

### 6.1 Lexer Fuzzer

Create `fuzz/fuzz_lexer.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // Must not panic or hang on any input
    let tokens = glyim_lex::tokenize(data);
    // Verify invariants:
    // - Token offsets are monotonically non-decreasing
    // - Token text matches source slice
    // - No overlap between tokens
    let mut last_end = 0;
    for tok in &tokens {
        assert!(tok.start >= last_end, "token overlap at offset {}", tok.start);
        assert!(tok.end >= tok.start, "negative-length token");
        assert_eq!(&data[tok.start..tok.end], tok.text);
        last_end = tok.end;
    }
});
```

### 6.2 Parser Fuzzer

Create `fuzz/fuzz_parser.rs`:

```rust
fuzz_target!(|data: &str| {
    let result = glyim_parse::parse(data);
    // Must not panic on any input
    // Verify:
    // - errors.len() is finite
    // - AST items have valid spans (start <= end)
    // - Interner is consistent (all referenced symbols resolve)
    for item in &result.ast.items {
        validate_item_spans(item);
    }
});
```

### 6.3 Round-trip Property Tests

Using `proptest`:

```
- parse_then_lower_roundtrip  // parse → lower → verify all items present
- typeck_no_panic_on_any_hir  // generate arbitrary Hir, check() doesn't panic
- codegen_verify_passes_on_valid_hir  // generate valid HIR, verify() passes
```

### 6.4 Fuzz Directory Setup

```
fuzz/
├── Cargo.toml              # [dependencies] glyim-lex, glyim-parse, libfuzzer-sys
├── fuzz_lexer.rs
├── fuzz_parser.rs
└── corpora/
    ├── lexer/               # seed corpus
    └── parser/
```

Add to workspace `Cargo.toml` as an excluded member:
```toml
exclude = ["fuzz"]
```

---

## Phase 7: CI & Developer Experience

**Goal**: Make tests fast, reliable, and easy to run.
**Effort**: 1 day
**Priority**: 🟢 P2

### 7.1 CI Workflow Improvements

Update `.github/workflows/ci.yml`:

```yaml
jobs:
  check:
    steps:
      # ... existing ...
      - name: Test (unit only)
        run: cargo nextest run --workspace --lib --bins
      - name: Test (integration)
        run: cargo nextest run -p glyim-cli --test integration
      - name: Test (UI snapshots)
        run: cargo nextest run -p glyim-cli --test ui
      - name: Verify no unaccepted snapshots
        run: |
          git diff --exit-code crates/glyim-cli/tests/snapshots/
      - name: Test (package manager)
        run: cargo nextest run -p glyim-pkg
      - name: Check DAG
        run: just check-dag
      - name: Check tiers
        run: just check-tiers
      - name: File sizes
        run: python3 scripts/check_file_sizes.py
```

### 7.2 Nextest Partitioning

For faster CI, partition tests:

```toml
# .cargo/nextest.toml
[profile.default]
slow-timeout = "60s"

[[profile.default.overrides]]
filter = "test(e2e_)"
slow-timeout = "120s"  # E2E tests compile + link + run
retries = 0
```

### 7.3 Justfile Additions

```just
# Run only fast tests (unit + parser)
test-fast:
    cargo nextest run --workspace --lib --bins -p glyim-parse

# Run only slow tests (E2E, integration)
test-slow:
    cargo nextest run -p glyim-cli --test integration

# Verify all snapshots are reviewed (no pending .snap.new)
test-snapshots-check:
    #!/usr/bin/env bash
    find crates -name '*.snap.new' | grep . && { echo "ERROR: unaccepted snapshots found"; exit 1; }
    echo "✅ All snapshots accepted"

# Run tests with memory sanitizer (when available)
test-msan:
    cargo nextest run --workspace --target x86_64-unknown-linux-gnu

# Generate test coverage report
test-coverage:
    cargo llvm-cov nextest --workspace --lcov --output-path lcov.info
```

### 7.4 Test Naming Convention

Adopt a consistent naming scheme:

```
// Unit tests: <module>_<scenario>_<expected>
interner_deduplicates_equal_strings
typeck_unknown_field_pushes_error
codegen_struct_lit_valid_fields_ok

// Integration tests: e2e_<feature>_<scenario>
e2e_match_with_enum_patterns_exhaustive
e2e_struct_field_access_unknown_field_fails

// UI tests: ui_<error_category>_<specific>
ui_type_error_bool_mismatch
ui_parse_error_unterminated_string
ui_codegen_error_missing_main

// Snapshot tests: ir__<feature_description>
ir__struct_literal_with_two_fields
```

### 7.5 Test Count Tracking

Update `count-tests` recipe to show per-crate breakdown:

```just
count-tests:
    #!/usr/bin/env bash
    echo "Test count per crate:"
    echo "──────────────────────────────────────"
    for crate in crates/*/; do
        name=$(basename "$crate")
        count=$(cargo test -p "glyim-${name#glyim-}" -- --list 2>/dev/null | grep -c ' tests$' || echo 0)
        if [ "$count" -gt 0 ]; then
            printf "  %-30s %4d\n" "$name" "$count"
        fi
    done
```

---

## Appendix A: Proposed Directory Structure for New Tests

```
crates/
├── glyim-diag/src/
│   └── lib.rs                  # ADD: #[cfg(test)] mod tests { ... }
├── glyim-typeck/src/typeck/
│   └── tests.rs                # NEW: ~50 unit tests
├── glyim-hir/src/lower/
│   └── tests.rs                # NEW: ~45 lowering tests
├── glyim-codegen-llvm/
│   └── tests/
│       ├── ir_snapshots.rs      # NEW: ~15 IR snapshot tests
│       └── snapshots/           # NEW: insta snapshot dir
│           ├── ir__minimal_main.snap
│           ├── ir__arithmetic.snap
│           └── ...
├── glyim-cli/tests/
│   ├── integration.rs           # EXPAND: +40 e2e tests
│   ├── ui.rs                    # FIX: compile_stderr runs full pipeline
│   ├── ui/                      # EXPAND: +10 new .g files
│   │   ├── type_error_let_mismatch.g
│   │   ├── type_error_non_exhaustive.g
│   │   ├── missing_fn_body.g
│   │   └── ...
│   └── snapshots/               # UPDATE: all snapshots after harness fix
├── glyim-pkg/tests/
│   ├── manifest_edge_tests.rs   # NEW: ~20 tests
│   └── ...existing...           # EXPAND: +15 edge cases
└── fuzz/                        # NEW
    ├── Cargo.toml
    ├── fuzz_lexer.rs
    ├── fuzz_parser.rs
    └── corpora/
        ├── lexer/               # seed: keywords, operators, unicode, empty
        └── parser/              # seed: valid programs, error programs
```

---

## Appendix B: Implementation Order Checklist

- [ ] **Phase 1.1**: `glyim-diag` tests (30 min)
- [ ] **Phase 1.2**: `glyim-typeck` tests (4–6 hours)
- [ ] **Phase 1.3**: `glyim-hir/lower` tests (3–4 hours)
- [ ] **Phase 2.1**: Fix `compile_stderr` harness (1 hour)
- [ ] **Phase 2.2**: Update all UI snapshots (1 hour)
- [ ] **Phase 2.3**: Add new UI test cases (1–2 hours)
- [ ] **Phase 3.1**: IR snapshot tests (2 hours)
- [ ] **Phase 3.2**: Verification tests (1 hour)
- [ ] **Phase 3.3**: Runtime shims tests (30 min)
- [ ] **Phase 3.4**: Type-to-LLVM tests (30 min)
- [ ] **Phase 4.1**: Manifest edge tests (1 hour)
- [ ] **Phase 4.2**: Lockfile edge tests (30 min)
- [ ] **Phase 4.3**: Resolver edge tests (1 hour)
- [ ] **Phase 4.4**: CAS client edge tests (30 min)
- [ ] **Phase 4.5**: CLI lockfile integration tests (30 min)
- [ ] **Phase 5.1**: New E2E tests (3–4 hours)
- [ ] **Phase 5.2**: Pipeline edge tests (1 hour)
- [ ] **Phase 5.3**: Dump output tests (30 min)
- [ ] **Phase 6.1**: Lexer fuzzer (1 hour)
- [ ] **Phase 6.2**: Parser fuzzer (1 hour)
- [ ] **Phase 6.3**: Property tests (2 hours)
- [ ] **Phase 7.1**: CI workflow (30 min)
- [ ] **Phase 7.2**: Nextest config (15 min)
- [ ] **Phase 7.3**: Justfile additions (30 min)

**Total estimated effort**: 10–14 working days

---

## Appendix C: Metrics Targets

| Metric | Current | Target |
|--------|---------|--------|
| Total test count | ~237 | ~500 |
| Crates with zero coverage | 5 (`diag`, `typeck`, `macro-core`, `cas-server`, `syntax` partial) | 2 (`macro-core` stub, `cas-server` needs HTTP test infra) |
| Empty/broken UI snapshots | 6 | 0 |
| `#[ignore]` tests | 7 | 7 (keep ignored — tracked in issue tracker) |
| Fuzz targets | 0 | 2 |
| IR snapshot tests | 0 | ~15 |
| Test execution time (CI) | ~unknown | < 3 min for unit+parser, < 5 min full |
