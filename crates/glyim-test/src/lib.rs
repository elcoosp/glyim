//! Crate root.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

/// annotations.
pub mod annotations;
/// assertions.
pub mod assertions;
/// comparison.
pub mod comparison;
/// error.
pub mod error;
/// fixtures.
pub mod fixtures;
/// harness.
pub mod harness;
/// mock.
pub mod mock;
/// phase.
pub mod phase;
/// property.
pub mod property;
/// snapshot.
pub mod snapshot;

pub use error::{AssertionFailure, FailureReason, TestDiscoveryError, TimeoutError};

pub use assertions::{
    MirAssert, TyAssert, TyCheck, assert_diag_code, assert_diag_contains, assert_error_count,
    assert_has_errors, assert_has_severity, assert_layout, assert_mir, assert_no_errors, assert_ty,
    check_ty,
};
pub use fixtures::{SourceBuilder, TyCtxBuilder, TyFactory};
pub use harness::{
    TestMode, TestPlan, TestRunner,
    runner::{OutputCheck, ProgramRunner, RunResult},
};
pub use mock::{MockBorrowckCtx, MockCodegen, MockLowerCtx, MockSolver, TestDbBuilder};
pub use phase::{AnalysisTester, CodegenTester, CompilationTrace, FrontendTester, MirGenTester};
pub use property::check_ty_property;
pub use snapshot::{snapshot_cst, snapshot_def_map, snapshot_mir, snapshot_mir_verbose};

use glyim_type::{TyCtx, TyCtxMut};

#[cfg(test)]
mod tests;

/// `test_ty_ctx`.
#[must_use]
pub fn test_ty_ctx() -> TyCtxMut {
    TyCtxBuilder::new().build_mut()
}

/// `test_frozen_ty_ctx`.
#[must_use]
pub fn test_frozen_ty_ctx() -> TyCtx {
    test_ty_ctx().freeze()
}

/// `with_fresh_ty_ctx`.
pub fn with_fresh_ty_ctx<F, R>(f: F) -> (TyCtx, R)
where
    F: FnOnce(&mut TyCtxMut) -> R,
{
    let mut ctx_mut = test_ty_ctx();
    let result = f(&mut ctx_mut);
    (ctx_mut.freeze(), result)
}
