use crate::lower::lower_crate;
use crate::{BodyId, Expr, ExprId, ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

fn get_body_hir(source: &str) -> (crate::CrateHir, Interner, BodyId) {
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    let body_id = match &hir.items[ItemId::from_raw(0)].kind {
        ItemKind::Fn(fn_item) => fn_item.body.expect("no body"),
        other => panic!("expected Fn item, got {other:?}"),
    };
    (hir, interner, body_id)
}

fn get_body(hir: &crate::CrateHir, body_id: BodyId) -> &crate::Body {
    &hir.bodies[body_id]
}

#[test]
fn test_break_continue_lowering() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { loop { break; continue; } }");
    let body = get_body(&hir, body_id);
    let block_id = ExprId::from_raw(body.exprs.len() as u32 - 1);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(loop_id),
            ..
        } => match &body.exprs[*loop_id] {
            Expr::Loop { body: b, .. } => match &body.exprs[*b] {
                Expr::Block { stmts, .. } => {
                    let mut saw_break = false;
                    let mut saw_continue = false;
                    for &sid in stmts {
                        match &body.exprs[sid] {
                            Expr::Break { .. } => saw_break = true,
                            Expr::Continue => saw_continue = true,
                            _ => {}
                        }
                    }
                    assert!(saw_break);
                    assert!(saw_continue);
                }
                _ => panic!(),
            },
            _ => panic!(),
        },
        _ => panic!(),
    }
}
