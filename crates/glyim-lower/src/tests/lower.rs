use glyim_core::def_id::{AdtId, CrateId, DefId, FnDefId, LocalDefId};
use glyim_test::{assert_mir, test_frozen_ty_ctx};

struct LocalMockLowerCtx<'a> {
    ctx: &'a glyim_type::TyCtx,
}

impl<'a> LocalMockLowerCtx<'a> {
    fn new(ctx: &'a glyim_type::TyCtx) -> Self {
        Self { ctx }
    }
}

impl crate::LowerCtx for LocalMockLowerCtx<'_> {
    fn ty_ctx(&self) -> &glyim_type::TyCtx {
        self.ctx
    }
    fn adt_def(&self, _id: AdtId) -> crate::AdtDef {
        crate::AdtDef {
            variants: vec![],
            kind: crate::AdtKind::Struct,
        }
    }
    fn push_span(&self, _span: Span) {}
    fn pop_span(&self) {}
    fn hir_body(&self, _owner: glyim_core::def_id::LocalDefId) -> Option<&glyim_hir::Body> {
        None
    }
}

use crate::lower_body;
use glyim_core::primitives::*;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir::{self, LocalVarId};

fn dummy_thir() -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        span: Span::DUMMY,
        return_ty: Ty::UNIT,
        params: vec![],
        stmts: vec![],
    }
}

#[test]
fn s15_t01_lower_empty_function() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let result = lower_body(&lower_ctx, &dummy_thir());

    let _ = assert_mir(&ctx, &result.body)
        .block_count(1)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Return");
}

#[test]
fn s15_t02_lower_params() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();
    thir.params.push(thir::Param {
        name: glyim_core::interner::Interner::new().intern("x"),
        ty: Ty::BOOL,
        span: Span::DUMMY,
        pat: thir::Pattern {
            kind: thir::PatternKind::Wild,
            ty: Ty::BOOL,
            span: Span::DUMMY,
        },
        local: thir::LocalVarId::from_raw(0),
    });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body).local_count(2);
}

#[test]
fn s15_t03_lower_let_binding() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let init_expr = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Let {
        name: glyim_core::interner::Interner::new().intern("x"),
        ty: Ty::BOOL,
        pat: thir::Pattern {
            kind: thir::PatternKind::Wild,
            ty: Ty::BOOL,
            span: Span::DUMMY,
        },
        init: Some(init_expr),
        span: Span::DUMMY,
    });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Return");
}

#[test]
fn s15_t04_lower_return() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let ret_expr = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Return {
        value: Some(ret_expr),
        span: Span::DUMMY,
    });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Return");
}

#[test]
fn s15_t05_lower_binary_op() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let lhs = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    let rhs = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(false)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    let bin_expr = thir::Expr {
        kind: thir::ExprKind::Binary {
            op: BinOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Expr { expr: bin_expr });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Return");
}

#[test]
fn s15_t06_lower_if_else() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let cond = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    let then_branch = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Unit),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    let if_expr = thir::Expr {
        kind: thir::ExprKind::If {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch: None,
        },
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Expr { expr: if_expr });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "SwitchInt");
}

#[test]
fn s15_t07_lower_function_call() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let func = thir::Expr {
        kind: thir::ExprKind::FnRef(FnDefId::from_raw(0)),
        ty: Ty::ERROR,
        span: Span::DUMMY,
    };

    let call_expr = thir::Expr {
        kind: thir::ExprKind::Call {
            func: Box::new(func),
            args: vec![],
        },
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Expr { expr: call_expr });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Call");
}

#[test]
fn s15_t08_lower_reference() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let var_ref = thir::Expr {
        kind: thir::ExprKind::VarRef(LocalVarId::from_raw(1)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    let ref_expr = thir::Expr {
        kind: thir::ExprKind::Ref {
            mutability: Mutability::Not,
            operand: Box::new(var_ref),
        },
        ty: Ty::ERROR,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Expr { expr: ref_expr });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body);
}

#[test]
fn s15_t09_lower_assign() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let lhs = thir::Expr {
        kind: thir::ExprKind::VarRef(LocalVarId::from_raw(1)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    let rhs = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Assign {
        lhs,
        rhs,
        span: Span::DUMMY,
    });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body).block_count(1);

    let block = &result.body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)];
    assert_eq!(block.statements.len(), 1);
    assert!(matches!(
        block.statements[0].kind,
        glyim_mir::StatementKind::Assign(..)
    ));
}

#[test]
fn s15_t10_lower_unary_op() {
    let ctx = test_frozen_ty_ctx();
    let lower_ctx = LocalMockLowerCtx::new(&ctx);
    let mut thir = dummy_thir();

    let operand = thir::Expr {
        kind: thir::ExprKind::Literal(thir::Literal::Bool(true)),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    let unary_expr = thir::Expr {
        kind: thir::ExprKind::Unary {
            op: UnOp::Not,
            operand: Box::new(operand),
        },
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };

    thir.stmts.push(thir::Stmt::Expr { expr: unary_expr });

    let result = lower_body(&lower_ctx, &thir);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
