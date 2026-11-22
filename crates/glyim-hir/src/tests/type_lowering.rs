use crate::lower::lower_crate;
use crate::{ItemId, ItemKind, TypeRef};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn test_type_ref_slice() {
    let source = "fn f() -> [i32] { unimplemented!() }";
    let file_id = FileId::from_usize(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    let ty = match &hir.items[ItemId::from_raw(0)].kind {
        ItemKind::Fn(fn_item) => fn_item.return_ty.clone().unwrap(),
        _ => panic!("expected Fn"),
    };
    match ty {
        TypeRef::Slice(inner) => match *inner {
            TypeRef::Path(p) => assert_eq!(p.as_name().unwrap(), interner.intern("i32")),
            _ => panic!(),
        },
        _ => panic!(),
    }
}
