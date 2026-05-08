use crate::elab::{ElabContext, synth::synth_expr};
use crate::ty::{Ty, TyKind};
use glyim_hir::HirExpr;

pub fn check_expr(ctx: &mut ElabContext, expr: &HirExpr, expected: Ty) {
    // Collect errors into a temporary vec, then push to ctx after
    let mut temp_errors: Vec<crate::diagnostics::TypeError> = Vec::new();

    if matches!(ctx.arena.get(expected), &TyKind::Error) {
        let synth = synth_expr(ctx, expr);
        let _ = ctx.unification.unify(
            ctx.arena,
            synth,
            expected,
            glyim_diag::Span::new(0, 0),
            &mut |e| temp_errors.push(e),
        );
        ctx.errors.extend(temp_errors);
        return;
    }

    let synth = synth_expr(ctx, expr);
    let _ = ctx.unification.unify(
        ctx.arena,
        synth,
        expected,
        glyim_diag::Span::new(0, 0),
        &mut |e| temp_errors.push(e),
    );
    ctx.errors.extend(temp_errors);
}
