use crate::lower_body;
use crate::tests::support::MockLowerCtx;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_span::Span;
use glyim_test::test_frozen_ty_ctx;
use glyim_typeck::thir;

#[test]
fn unsupported_literal_works() {
    let ctx = test_frozen_ty_ctx();
    let mock_ctx = MockLowerCtx::new(&ctx);
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let thir_body = thir::Body {
        owner,
        stmts: vec![],
        params: vec![],
        span: Span::DUMMY,
        return_ty: ctx.unit_ty(),
    };
    let result = lower_body(&mock_ctx, &thir_body);
    assert!(!result.diagnostics.iter().any(|d| d.is_error()));
}
