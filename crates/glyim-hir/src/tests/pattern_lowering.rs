use crate::lower::lower_crate;
use crate::{ItemId, ItemKind, Pat};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn test_pattern_or_lowering() {
    let source = "fn f(x: i32) { match x { 0 | 1 => 2, _ => 3 } }";
    let file_id = FileId::from_usize(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    let body_id = match &hir.items[ItemId::from_usize(0)].kind {
        ItemKind::Fn(fn_item) => fn_item.body.expect("no body"),
        _ => panic!("expected Fn item"),
    };
    let body = &hir.bodies[body_id];
    let mut found_or_pat = false;
    for (_id, expr) in body.exprs.iter_enumerated() {
        if let crate::Expr::Match { arms, .. } = expr
            && let Some(arm) = arms.first()
            && let Pat::Or(pats) = &body.pats[arm.pat]
            && pats.len() == 2
        {
            found_or_pat = true;
            break;
        }
    }
    assert!(found_or_pat, "Expected OR pattern with 2 alternatives");
}
