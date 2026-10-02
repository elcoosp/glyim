use glyim_core::primitives::{IntTy, UintTy};
use glyim_mir::TerminatorKind;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Const, ConstKind, ParamConst, TyKind, substitution::GenericArg};

#[test]
fn drop_glue_for_i32_generates_body() {
    let (ty_ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let body = crate::mono_cache::generate_drop_glue(ty, &ty_ctx);
    // Just verify the body has at least one block (no panic)
    assert!(!body.basic_blocks.is_empty());
}

#[test]
fn drop_glue_for_bool_generates_body() {
    let (ty_ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let body = crate::mono_cache::generate_drop_glue(ty, &ty_ctx);
    assert!(!body.basic_blocks.is_empty());
}

#[test]
fn drop_glue_for_array_of_droppable_drops_each_element() {
    // `[String; 5]`: each element needs drop, so the generated glue must contain
    // exactly 5 `Drop` terminators (one per element index), not a bare `Return`.
    // This is the de-stubbing-plan §16.1 fix — previously arrays produced a
    // single `Return` and silently skipped every element's destructor.
    let (ty_ctx, ty) = with_fresh_ty_ctx(|c| {
        let string_ty = c.mk_ty(TyKind::String);
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(5),
            ty: usize_ty,
        };
        c.mk_ty(TyKind::Array(string_ty, len))
    });
    let body = crate::mono_cache::generate_drop_glue(ty, &ty_ctx);

    let mut drop_terminators = 0usize;
    let mut indexed_drops = 0usize;
    for bb in body.basic_blocks.iter() {
        if let TerminatorKind::Drop { place, .. } = &bb.terminator.kind {
            drop_terminators += 1;
            if place
                .projection
                .iter()
                .any(|p| matches!(p, glyim_mir::ProjectionElem::Index(_)))
            {
                indexed_drops += 1;
            }
        }
    }
    assert_eq!(
        drop_terminators, 5,
        "expected 5 Drop terminators (one per element)"
    );
    assert_eq!(
        indexed_drops, 5,
        "each Drop must target a distinct array element"
    );
}

/// Phase 2 (GLYIM_DESTUB_PLAN): const-generic array length must be resolved
/// *before* drop-glue generation, and the substitution must flow all the way
/// into `generate_drop_glue` so the right number of per-element `Drop`
/// terminators are produced.
///
/// This is the end-to-end version of the unit-level
/// `subst_ty_substitutes_const_param_array_length` test: it starts from a
/// `[String; N]` whose length is a `ConstKind::Param` (exactly the shape
/// monomorphization hands to the drop-glue generator for a
/// `struct Buf<const N: usize>([Bar; N])`), runs it through `TyCtx::subst_ty`
/// with `N := 3`, and asserts the generator emits the resolved count of element
/// drops u2014 not the old "single `Return`, silently skip every destructor"
/// behaviour (de-stubbing plan u00a716.1).
#[test]
fn const_generic_array_drop_glue_resolves_length_through_substitution() {
    let (ty_ctx, arr_with_param_len) = with_fresh_ty_ctx(|c| {
        let string_ty = c.mk_ty(TyKind::String);
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let param_n = Const {
            kind: ConstKind::Param(ParamConst {
                index: 0,
                name: c.resolver().intern("N"),
            }),
            ty: usize_ty,
        };
        c.mk_ty(TyKind::Array(string_ty, param_n))
    });

    // Monomorphize the length: N := 3 (mirrors what polymorphize's
    // `mark_used_params` + `subst_ty` do after resolving the const arg).
    let concrete_len = Const {
        kind: ConstKind::Int(3),
        ty: ty_ctx.bool_ty(),
    };
    let mut tcx = ty_ctx.to_mut();
    let mut subst = std::collections::HashMap::new();
    subst.insert(0u32, GenericArg::Const(concrete_len.clone()));
    let monomorphic_arr = tcx.subst_ty(arr_with_param_len, &subst);

    // The substituted type must carry the concrete length `3`.
    match tcx.ty_kind(monomorphic_arr) {
        TyKind::Array(_, len) => {
            assert_eq!(
                len.kind,
                ConstKind::Int(3),
                "subst_ty must resolve the const-parameter length to 3"
            );
        }
        other => panic!("expected Array after substitution, got {other:?}"),
    }

    // Generating drop glue for the monomorphic `[String; 3]` must produce
    // exactly 3 per-element `Drop` terminators (no panic, no silent skip).
    let body = crate::mono_cache::generate_drop_glue(monomorphic_arr, &ty_ctx);
    let mut drop_terminators = 0usize;
    let mut indexed_drops = 0usize;
    for bb in body.basic_blocks.iter() {
        if let TerminatorKind::Drop { place, .. } = &bb.terminator.kind {
            drop_terminators += 1;
            if place
                .projection
                .iter()
                .any(|p| matches!(p, glyim_mir::ProjectionElem::Index(_)))
            {
                indexed_drops += 1;
            }
        }
    }
    assert_eq!(
        drop_terminators, 3,
        "expected 3 Drop terminators (one per element) after const substitution"
    );
    assert_eq!(
        indexed_drops, 3,
        "each Drop must target a distinct array element after substitution"
    );
}

/// Phase 2c (GLYIM_DESTUB_PLAN): `generate_array_drop_glue` must *panic* (hard
/// assertion) when it receives a non-monomorphic array length, instead of the
/// old silent "skip every destructor" fallback. This is the regression guard
/// that turns "leaks memory in release" into "caught immediately by CI."
#[test]
#[should_panic(expected = "internal error: array drop glue requested")]
fn const_generic_array_drop_glue_panics_on_unresolved_length() {
    let (ty_ctx, arr_with_param_len) = with_fresh_ty_ctx(|c| {
        let string_ty = c.mk_ty(TyKind::String);
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let param_n = Const {
            kind: ConstKind::Param(ParamConst {
                index: 0,
                name: c.resolver().intern("N"),
            }),
            ty: usize_ty,
        };
        c.mk_ty(TyKind::Array(string_ty, param_n))
    });

    // NOTE: intentionally NOT substituted u2014 the length is still a `Param`.
    // `generate_drop_glue` must refuse to silently emit a no-op, per u00a72c.
    let _ = crate::mono_cache::generate_drop_glue(arr_with_param_len, &ty_ctx);
}

/// INF-23 regression: the `[T]` (slice) drop glue loop must be a real
/// terminating loop that drops each element. The old code put `idx = 0` in the
/// header and switched on the raw `idx` with `(0, exit)`, so the first
/// evaluation exited immediately and **zero** elements were dropped; its latch
/// also jumped back to a header that re-zeroed `idx` (non-terminating).
///
/// This checks the *structure*: the header compares `idx < len` and switches
/// on that bool (true -> body), the body drops an indexed element, and the
/// latch increments `idx` and jumps back to the header.
#[test]
fn slice_drop_glue_loop_drops_each_element() {
    let (ty_ctx, ty) = with_fresh_ty_ctx(|c| {
        let string_ty = c.mk_ty(TyKind::String);
        // `[String]` — a slice whose element needs drop.
        c.mk_ty(TyKind::Slice(string_ty))
    });
    let body = crate::mono_cache::generate_drop_glue(ty, &ty_ctx);

    // 1. A block must compute `cond = idx < len`.
    let has_lt = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|s| {
            matches!(
                &s.kind,
                glyim_mir::StatementKind::Assign(
                    _,
                    glyim_mir::Rvalue::BinaryOp(
                        glyim_core::primitives::BinOp::Lt,
                        _
                    )
                )
            )
        })
    });
    assert!(has_lt, "slice drop glue must compare `idx < len` (INF-23)");

    // 2. A block must switch on a bool with `true (1) -> body` (not exit).
    let switch = body.basic_blocks.iter().find_map(|bb| {
        if let TerminatorKind::SwitchInt {
            switch_ty, targets, ..
        } = &bb.terminator.kind
            && *switch_ty == glyim_type::Ty::BOOL
        {
            return Some(targets.clone());
        }
        None
    });
    let targets = switch.expect("slice drop glue must switch on the loop condition");
    let true_target = targets
        .iter()
        .find(|(v, _)| *v == 1)
        .map(|(_, t)| t)
        .expect("loop condition must have a `true` (1) branch");

    // 3. The true branch must be a block that drops an *indexed* element.
    let body_bb = &body.basic_blocks[true_target];
    match &body_bb.terminator.kind {
        TerminatorKind::Drop { place, .. } => {
            assert!(
                place
                    .projection
                    .iter()
                    .any(|p| matches!(p, glyim_mir::ProjectionElem::Index(_))),
                "loop body must drop an indexed element"
            );
        }
        other => panic!("true branch of the loop condition must be a Drop, got {other:?}"),
    }

    // 4. Some block must increment (`idx + 1`) and Goto back into the loop.
    let has_increment = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|s| {
            matches!(
                &s.kind,
                glyim_mir::StatementKind::Assign(
                    _,
                    glyim_mir::Rvalue::BinaryOp(glyim_core::primitives::BinOp::Add, _)
                )
            )
        }) && matches!(bb.terminator.kind, TerminatorKind::Goto { .. })
    });
    assert!(
        has_increment,
        "slice drop glue must have an `idx = idx + 1` latch that loops back"
    );
}
