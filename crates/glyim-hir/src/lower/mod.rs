pub(crate) mod lower_expr;
pub(crate) mod lower_item;
pub(crate) mod lower_pat;
pub(crate) mod lower_type;
pub(crate) mod lower_async;

#[cfg(test)]
pub(crate) use lower_expr::{lower_expr, lower_literal};

use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::{Interner, Name};
use glyim_diag::GlyimDiagnostic;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

use crate::{Body, BodyId, CrateHir, Item, ItemId};

// ---------- helpers ----------

pub(crate) fn first_ident_text(node: &SyntaxNode) -> Option<String> {
    for el in node.children_with_tokens() {
        if let glyim_syntax::SyntaxElement::Token(t) = el
            && t.kind() == SyntaxKind::Ident
        {
            return Some(t.text().to_string());
        }
    }
    None
}

/// Like `first_ident_text` but descends into nested nodes (e.g. a closure
/// parameter `|n: i32|` is parsed as `Param -> PatIdent -> Ident`, so the
/// identifier is not a direct child of the `Param` node).
pub(crate) fn first_ident_text_with_depth(node: &SyntaxNode) -> Option<String> {
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t)
                if t.kind() == SyntaxKind::Ident
                    || t.kind() == SyntaxKind::KwSelf
                    || t.kind() == SyntaxKind::KwSuper
                    || t.kind() == SyntaxKind::KwCrate =>
            {
                return Some(t.text().to_string());
            }
            glyim_syntax::SyntaxElement::Node(n) => {
                if let Some(found) = first_ident_text_with_depth(&n) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn is_type_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::PathType
            | SyntaxKind::RefType
            | SyntaxKind::FnType
            | SyntaxKind::DynType
            | SyntaxKind::SliceType
            | SyntaxKind::ArrayType
            | SyntaxKind::TupleType
            | SyntaxKind::NeverType
            | SyntaxKind::InferType
    )
}

pub(crate) fn is_expr_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::Block
            | SyntaxKind::BinaryExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::PathExpr
            | SyntaxKind::LitExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::MethodCallExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::IndexExpr
            | SyntaxKind::UnaryExpr
            | SyntaxKind::RefExpr
            | SyntaxKind::MatchExpr
            | SyntaxKind::WhileExpr
            | SyntaxKind::LoopExpr
            | SyntaxKind::ForExpr
            | SyntaxKind::AssignExpr
            | SyntaxKind::BreakExpr
            | SyntaxKind::ContinueExpr
            | SyntaxKind::ReturnExpr
            | SyntaxKind::CastExpr
            | SyntaxKind::ClosureExpr
            | SyntaxKind::ArrayExpr
            | SyntaxKind::TupleExpr
            | SyntaxKind::StructExpr
            | SyntaxKind::RangeExpr
            | SyntaxKind::AwaitExpr
    )
}

pub(crate) fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    let lo = ByteIdx::from_raw(u32::from(range.start()));
    let hi = ByteIdx::from_raw(u32::from(range.end()));
    Span::new(FileId::from_raw(1), lo, hi, SyntaxContext::ROOT)
}

fn next_local_def_id(counter: &mut u32) -> LocalDefId {
    let id = *counter;
    *counter += 1;
    LocalDefId::from_raw(id)
}

/// Recursively walk the whole syntax tree for `ExternBlock` nodes and lower
/// each inner `fn` declaration into a top-level `FnDef` HIR item. This lets the
/// type-checker register a callable signature for `extern "C" { fn foo(); }`
/// imports regardless of where the block appears (module top level or nested
/// inside a function body). The def-map's `collect_extern_imports` registers
/// the same fns in the crate-root value namespace, so resolution by name aligns
/// the two and `check_path` can find the signature.
fn lower_extern_imports(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
    items: &mut IndexVec<ItemId, Item>,
) {
    for child in node.children() {
        if child.kind() == SyntaxKind::ExternBlock {
            for inner in child.children() {
                if inner.kind() == SyntaxKind::FnDef {
                    if let Some(item) = lower_item::lower_fn_def(
                        &inner,
                        interner,
                        local_def_counter,
                        item_id_counter,
                        bodies,
                        body_owners,
                        diags,
                        struct_field_map,
                    ) {
                        items.push(item);
                    }
                }
            }
        }
        // Recurse into every child so nested-in-body `extern` blocks are found.
        lower_extern_imports(
            &child,
            interner,
            local_def_counter,
            item_id_counter,
            bodies,
            body_owners,
            diags,
            struct_field_map,
            items,
        );
    }
}

// ---------- entry ----------

/// Lower the parsed AST into a `CrateHir`, running the `async fn` / `.await`
/// desugar (`lower_async`) so the resulting HIR is the future state-machine
/// shape the type-checker understands. Used by the real compile pipeline
/// (`lower_crate_for_pipeline`) and by the lowering unit tests.
pub(crate) fn lower_crate(
    root: &SyntaxNode,
    interner: &mut Interner,
    diags: &mut Vec<GlyimDiagnostic>,
) -> CrateHir {
    let mut hir = lower_crate_raw(root, interner, diags);
    lower_async::desugar_async(&mut hir, diags);
    hir
}

/// Raw syntax → HIR lowering WITHOUT the `async fn` / `.await` desugar. Used by
/// `lower_crate_for_pipeline` (and thus the plan §6.1 plumbing tests, which
/// assert the `async fn` keyword lowers to `FnItem { is_async: true }` before
/// desugaring rewrites the item into a synchronous future-returning wrapper).
pub(crate) fn lower_crate_raw(
    root: &SyntaxNode,
    interner: &mut Interner,
    diags: &mut Vec<GlyimDiagnostic>,
) -> CrateHir {
    let mut items = IndexVec::new();
    let mut bodies = IndexVec::new();
    let mut body_owners = IndexVec::new();
    let mut local_def_counter = 0u32;
    let mut item_id_counter = 0u32;

    // First pass: collect all struct definitions for field ordering
    let mut struct_field_map = std::collections::HashMap::new();
    for child in root.children() {
        if child.kind() == SyntaxKind::StructDef
            && let Some((name, fields)) = lower_item::collect_struct_fields(&child, interner)
        {
            struct_field_map.insert(name, fields);
        }
    }

    // Second pass: lower all items (fn bodies can now reorder fields)
    for child in root.children() {
        match child.kind() {
            SyntaxKind::FnDef => {
                if let Some(item) = lower_item::lower_fn_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                    &mut bodies,
                    &mut body_owners,
                    diags,
                    &struct_field_map,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::StructDef => {
                if let Some(item) = lower_item::lower_struct_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::EnumDef => {
                if let Some(item) = lower_item::lower_enum_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::ImplDef => {
                if let Some(item) = lower_item::lower_impl_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                    &mut bodies,
                    &mut body_owners,
                    diags,
                    &struct_field_map,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::TraitDef => {
                if let Some(item) = lower_item::lower_trait_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                    &mut bodies,
                    &mut body_owners,
                    diags,
                    &struct_field_map,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::ExternBlock => {
                // Handled by the full-tree `lower_extern_imports` scan below,
                // which catches both top-level and nested-in-body blocks.
            }
            SyntaxKind::Module => {
                if let Some(item) = lower_item::lower_mod_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                    &mut items,
                    &mut bodies,
                    &mut body_owners,
                    diags,
                    &struct_field_map,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::ConstDef => {
                if let Some(item) = lower_item::lower_const_def(
                    &child,
                    interner,
                    &mut local_def_counter,
                    &mut item_id_counter,
                    &mut bodies,
                    &mut body_owners,
                    diags,
                    &struct_field_map,
                ) {
                    items.push(item);
                }
            }
            SyntaxKind::TypeAlias => {
                if let Some(item) = lower_item::lower_type_alias(&child, interner, &mut item_id_counter) {
                    items.push(item);
                }
            }
            // Other item kinds (Trait, Use, Extern, etc.) are not yet lowered.
            _ => {}
        }
    }

    // Full-tree scan for `extern "C" { fn name(...); }` import blocks. The
    // second-pass item walk above only visits *top-level* module children, so
    // `extern` blocks nested inside function bodies are never seen there. We
    // lower every inner `fn` of every `ExternBlock` anywhere in the tree into a
    // top-level `FnDef` item. This mirrors the def-map's `collect_extern_imports`
    // pass (which registers those fns in the crate-root value namespace), so
    // the type-checker can resolve each by `item.name` and register its
    // signature — making `extern "C" { fn foo(); }` calls callable whether the
    // block is at module top level or inside a function body.
    lower_extern_imports(
        root,
        interner,
        &mut local_def_counter,
        &mut item_id_counter,
        &mut bodies,
        &mut body_owners,
        diags,
        &struct_field_map,
        &mut items,
    );

    CrateHir {
        items,
        bodies,
        body_owners,
        interner: interner.clone(),
    }
}
