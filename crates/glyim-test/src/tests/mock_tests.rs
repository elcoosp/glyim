use crate::*;

#[test]
fn test_mock_solver() {
    use glyim_solve::TraitSolver;
    let mut solver = mock::MockSolver::new().respond_for_any(glyim_solve::SolverResult::Proven);
    assert_eq!(solver.call_count(), 0);

    let mut ctx_mut = test_ty_ctx();
    let substs = ctx_mut.intern_substitution(vec![]);
    let ctx = ctx_mut.freeze();

    let trait_pred = glyim_type::TraitPredicate {
        trait_ref: glyim_type::TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(0),
            substs,
        },
        polarity: glyim_type::ImplPolarity::Positive,
    };
    let result = solver.can_prove(&ctx, &trait_pred);
    assert!(matches!(result, glyim_solve::SolverResult::Proven));
    assert_eq!(solver.call_count(), 1);
}

#[test]
fn test_mock_solver_default_ambiguous() {
    use glyim_solve::TraitSolver;
    let mut solver = mock::MockSolver::new();

    let mut ctx_mut = test_ty_ctx();
    let substs = ctx_mut.intern_substitution(vec![]);
    let ctx = ctx_mut.freeze();

    let trait_pred = glyim_type::TraitPredicate {
        trait_ref: glyim_type::TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(99),
            substs,
        },
        polarity: glyim_type::ImplPolarity::Positive,
    };
    let result = solver.can_prove(&ctx, &trait_pred);
    assert!(matches!(result, glyim_solve::SolverResult::Ambiguous));
}

#[test]
fn test_mock_codegen() {
    use glyim_codegen::CodegenBackend;
    let mock = mock::MockCodegen::new();
    assert_eq!(mock.name(), "mock");
    assert_eq!(mock.calls().len(), 0);
    assert_eq!(mock.function_call_count(), 0);
}

#[test]
fn test_mock_codegen_generate() {
    use glyim_codegen::CodegenBackend;
    let mock = mock::MockCodegen::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(0),
    )));
    // `generate` now takes `(symbol, body)` pairs — the mangled FnDef
    // symbol the pipeline threads through for per-instantiation uniqueness
    // (see HANDOFF_SESSION_13_FINAL.md §2). This test doesn't exercise
    // generic instantiations, so the plain legacy form is fine.
    let body = body;
    let symbol = format!("__glyim_fn_{}", body.owner.local_id.to_raw());
    let result = mock.generate(&[(symbol, body)], std::path::Path::new("test.o"));
    assert!(result.is_ok());
    assert_eq!(mock.calls().len(), 1);
    assert_eq!(mock.calls()[0].body_count, 1);
}

#[test]
fn test_mock_codegen_generate_function() {
    use glyim_codegen::CodegenBackend;
    let mock = mock::MockCodegen::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(0),
    )));
    let result = mock.generate_function(&body);
    assert!(result.is_ok());
    assert_eq!(mock.function_call_count(), 1);
}

#[test]
fn test_test_db_builder() {
    use std::sync::Arc;
    let _db = mock::TestDbBuilder::new()
        .name("my_test")
        .target_triple("aarch64-unknown-linux-gnu")
        .opt_level(2)
        .file(
            std::path::PathBuf::from("main.g"),
            Arc::from("fn main() {}"),
        )
        .build();
}

#[test]
fn test_test_db_builder_default() {
    let _db = mock::TestDbBuilder::default().build();
}
