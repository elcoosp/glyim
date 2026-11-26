//! Typed-body boundary bench (plan M5.2 seam).
//!
//! Dependency-free (`harness = false`), mirroring the other seam benches:
//! measures the per-body cost of the checked [`TypedBody::try_from_body`]
//! constructor — the signature-slot verification walk that makes an
//! unverified THIR body unrepresentable at the lowering boundary — against
//! the baseline of constructing the same body without verification. The
//! invariant walk must stay negligible next to type checking and lowering,
//! or the M5.2 seam costs more than it buys. Run with:
//!
//! ```text
//! cargo bench -p glyim-typeck --bench typed_body_ctor
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::interner::Interner;
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut};
use glyim_typeck::TypedBody;
use glyim_typeck::thir;

const BODIES: u32 = 1 << 14;
const STMTS_PER_BODY: usize = 8;

/// `fn f(x: i32) { <STMTS_PER_BODY statement exprs>; }` with every slot
/// concrete.
fn resolved_body(name: glyim_core::interner::Name, return_ty: Ty) -> thir::Body {
    let stmts = (0..STMTS_PER_BODY)
        .map(|i| thir::Stmt::Expr {
            expr: thir::Expr {
                kind: thir::ExprKind::Literal(thir::Literal::Int(i as i128, None)),
                ty: Ty::I32,
                span: Span::DUMMY,
            },
        })
        .collect();
    thir::Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        params: vec![thir::Param {
            name,
            ty: Ty::I32,
            span: Span::DUMMY,
            pat: thir::Pattern {
                kind: thir::PatternKind::Wild,
                ty: Ty::I32,
                span: Span::DUMMY,
            },
            local: thir::LocalVarId::from_usize(0),
        }],
        return_ty,
        stmts,
        span: Span::DUMMY,
    }
}

fn main() {
    let interner = Interner::new();
    let name = interner.intern("x");
    let ctx = TyCtxMut::new(interner);
    let frozen = ctx.freeze();

    // Warm-up pass so allocator and interner reach steady state.
    for _ in 0..1024u32 {
        let body = resolved_body(name, Ty::I32);
        black_box(TypedBody::try_from_body(body, &frozen).is_ok());
    }

    let start = Instant::now();
    for _ in 0..BODIES {
        let body = black_box(resolved_body(name, Ty::I32));
        black_box(TypedBody::try_from_body(body, &frozen).is_ok());
    }
    let checked = start.elapsed();

    let start = Instant::now();
    for _ in 0..BODIES {
        black_box(resolved_body(name, Ty::I32));
    }
    let baseline = start.elapsed();

    let per_body_checked_ns = checked.as_nanos() / u128::from(BODIES);
    let per_body_baseline_ns = baseline.as_nanos() / u128::from(BODIES);
    let verification_ns = per_body_checked_ns.saturating_sub(per_body_baseline_ns);
    println!(
        "typed_body ctor: {per_body_checked_ns} ns/body (baseline {per_body_baseline_ns} \
         ns/body, verification approx {verification_ns} ns/body) over {STMTS_PER_BODY} \
         statements — the M5.2 signature walk must stay negligible next to typeck"
    );
    assert!(
        per_body_checked_ns < 100_000,
        "the checked constructor regressed past 100 µs/body: {per_body_checked_ns} ns"
    );
}
