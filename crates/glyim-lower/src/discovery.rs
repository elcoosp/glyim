use crate::mono::MonoItem;
use glyim_core::def_id::{FnDefId, LocalDefId, StaticDefId};
use glyim_def_map::CrateDefMap;
use glyim_diag::{DiagSeverity, GlyimDiagnostic, MultiSpan};
use glyim_hir::{CrateHir, ItemKind};
use glyim_span::Span;
use glyim_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use glyim_type::TyCtxMut;

/// Discover root mono items from a crate.
pub fn discover_mono_roots(
    root: &SyntaxNode,
    hir: &CrateHir,
    def_map: Option<&CrateDefMap>,
    ctx: &mut TyCtxMut,
) -> (Vec<MonoItem>, Vec<GlyimDiagnostic>) {
    let mut items = Vec::new();
    let mut diags = Vec::new();

    // Resolve a root item's `LocalDefId` from the def-map value namespace,
    // which is the same id space typeck uses to register fn sigs/bodies and
    // that `check_path` resolves value paths to. When no def-map is supplied
    // or it has no entry (e.g. synthetic test harnesses), fall back to the
    // HIR `item.id`, preserving the historical discovery behaviour.
    let resolve_root_id = |name: glyim_core::interner::Name| -> Option<LocalDefId> {
        def_map
            .and_then(|dm| dm.modules.get(dm.root))
            .and_then(|m| m.scope.values.get(&name))
            .map(|(id, _, _)| *id)
    };

    for item in hir.items.iter() {
        match &item.kind {
            ItemKind::Fn(_fn_item) => {
                let item_name = ctx.name_str(item.name);
                let has_main_name = item_name == "main";
                let has_start_attr = has_attr_in_span(root, item.span, "start");
                let has_no_mangle = has_attr_in_span(root, item.span, "no_mangle");

                if has_main_name || has_start_attr || has_no_mangle {
                    let local_def_id = resolve_root_id(item.name)
                        .unwrap_or_else(|| LocalDefId::from_raw(item.id.to_raw()));
                    let fn_def_id = FnDefId::from_raw(local_def_id.to_raw());

                    let empty_subst = ctx.intern_substitution(Vec::new());
                    let mono_item = MonoItem::Fn {
                        def_id: fn_def_id,
                        substs: empty_subst,
                    };
                    items.push(mono_item);
                }
            }
            ItemKind::Static(_static_item) if has_attr_in_span(root, item.span, "used") => {
                let local_def_id = resolve_root_id(item.name)
                    .unwrap_or_else(|| LocalDefId::from_raw(item.id.to_raw()));
                let static_def_id = StaticDefId::from_raw(local_def_id.to_raw());

                let mono_item = MonoItem::Static {
                    def_id: static_def_id,
                };
                items.push(mono_item);
            }
            _ => {}
        }
    }

    if items.is_empty() {
        let warn = GlyimDiagnostic::new(
            glyim_diag::ErrorCode {
                category: glyim_diag::ErrorCategory::Internal,
                number: 0,
            },
            DiagSeverity::Warning,
            "no entry points found in crate (no `main`, `#[start]`, `#[no_mangle]`, or `#[used]` static)"
                .to_string(),
            MultiSpan::from_span(Span::DUMMY),
        );
        diags.push(warn);
    }

    (items, diags)
}

/// Check if a `#` `[...]` attribute with the given name exists within the given span in the syntax tree.
fn has_attr_in_span(root: &SyntaxNode, target_span: Span, attr_name: &str) -> bool {
    let mut tokens = Vec::new();
    collect_tokens(root, &mut tokens);

    let mut i = 0;
    while i + 3 < tokens.len() {
        let t0 = &tokens[i];
        if t0.kind() == SyntaxKind::Hash
            && let (Some(t1), Some(t2), Some(t3)) =
                (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3))
            && t1.kind() == SyntaxKind::LBracket
            && t2.kind() == SyntaxKind::Ident
            && t3.kind() == SyntaxKind::RBracket
        {
            let ident_text = t2.text();
            if ident_text == attr_name {
                // Check that this attribute is within the item span
                let attr_span = token_span(t0);
                if span_intersects(target_span, attr_span) {
                    return true;
                }
            }
        }
        i += 1;
    }

    false
}

fn collect_tokens(node: &SyntaxNode, out: &mut Vec<SyntaxToken>) {
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            out.push(token.clone());
        } else if let Some(child) = elem.as_node() {
            collect_tokens(child, out);
        }
    }
}

fn token_span(tok: &SyntaxToken) -> Span {
    let range = tok.text_range();
    Span::new(
        glyim_span::FileId::BOGUS,
        glyim_span::ByteIdx::from_raw(range.start().into()),
        glyim_span::ByteIdx::from_raw(range.end().into()),
        glyim_span::SyntaxContext::ROOT,
    )
}

fn span_intersects(a: Span, b: Span) -> bool {
    a.lo <= b.hi && b.lo <= a.hi
}
