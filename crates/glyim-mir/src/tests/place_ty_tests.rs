use glyim_core::arena::IndexVec;
use glyim_test::{assert_ty, with_fresh_ty_ctx};

#[test]
fn place_ty_deref_then_field_on_tuple() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        // Build substitution components first
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let bool_ty = ctx_mut.bool_ty();
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(bool_ty),
        ]);
        let inner_ty = ctx_mut.mk_ty(glyim_type::TyKind::Tuple(subst));
        let ref_ty = ctx_mut.mk_ref(
            glyim_type::Region::Erased,
            inner_ty,
            glyim_core::primitives::Mutability::Not,
        );
        let mut locals = IndexVec::new();
        locals.push(crate::LocalDecl {
            ty: ref_ty,
            mutability: glyim_core::primitives::Mutability::Not,
            source_info: crate::SourceInfo::new(glyim_span::Span::DUMMY),
        });
        crate::Body {
            owner: glyim_core::def_id::DefId::new(
                glyim_core::def_id::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(0),
            ),
            basic_blocks: IndexVec::new(),
            locals,
            arg_count: 0,
            return_ty: glyim_type::Ty::ERROR,
            span: glyim_span::Span::DUMMY,
            var_debug_info: Vec::new(),
        }
    });
    let place = crate::Place {
        local: crate::LocalIdx::from_raw(0),
        projection: Box::new([
            crate::ProjectionElem::Deref,
            crate::ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0)),
        ]),
    };
    let ty = place.ty(&ctx, &body.locals);
    let _ = assert_ty(&ctx, ty).is_int(glyim_core::primitives::IntTy::I32);
}

#[test]
fn place_ty_field_on_tuple() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i64_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I64));
        let bool_ty = ctx_mut.bool_ty();
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(bool_ty),
        ]);
        let tuple_ty = ctx_mut.mk_ty(glyim_type::TyKind::Tuple(subst));
        let mut locals = IndexVec::new();
        locals.push(crate::LocalDecl {
            ty: tuple_ty,
            mutability: glyim_core::primitives::Mutability::Not,
            source_info: crate::SourceInfo::new(glyim_span::Span::DUMMY),
        });
        crate::Body {
            owner: glyim_core::def_id::DefId::new(
                glyim_core::def_id::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(0),
            ),
            basic_blocks: IndexVec::new(),
            locals,
            arg_count: 0,
            return_ty: glyim_type::Ty::ERROR,
            span: glyim_span::Span::DUMMY,
            var_debug_info: Vec::new(),
        }
    });
    let place = crate::Place {
        local: crate::LocalIdx::from_raw(0),
        projection: Box::new([crate::ProjectionElem::Field(
            glyim_type::FieldIdx::from_raw(1),
        )]),
    };
    let ty = place.ty(&ctx, &body.locals);
    let _ = assert_ty(&ctx, ty).is_bool();
}
