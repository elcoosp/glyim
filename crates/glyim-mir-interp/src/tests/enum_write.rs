//! MIR-17 regression: writing an enum field must skip the discriminant tag.
//!
//! Enums are laid out as `[tag, ...payload]`. The read path offsets a `Field`
//! projection by `+1` when the base is an enum; the write path did NOT, so
//! `(e as B).0 = v` executed `fields[0] = v` — overwriting the variant tag
//! with payload data, after which a match dispatched to a garbage variant.

use glyim_core::{AdtId, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::adt_def::{AdtKind, VariantStyle};
use glyim_type::{AdtDef, FieldDef, TyKind, VariantDef};

use crate::{InterpValue, Interpreter};

fn dummy_owner() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}
fn dummy_si() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

#[test]
fn enum_field_write_preserves_discriminant() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    // Register an enum `E { A(i32), B(i32) }`.
    let adt_id = AdtId::from_raw(1000);
    let mk_fields = |ctx: &mut glyim_type::TyCtxMut| {
        IndexVec::from_raw(vec![FieldDef {
            name: ctx.resolver().intern("0"),
            ty: i32_ty,
        }])
    };
    let (f0, f1) = (mk_fields(&mut ctx_mut), mk_fields(&mut ctx_mut));
    let (n0, n1) = (ctx_mut.resolver().intern("A"), ctx_mut.resolver().intern("B"));
    let adt_def = AdtDef {
        kind: AdtKind::Enum,
        fields: IndexVec::new(),
        variants: vec![
            VariantDef { name: n0, fields: f0, style: VariantStyle::Tuple },
            VariantDef { name: n1, fields: f1, style: VariantStyle::Tuple },
        ],
        generic_params: vec![],
    };
    ctx_mut.register_adt(adt_id, adt_def);
    let empty_subst = ctx_mut.intern_substitution(vec![]);
    let enum_ty = ctx_mut.mk_ty(TyKind::Adt(adt_id, empty_subst));
    let tcx = ctx_mut.freeze();

    // local 1: the enum value `E::A(10)` = `[tag=0, payload=10]`.
    let mut body = Body::dummy(dummy_owner());
    let ev = body.locals.push(LocalDecl {
        ty: enum_ty,
        mutability: Mutability::Mut,
        source_info: dummy_si(),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(ev),
                Rvalue::Aggregate(
                    AggregateKind::Adt(adt_id, VariantIdx::from_raw(0), tcx.intern_substitution(vec![])),
                    vec![Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })],
                ),
            ),
            source_info: dummy_si(),
        });

    // `Body::dummy`'s bb0 ends in `Unreachable`; terminate it with `Return`.
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_si(),
    };

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    // Write payload field 0 = 42 through a `Field(0)` projection. Because the
    // base is an enum, this must hit element 1 (past the tag).
    interp
        .write_place(
            &Place {
                local: ev,
                projection: Box::new([ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0))]),
            },
            InterpValue::Int(42),
        )
        .unwrap();

    let v = interp.get_local_value(ev).unwrap();
    match v {
        InterpValue::Aggregate(fields) => {
            assert_eq!(
                fields[0],
                InterpValue::Int(0),
                "the discriminant tag must be preserved (MIR-17); got {fields:?}"
            );
            assert_eq!(
                fields[1],
                InterpValue::Int(42),
                "the payload field 0 must be updated; got {fields:?}"
            );
        }
        other => panic!("expected an enum aggregate, got {other:?}"),
    }
}
