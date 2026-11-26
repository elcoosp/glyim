use crate::*;
use glyim_core::interner::Interner;
use glyim_span::Span;
use glyim_type::Ty;

fn make_name(s: &str) -> glyim_core::interner::Name {
    let interner = Interner::new();
    interner.intern(s)
}

#[test]
fn var_debug_info_place() {
    let name = make_name("x");
    let info = VarDebugInfo {
        name,
        value: VarDebugInfoValue::Place(Place::new(LocalIdx::from_usize(1))),
    };
    assert!(matches!(info.value, VarDebugInfoValue::Place(_)));
}

#[test]
fn var_debug_info_const() {
    let name = make_name("y");
    let info = VarDebugInfo {
        name,
        value: VarDebugInfoValue::Const(MirConst {
            kind: MirConstKind::Int(42),
            ty: Ty::BOOL,
            span: Span::DUMMY,
        }),
    };
    assert!(matches!(info.value, VarDebugInfoValue::Const(_)));
}

#[test]
fn body_with_var_debug_info() {
    let mut body = Body::dummy(glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_usize(0),
        glyim_core::def_id::LocalDefId::from_usize(0),
    ));

    let name_x = make_name("x");
    let name_y = make_name("y");

    body.var_debug_info.push(VarDebugInfo {
        name: name_x,
        value: VarDebugInfoValue::Place(Place::new(LocalIdx::from_usize(1))),
    });
    body.var_debug_info.push(VarDebugInfo {
        name: name_y,
        value: VarDebugInfoValue::Const(MirConst {
            kind: MirConstKind::Bool(true),
            ty: Ty::BOOL,
            span: Span::DUMMY,
        }),
    });

    assert_eq!(body.var_debug_info.len(), 2);
    assert!(matches!(
        body.var_debug_info[0].value,
        VarDebugInfoValue::Place(_)
    ));
    assert!(matches!(
        body.var_debug_info[1].value,
        VarDebugInfoValue::Const(_)
    ));
}
