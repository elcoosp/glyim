use glyim_core::interner::{Interner, Name};
use glyim_core::path::PathKind;
use glyim_core::primitives::*;
use glyim_diag::GlyimDiagnostic;
use glyim_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::HashMap;
use crate::TypeRef;

/// Checks if a syntax node kind represents a pattern.
/// This is an exhaustive match to ensure new pattern kinds are added here explicitly.
pub(crate) fn is_pattern(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PatIdent
            | SyntaxKind::PatWild
            | SyntaxKind::PatLit
            | SyntaxKind::PatRange
            | SyntaxKind::PatTuple
            | SyntaxKind::PatStruct
            | SyntaxKind::PatOr
            | SyntaxKind::PatSlice
    )
}

use crate::{
    Body, Expr, ExprId, Literal, MatchArm, Pat, PatId, Path as HirPath, PathSegment, Span,
};

use super::{
    first_ident_text, is_expr_node, is_type_node, lower_item::lower_param, lower_pat::lower_pat,
    lower_type::lower_type_ref, node_span,
};

pub(crate) fn lower_block_to_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> ExprId {
    let mut stmts = Vec::new();
    let mut pending: Option<ExprId> = None;
    let mut last_has_semi = false;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::ExprStmt => {
                let has_semi = child.children_with_tokens().any(|el| {
                    matches!(el, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Semicolon)
                });
                let mut chain_base: Option<ExprId> = None;
                for inner in child.children() {
                    if !is_expr_node(&inner)
                        && inner.kind() != SyntaxKind::Block
                        && inner.kind() != SyntaxKind::MacroCall
                    {
                        continue;
                    }
                    if (inner.kind() == SyntaxKind::FieldExpr
                        || inner.kind() == SyntaxKind::MethodCallExpr)
                        && let Some(base_id) = chain_base
                    {
                        if let Some(id) = lower_field_or_method_with_receiver(
                            &inner,
                            base_id,
                            interner,
                            body,
                            diags,
                            struct_field_map,
                        ) {
                            chain_base = Some(id);
                        }
                        continue;
                    }
                    let current = lower_expr(&inner, interner, body, diags, struct_field_map);
                    if let Some(id) = current {
                        if let Some(prev) = chain_base.take() {
                            stmts.push(prev);
                        } else if let Some(prev) = pending.take() {
                            stmts.push(prev);
                        }
                        chain_base = Some(id);
                    }
                }
                if let Some(base_id) = chain_base.take() {
                    pending = Some(base_id);
                    last_has_semi = has_semi;
                }
            }
            SyntaxKind::LetStmt => {
                let mut pat_node = None;
                let mut expr_node = None;
                for inner in child.children() {
                    if is_expr_node(&inner) || inner.kind() == SyntaxKind::Block {
                        expr_node = Some(inner.clone());
                    } else if is_pattern(inner.kind()) {
                        pat_node = Some(inner);
                    }
                }
                if let (Some(pat), Some(rhs)) = (pat_node, expr_node.clone())
                    && let Some(pat_id) = lower_pat(&pat, interner, &mut body.pats, diags)
                {
                    let rhs_expr_id = lower_expr(&rhs, interner, body, diags, struct_field_map);
                    if let Some(rhs_id) = rhs_expr_id {
                        let let_expr = Expr::Let {
                            pat: pat_id,
                            value: rhs_id,
                        };
                        let let_id = body.alloc_expr(let_expr, node_span(&child));
                        if let Some(prev) = pending.take() {
                            stmts.push(prev);
                        }
                        stmts.push(let_id);
                        pending = None;
                        last_has_semi = true;
                        continue;
                    }
                }
                if let Some(rhs) = expr_node {
                    if let Some(prev) = pending.take() {
                        stmts.push(prev);
                    }
                    pending = lower_expr(&rhs, interner, body, diags, struct_field_map);
                    last_has_semi = true;
                }
            }
            _ => unreachable!(
                "parser produced a Block with unrecognized child kind: {:?}",
                child.kind()
            ),
        }
    }

    let tail = if last_has_semi {
        if let Some(last) = pending.take() {
            stmts.push(last);
        }
        None
    } else {
        pending.take()
    };

    let expr = Expr::Block { stmts, tail };

    
    body.alloc_expr(expr, node_span(node))
}

/// Convert a pattern into an expression (for LHS of assignment). Kept for the
/// planned assignment-desugaring tier; not yet wired into lowering.
#[allow(dead_code)]
fn pat_to_expr(
    pat_id: PatId,
    body: &mut Body,
    _interner: &mut Interner,
    span: Span,
) -> Option<ExprId> {
    match &body.pats[pat_id] {
        Pat::Wild => None,
        Pat::Binding { name, .. } => {
            let path = HirPath::from_single(*name);
            let expr = Expr::Path(path);
            Some(body.alloc_expr(expr, span))
        }
        Pat::Path(path) => {
            let expr = Expr::Path(path.clone());
            Some(body.alloc_expr(expr, span))
        }
        Pat::Struct { path, .. } => {
            let expr = Expr::Path(path.clone());
            Some(body.alloc_expr(expr, span))
        }
        Pat::Tuple(_)
        | Pat::Slice(_)
        | Pat::Or(_)
        | Pat::Literal(_)
        | Pat::Range { .. }
        | Pat::Err => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_field_or_method_with_receiver(
    node: &SyntaxNode,
    receiver_id: ExprId,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut found_dot = false;
    let mut name = None;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(ref t) if t.kind() == SyntaxKind::Dot => {
                found_dot = true;
            }
            glyim_syntax::SyntaxElement::Token(ref t)
                if found_dot && t.kind() == SyntaxKind::Ident =>
            {
                name = Some(interner.intern(t.text()));
                break;
            }
            // Tuple field access: `.0`, `.1`, etc. An integer after a
            // dot is always a tuple index, never a method name.
            glyim_syntax::SyntaxElement::Token(ref t)
                if found_dot && t.kind() == SyntaxKind::IntLit =>
            {
                let field = interner.intern(t.text());
                let expr = Expr::Field {
                    receiver: receiver_id,
                    field,
                };
                let eid = body.alloc_expr(expr, node_span(node));
                return Some(eid);
            }
            _ => {}
        }
    }
    let name = name?;
    let is_method = node.children_with_tokens().any(
        |el| matches!(el, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::LParen),
    );
    if is_method {
        let mut arg_ids = Vec::new();
        for child in node.children() {
            if (is_expr_node(&child) || child.kind() == SyntaxKind::Block)
                && let Some(id) = lower_expr(&child, interner, body, diags, struct_field_map)
            {
                arg_ids.push(id);
            }
        }
        let expr = Expr::MethodCall {
            receiver: receiver_id,
            method: name,
            args: arg_ids,
        };
        let eid = body.alloc_expr(expr, node_span(node));
        Some(eid)
    } else {
        let expr = Expr::Field {
            receiver: receiver_id,
            field: name,
        };
        let eid = body.alloc_expr(expr, node_span(node));
        Some(eid)
    }
}

pub(crate) fn lower_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    match node.kind() {
        SyntaxKind::Block => Some(lower_block_to_expr(
            node,
            interner,
            body,
            diags,
            struct_field_map,
        )),
        SyntaxKind::BinaryExpr => lower_binary_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::IfExpr => lower_if_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::PathExpr => lower_path_expr(node, interner, body),
        SyntaxKind::LitExpr => lower_lit_expr(node, interner, body),
        SyntaxKind::CallExpr => lower_call_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::MethodCallExpr => {
            lower_method_call_expr(node, interner, body, diags, struct_field_map)
        }
        SyntaxKind::UnaryExpr => lower_unary_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::RefExpr => lower_ref_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::MatchExpr => lower_match_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::WhileExpr => lower_while_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::LoopExpr => lower_loop_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::ForExpr => lower_for_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::AssignExpr => lower_assign_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::BreakExpr => lower_break_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::ContinueExpr => {
            let expr = Expr::Continue;
            let eid = body.alloc_expr(expr, node_span(node));
            Some(eid)
        }
        SyntaxKind::CastExpr => lower_cast_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::FieldExpr => lower_field_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::IndexExpr => lower_index_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::ArrayExpr => lower_array_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::TupleExpr => lower_tuple_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::RangeExpr => lower_range_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::ReturnExpr => lower_return_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::ClosureExpr => {
            lower_closure_expr(node, interner, body, diags, struct_field_map)
        }
        SyntaxKind::StructExpr => lower_struct_expr(node, interner, body, diags, struct_field_map),
        SyntaxKind::AwaitExpr => {
            // `.await`: lower the single operand child into `Expr::Await`.
            // The async desugaring pass (`lower_async`) later rewrites the
            // surrounding `async fn` body (and these await sites) into a poll
            // loop over the `Future` trait.
            let operand = node
                .children()
                .find(is_expr_node)
                .and_then(|c| lower_expr(&c, interner, body, diags, struct_field_map));
            let operand = match operand {
                Some(e) => e,
                None => {
                    
                    body.alloc_expr(Expr::Missing, node_span(node))
                }
            };
            let expr = Expr::Await { expr: operand };
            let eid = body.alloc_expr(expr, node_span(node));
            Some(eid)
        },
        // A bare `Path` node (e.g. an identifier inside a macro token tree that
        // the parser didn't wrap in `PathExpr`) is a variable/name reference.
        // Token-tree contents may be `UsePath` nodes (the inner node of a path),
        // which `lower_path_expr` also handles.
        SyntaxKind::UsePath => lower_path_expr(node, interner, body),
        // Phase 8.2 (unstub-5): a macro call (`println!(...)`, `vec![...)`, ...)
        // lowered so its *argument expressions* become real HIR nodes. This makes
        // variables used only inside a macro argument visible to consumers that
        // walk the HIR — most importantly the LSP reference graph (rename/find-
        // references), which previously lost them because the `_ =>` arm dropped
        // the whole `MacroCall`. The macro name is carried as the call's `func`
        // path so the call site is represented; typeck/codegen for builtin macros
        // is handled by the macro-expansion pass that runs before lowering in the
        // full pipeline. See docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md §8.2.
        SyntaxKind::MacroCall => lower_macro_call_expr(node, interner, body, diags, struct_field_map),
        _ => {
            diags.push(GlyimDiagnostic::internal_error(format!(
                "Unhandled expression kind: {:?}",
                node.kind()
            )));
            None
        }
    }
}

fn lower_closure_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut params = Vec::new();
    let mut body_expr = None;
    let mut is_move = false;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::KwMove => {
                is_move = true;
            }
            SyntaxKind::ParamList => {
                for param_node in child.children().filter(|c| c.kind() == SyntaxKind::Param) {
                    let (_, pat_id) = lower_param(&param_node, interner, &mut body.pats);
                    params.push(pat_id);
                }
            }
            _ if (is_expr_node(&child) || child.kind() == SyntaxKind::Block)
                && body_expr.is_none() =>
            {
                body_expr = lower_expr(&child, interner, body, diags, struct_field_map);
            }
            _ => {}
        }
    }
    let body_id = body_expr.unwrap_or_else(|| body.alloc_missing(node_span(node)));
    let expr = Expr::Closure {
        params,
        body: body_id,
        is_move,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn path_as_name(node: &SyntaxNode, interner: &mut Interner) -> Option<Name> {
    let mut segments = Vec::new();
    for el in node.children_with_tokens() {
        if let glyim_syntax::SyntaxElement::Token(t) = el {
            if t.kind() == SyntaxKind::Ident {
                segments.push(interner.intern(t.text()));
            }
        } else if let glyim_syntax::SyntaxElement::Node(n) = el
            && n.kind() == SyntaxKind::UsePath
        {
            for t in n.children_with_tokens() {
                if let glyim_syntax::SyntaxElement::Token(tt) = t
                    && tt.kind() == SyntaxKind::Ident
                {
                    segments.push(interner.intern(tt.text()));
                }
            }
        }
    }
    if segments.len() == 1 {
        Some(segments[0])
    } else {
        None
    }
}
fn lower_struct_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut path = None;
    let mut fields = Vec::new();
    let mut spread = None;

    // First, find the path (struct name). Build the `HirPath` directly rather
    // than allocating a stray `Expr::Path`, so the struct name is not
    // independently type-checked as a value path (it is a *type*, resolved via
    // `resolve_path_type` inside the struct literal). Mirrors the segment
    // extraction in `lower_path_expr` but does not allocate an orphan expr.
    for child in node.children() {
        if (child.kind() == SyntaxKind::PathExpr || child.kind() == SyntaxKind::UsePath)
            && path.is_none()
        {
            let mut segments = Vec::new();
            for el in child.children_with_tokens() {
                if let glyim_syntax::SyntaxElement::Token(t) = el {
                    if t.kind() == SyntaxKind::Ident {
                        segments.push(PathSegment {
                            name: interner.intern(t.text()),
                            generic_args: None,
                        });
                    }
                } else if let glyim_syntax::SyntaxElement::Node(n) = el
                    && n.kind() == SyntaxKind::UsePath
                {
                    for t in n.children_with_tokens() {
                        if let glyim_syntax::SyntaxElement::Token(tt) = t
                            && tt.kind() == SyntaxKind::Ident
                        {
                            segments.push(PathSegment {
                                name: interner.intern(tt.text()),
                                generic_args: None,
                            });
                        }
                    }
                }
            }
            if !segments.is_empty() {
                path = Some(HirPath {
                    segments,
                    kind: PathKind::Plain,
                });
            }
        }
    }

    // Helper to collect fields from a list of sibling elements (nodes and
    // tokens). It must scan tokens too, because the `..base` spread syntax
    // produces a `DotDot` *token* directly under the StructExpr, not a node.
    fn collect_from_siblings(
        siblings: &[SyntaxElement],
        interner: &mut Interner,
        body: &mut Body,
        diags: &mut Vec<GlyimDiagnostic>,
        struct_field_map: &HashMap<Name, Vec<Name>>,
        fields: &mut Vec<(Name, ExprId)>,
        spread: &mut Option<ExprId>,
        skip_name: Option<Name>,
    ) {
        let mut i = 0;
        while i < siblings.len() {
            let element = &siblings[i];
            match element {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::DotDot {
                        // `..base`: the following element (a PathExpr node) is the
                        // spread expression.
                        if i + 1 < siblings.len()
                            && let SyntaxElement::Node(next) = &siblings[i + 1]
                                && let Some(expr_id) =
                                    lower_expr(next, interner, body, diags, struct_field_map)
                                {
                                    *spread = Some(expr_id);
                                    i += 2;
                                    continue;
                                }
                    }
                    i += 1;
                }
                SyntaxElement::Node(node) => {
                    match node.kind() {
                        SyntaxKind::StructField => {
                            // Extract field name
                            let field_name = first_ident_text(node).unwrap_or_default();
                            let name = interner.intern(&field_name);
                            // Check if the field has an expression inside it
                            let expr_inside = node.children().find(is_expr_node);
                            if let Some(expr_id) = expr_inside
                                .as_ref()
                                .and_then(|n| lower_expr(n, interner, body, diags, struct_field_map))
                            {
                                fields.push((name, expr_id));
                                i += 1;
                                continue;
                            }
                            // Otherwise, assume the next sibling is the expression
                            if i + 1 < siblings.len()
                                && let SyntaxElement::Node(next) = &siblings[i + 1]
                                    && let Some(expr_id) = lower_expr(
                                        next,
                                        interner,
                                        body,
                                        diags,
                                        struct_field_map,
                                    ) {
                                        fields.push((name, expr_id));
                                        i += 2;
                                        continue;
                                    }
                            i += 1;
                        }
                        SyntaxKind::PathExpr => {
                            // Shorthand field: single identifier. Skip the
                            // struct-name itself (it is a *type*, not a field,
                            // and must not be lowered as a value path).
                            if let Some(name) = path_as_name(node, interner) {
                                if Some(name) != skip_name {
                                    if let Some(expr_id) =
                                        lower_expr(node, interner, body, diags, struct_field_map)
                                    {
                                        fields.push((name, expr_id));
                                    }
                                }
                            }
                            i += 1;
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
            }
        }
    }

    // Recursively collect all field nodes, but also gather sibling lists for fallback
    fn collect_fields(
        n: &SyntaxNode,
        interner: &mut Interner,
        body: &mut Body,
        diags: &mut Vec<GlyimDiagnostic>,
        struct_field_map: &HashMap<Name, Vec<Name>>,
        fields: &mut Vec<(Name, ExprId)>,
        spread: &mut Option<ExprId>,
        skip_name: Option<Name>,
    ) {
        match n.kind() {
            SyntaxKind::StructExpr => {
                // For the top-level StructExpr, use sibling-based collection on its
                // children *and* tokens (the `..base` spread is a token, not a node).
                let elements: Vec<SyntaxElement> = n.children_with_tokens().collect();
                collect_from_siblings(
                    &elements,
                    interner,
                    body,
                    diags,
                    struct_field_map,
                    fields,
                    spread,
                    skip_name,
                );
                // Also recurse into children for safety (but sibling collection should cover it)
                for child in n.children() {
                    collect_fields(
                        &child,
                        interner,
                        body,
                        diags,
                        struct_field_map,
                        fields,
                        spread,
                        skip_name,
                    );
                }
            }
            _ => {
                // For other nodes, just recurse
                for child in n.children() {
                    collect_fields(
                        &child,
                        interner,
                        body,
                        diags,
                        struct_field_map,
                        fields,
                        spread,
                        skip_name,
                    );
                }
            }
        }
    }

    collect_fields(
        node,
        interner,
        body,
        diags,
        struct_field_map,
        &mut fields,
        &mut spread,
        path.as_ref().and_then(|p| p.as_name()),
    );

    // Remove any field that matches the struct name (shorthand for the struct itself)
    let path_struct = path.unwrap_or_else(|| HirPath {
        segments: vec![],
        kind: PathKind::Plain,
    });
    let struct_name = path_struct.as_name();
    if let Some(struct_name_val) = struct_name {
        fields.retain(|(name, _)| *name != struct_name_val);
    }

    let ordered_fields = if let Some(name) = struct_name {
        if let Some(def_order) = struct_field_map.get(&name) {
            let mut ordered = Vec::new();
            for field_name in def_order {
                if let Some(pos) = fields.iter().position(|(f, _)| f == field_name) {
                    ordered.push(fields[pos]);
                }
            }
            for field in &fields {
                if !def_order.contains(&field.0) {
                    ordered.push(*field);
                }
            }
            ordered
        } else {
            fields
        }
    } else {
        fields
    };

    // §3.4: a struct literal that omits a field *and* has no `..base` spread is
    // a hard error (it would otherwise generate reads of uninitialized memory).
    // List *every* missing field at once rather than just the first.
    if spread.is_none()
        && let Some(name) = struct_name
            && let Some(def_order) = struct_field_map.get(&name) {
                let provided: std::collections::HashSet<Name> =
                    ordered_fields.iter().map(|(f, _)| *f).collect();
                let missing: Vec<&Name> = def_order
                    .iter()
                    .filter(|declared| !provided.contains(declared))
                    .collect();
                if !missing.is_empty() {
                    let missing_names: Vec<String> = missing
                        .iter()
                        .map(|n| interner.resolve(**n).to_string())
                        .collect();
                    let span = node_span(node);
                    diags.push(GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "missing field(s) in struct literal: {}",
                            missing_names.join(", ")
                        ),
                    ));
                }
            }

    let expr = Expr::Struct {
        path: path_struct,
        fields: ordered_fields,
        spread,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_binary_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let op_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            !t.kind().is_trivia()
                && is_bin_op_kind(t.kind())
        });
    if let Some(op_token) = op_token {
        let lhs_node = node
            .children_with_tokens()
            .take_while(|el| match el {
                glyim_syntax::SyntaxElement::Token(t) => t != &op_token,
                _ => true,
            })
            .filter_map(|el| el.as_node().cloned())
            .last()
            .filter(|n| is_expr_node(n) || n.kind() == SyntaxKind::Block);
        let rhs_node = node
            .children_with_tokens()
            .skip_while(|el| match el {
                glyim_syntax::SyntaxElement::Token(t) => t != &op_token,
                _ => true,
            })
            .skip(1)
            .find_map(|el| el.as_node().cloned())
            .filter(|n| is_expr_node(n) || n.kind() == SyntaxKind::Block);
        if let (Some(lhs), Some(rhs)) = (lhs_node, rhs_node) {
            let lhs_id = lower_expr(&lhs, interner, body, diags, struct_field_map)?;
            let rhs_id = lower_expr(&rhs, interner, body, diags, struct_field_map)?;
            let op = lower_bin_op_token(&op_token);
            let expr = Expr::Binary {
                op,
                lhs: lhs_id,
                rhs: rhs_id,
            };
            let eid = body.alloc_expr(expr, node_span(node));
            return Some(eid);
        }
    }
    let expr_children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .collect();
    if expr_children.len() < 2 {
        return None;
    }
    let lhs_id = lower_expr(&expr_children[0], interner, body, diags, struct_field_map)?;
    let rhs_id = lower_expr(&expr_children[1], interner, body, diags, struct_field_map)?;
    diags.push(GlyimDiagnostic::internal_error(
        "Unrecognized binary operator token",
    ));
    let expr = Expr::Binary {
        op: BinOp::Add,
        lhs: lhs_id,
        rhs: rhs_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

/// Whether a syntax token kind is a binary-operator token. Restricting the
/// operator search to these kinds (rather than "first non-Ident token") is
/// essential: operand paths like `self.current` begin with `KwSelf`, which is
/// NOT `Ident`, so the naive filter would mis-detect `self` as the operator
/// and drop the whole expression.
fn is_bin_op_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Plus
            | SyntaxKind::Minus
            | SyntaxKind::Star
            | SyntaxKind::Slash
            | SyntaxKind::Percent
            | SyntaxKind::Eq
            | SyntaxKind::EqEq
            | SyntaxKind::BangEq
            | SyntaxKind::Lt
            | SyntaxKind::Gt
            | SyntaxKind::LtEq
            | SyntaxKind::GtEq
            | SyntaxKind::And
            | SyntaxKind::Or
            | SyntaxKind::AndAnd
            | SyntaxKind::OrOr
            | SyntaxKind::Caret
            | SyntaxKind::Shl
            | SyntaxKind::Shr
    )
}

fn lower_bin_op_token(token: &SyntaxToken) -> BinOp {
    match token.text() {
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "/" => BinOp::Div,
        "%" => BinOp::Rem,
        "==" => BinOp::Eq,
        "!=" => BinOp::Ne,
        "<" => BinOp::Lt,
        ">" => BinOp::Gt,
        "<=" => BinOp::LtEq,
        ">=" => BinOp::GtEq,
        "&&" => BinOp::And,
        "||" => BinOp::Or,

        "&" => BinOp::BitAnd,
        "|" => BinOp::BitOr,
        "^" => BinOp::BitXor,
        "<<" => BinOp::Shl,
        ">>" => BinOp::Shr,
        other => unreachable!(
            "parser produced a binary-expr node with unrecognized operator \
             token {:?} -- parser and HIR lowering are out of sync, this is \
             a compiler bug, not a user error",
            other
        ),
    }
}

fn lower_if_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .collect();
    if children.len() < 2 {
        return None;
    }
    let cond = children.remove(0);
    let then_branch = children.remove(0);
    let else_branch = children.pop();
    let cond_id = lower_expr(&cond, interner, body, diags, struct_field_map)?;
    let then_id = lower_expr(&then_branch, interner, body, diags, struct_field_map)?;
    let else_id = else_branch.and_then(|e| lower_expr(&e, interner, body, diags, struct_field_map));
    let expr = Expr::If {
        cond: cond_id,
        then_branch: then_id,
        else_branch: else_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_path_expr(node: &SyntaxNode, interner: &mut Interner, body: &mut Body) -> Option<ExprId> {
    // NOTE: generic args always belong to the *preceding* path segment in this
    // grammar (no per-segment turbofish like `Foo::<T>::Bar::<U>`). The type
    // nodes emitted by `parse_type_arg_list` are siblings of the `Ident`
    // tokens, so when we encounter a type node we attach the collected args to
    // the segment pushed just before it (this is what makes `Vec::<i32>::new`
    // put `<i32>` on `Vec`, not on `new`).
    let mut segments = Vec::new();
    let mut pending_args: Vec<TypeRef> = Vec::new();

    let flush_pending = |segments: &mut Vec<PathSegment>, pending_args: &mut Vec<TypeRef>| {
        if !pending_args.is_empty() {
            if let Some(last) = segments.last_mut() {
                last.generic_args = Some(std::mem::take(pending_args));
            }
        }
    };

    for el in node.children_with_tokens() {
        match el {
        glyim_syntax::SyntaxElement::Token(t)
            if t.kind() == SyntaxKind::Ident
                || t.kind() == SyntaxKind::KwSelf
                || t.kind() == SyntaxKind::KwSuper
                || t.kind() == SyntaxKind::KwCrate =>
        {
            flush_pending(&mut segments, &mut pending_args);
            segments.push(PathSegment {
                name: interner.intern(t.text()),
                generic_args: None,
            });
        }
            glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::UsePath => {
                for t in n.children_with_tokens() {
                    if let glyim_syntax::SyntaxElement::Token(tt) = t
                        && (tt.kind() == SyntaxKind::Ident
                            || tt.kind() == SyntaxKind::KwSelf
                            || tt.kind() == SyntaxKind::KwSuper
                            || tt.kind() == SyntaxKind::KwCrate)
                    {
                        flush_pending(&mut segments, &mut pending_args);
                        segments.push(PathSegment {
                            name: interner.intern(tt.text()),
                            generic_args: None,
                        });
                    }
                }
            }
            // Turbofish type args are lowered with the same lower_type_ref used
            // by ordinary type positions, so `Vec::<i32>` and `Vec<i32>`
            // produce identical TypeRef::Path shapes.
            glyim_syntax::SyntaxElement::Node(n) if super::is_type_node(&n) => {
                if let Some(ty) = super::lower_type::lower_type_ref(&n, interner) {
                    pending_args.push(ty);
                }
            }
            _ => {}
        }
    }
    // Flush any trailing args (e.g. `Vec<i32>` with no further segment).
    flush_pending(&mut segments, &mut pending_args);
    if segments.is_empty() {
        return None;
    }
    let path = HirPath {
        segments,
        kind: PathKind::Plain,
    };
    let expr = Expr::Path(path);
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_lit_expr(node: &SyntaxNode, interner: &mut Interner, body: &mut Body) -> Option<ExprId> {
    let lit_token = node
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| {
            t.kind().is_literal()
                || t.kind() == SyntaxKind::KwTrue
                || t.kind() == SyntaxKind::KwFalse
        })?;
    let lit = lower_literal(&lit_token, interner);
    let expr = Expr::Literal(lit);
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

pub(crate) fn lower_literal(token: &SyntaxToken, interner: &mut Interner) -> Literal {
    let text = token.text().to_string();
    match token.kind() {
        SyntaxKind::IntLit => {
            let (num_str, suffix) = split_int_literal(&text);
            let (value, is_unsigned) = parse_int_with_prefix(&num_str);
            if let Some(suffix) = suffix {
                match suffix.as_str() {
                    "i8" => return Literal::Int(value, Some(IntTy::I8)),
                    "i16" => return Literal::Int(value, Some(IntTy::I16)),
                    "i32" => return Literal::Int(value, Some(IntTy::I32)),
                    "i64" => return Literal::Int(value, Some(IntTy::I64)),
                    "isize" => return Literal::Int(value, Some(IntTy::Isize)),
                    "u8" => return Literal::Uint(value as u128, Some(UintTy::U8)),
                    "u16" => return Literal::Uint(value as u128, Some(UintTy::U16)),
                    "u32" => return Literal::Uint(value as u128, Some(UintTy::U32)),
                    "u64" => return Literal::Uint(value as u128, Some(UintTy::U64)),
                    "usize" => return Literal::Uint(value as u128, Some(UintTy::Usize)),
                    _ => {
                        tracing::warn!("Unknown integer suffix: {}", suffix);
                        return Literal::Int(value, None);
                    }
                }
            }
            if is_unsigned {
                Literal::Uint(value as u128, None)
            } else {
                Literal::Int(value, None)
            }
        }
        SyntaxKind::FloatLit => {
            let (num_str, _suffix) = split_float_literal(&text);
            if let Ok(f) = num_str.parse::<f64>() {
                return Literal::Float(f.to_bits(), FloatTy::F64);
            }
            tracing::warn!("Failed to parse float literal: {}", text);
            Literal::Unit
        }
        SyntaxKind::KwTrue | SyntaxKind::BoolLit if text == "true" => Literal::Bool(true),
        SyntaxKind::KwFalse | SyntaxKind::BoolLit if text == "false" => Literal::Bool(false),
        SyntaxKind::CharLit => {
            let inner = &text[1..text.len() - 1];
            if let Some(c) = parse_char_literal(inner) {
                Literal::Char(c)
            } else {
                Literal::Unit
            }
        }
        SyntaxKind::StringLit => {
            let raw = token.text().trim_start_matches('"').trim_end_matches('"');
            let processed = raw
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\")
                .replace("\\'", "'")
                .replace("\\\"", "\"");
            Literal::String(interner.intern(&processed))
        }
        _ => Literal::Unit,
    }
}

fn split_int_literal(s: &str) -> (String, Option<String>) {
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    if i + 1 < chars.len() && chars[i] == '0' {
        let prefix = chars[i + 1];
        if prefix == 'x' || prefix == 'X' {
            i += 2;
            while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                i += 1;
            }
        } else if prefix == 'o' || prefix == 'O' {
            i += 2;
            while i < chars.len() && (('0' <= chars[i] && chars[i] <= '7') || chars[i] == '_') {
                i += 1;
            }
        } else if prefix == 'b' || prefix == 'B' {
            i += 2;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1' || chars[i] == '_') {
                i += 1;
            }
        } else {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                i += 1;
            }
        }
    } else {
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
            i += 1;
        }
    }
    let num_part = &s[..i];
    let suffix = if i < s.len() { Some(&s[i..]) } else { None };
    (num_part.replace('_', ""), suffix.map(|s| s.to_string()))
}

fn parse_int_with_prefix(s: &str) -> (i128, bool) {
    let s = s.trim_start_matches('+');
    if s.starts_with("0x") || s.starts_with("0X") {
        (i128::from_str_radix(&s[2..], 16).unwrap_or(0), false)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        (i128::from_str_radix(&s[2..], 8).unwrap_or(0), false)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        (i128::from_str_radix(&s[2..], 2).unwrap_or(0), false)
    } else {
        (s.parse::<i128>().unwrap_or(0), s.starts_with('-'))
    }
}

fn split_float_literal(s: &str) -> (String, Option<String>) {
    let mut digits_end = s.len();
    for (i, ch) in s.char_indices() {
        if !(ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '+' || ch == '-') {
            digits_end = i;
            break;
        }
    }
    let num_part = &s[..digits_end];
    let suffix = if digits_end < s.len() {
        Some(&s[digits_end..])
    } else {
        None
    };
    (num_part.to_string(), suffix.map(|s| s.to_string()))
}

fn parse_char_literal(s: &str) -> Option<char> {
    if s.len() == 1 {
        return s.chars().next();
    }
    if let Some(stripped) = s.strip_prefix('\\') {
        match stripped {
            "n" => Some('\n'),
            "r" => Some('\r'),
            "t" => Some('\t'),
            "\\" => Some('\\'),
            "'" => Some('\''),
            "\"" => Some('\"'),
            _ => None,
        }
    } else {
        None
    }
}

fn lower_call_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::PathExpr)
        .collect();
    let func = children.first()?.clone();
    let args: Vec<SyntaxNode> = children.into_iter().skip(1).collect();
    let func_id = lower_expr(&func, interner, body, diags, struct_field_map)?;
    let mut arg_ids = Vec::new();
    for arg in args {
        if let Some(id) = lower_expr(&arg, interner, body, diags, struct_field_map) {
            arg_ids.push(id);
        }
    }
    let expr = Expr::Call {
        func: func_id,
        args: arg_ids,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

/// Lower a `MacroCall` (`println!(..)`, `vec![..]`, user `macro_rules!`) so its
/// *argument expressions* become real HIR nodes. Phase 8.2 (unstub-5): variables
/// used only inside a macro argument were previously invisible to HIR consumers
/// (the `_ =>` arm in `lower_expr` dropped the whole node). We now emit an
/// `Expr::Call` whose `func` is the macro's name path and whose `args` are the
/// lowered argument expressions, so the reference graph (rename / find-references)
/// can see them.
///
/// Note: this is the *pre-expansion* representation. In the full codegen pipeline
/// the macro-expansion pass (`glyim_meta::expand_crate`) runs before HIR lowering,
/// so builtin/declarative macro calls are rewritten into ordinary code and never
/// reach this path. This handler exists for the LSP analysis path (which walks the
/// unexpanded HIR) and as a safe fallback that preserves macro-call arguments
/// instead of discarding them.
fn lower_macro_call_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    // The macro name is the first child (a `PathExpr` holding the macro
    // identifier, e.g. `println` in `println!(..)`). The identifier is nested
    // inside `UsePath`, so search descendants for the first `Ident` token.
    fn first_ident_deep(node: &SyntaxNode) -> Option<String> {
        for el in node.children_with_tokens() {
            match el {
                glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Ident => {
                    return Some(t.text().to_string());
                }
                glyim_syntax::SyntaxElement::Node(n) => {
                    if let Some(s) = first_ident_deep(&n) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let macro_name = node
        .children()
        .next()
        .and_then(|c| first_ident_deep(&c))
        .map(|s| interner.intern(&s));
    let macro_name = macro_name?;

    // The arguments live in the *second* `TokenTree` child (the first holds the
    // macro path). Token trees nest arbitrarily and their leaves are raw tokens
    // (the parser does not build `PathExpr`/`UsePath` nodes inside them), so we
    // collect every `Ident` token *and* every expression node that appears
    // anywhere beneath the args token tree. Variable idents become `Expr::Path`
    // nodes; nested expressions (e.g. another macro call) are lowered normally.
    // This is what makes variables used only inside macro arguments visible to
    // HIR consumers such as the reference graph (rename / find-references).
    fn collect_arg_elements(node: &SyntaxNode, out: &mut Vec<glyim_syntax::SyntaxElement>) {
        for el in node.children_with_tokens() {
            match &el {
                glyim_syntax::SyntaxElement::Node(n) => {
                    if is_expr_node(n) || n.kind() == SyntaxKind::UsePath {
                        // Owned expression node: lower it as a whole (its own
                        // idents are handled inside `lower_expr`), so do not
                        // descend into it.
                        out.push(el.clone());
                    } else {
                        collect_arg_elements(n, out);
                    }
                }
                glyim_syntax::SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::Ident {
                        out.push(el.clone());
                    }
                }
            }
        }
    }
    let mut arg_elements: Vec<glyim_syntax::SyntaxElement> = Vec::new();
    if let Some(args_tree) = node.children().nth(1) {
        collect_arg_elements(&args_tree, &mut arg_elements);
    }
    let mut arg_ids = Vec::new();
    for el in arg_elements {
        match el {
            glyim_syntax::SyntaxElement::Node(n) => {
                if let Some(id) = lower_expr(&n, interner, body, diags, struct_field_map) {
                    arg_ids.push(id);
                }
            }
            glyim_syntax::SyntaxElement::Token(t) => {
                let name = interner.intern(t.text());
                let path = HirPath::from_single(name);
                arg_ids.push(body.alloc_expr(Expr::Path(path), node_span(node)));
            }
        }
    }

    let func_path = HirPath::from_single(macro_name);
    let func_id = body.alloc_expr(Expr::Path(func_path), node_span(node));
    let expr = Expr::Call {
        func: func_id,
        args: arg_ids,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_method_call_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    // Find the receiver: any expression node, not just PathExpr.
    // This supports tuple field access like `(1, 2).0` where the
    // receiver is a TupleExpr, as well as chained field access like
    // `a.b.c` where the inner receiver is another FieldExpr.
    let receiver = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)?;
    let receiver_id = lower_expr(&receiver, interner, body, diags, struct_field_map)?;
    let mut found_dot = false;
    let mut method_name = None;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(ref t) if t.kind() == SyntaxKind::Dot => {
                found_dot = true
            }
            glyim_syntax::SyntaxElement::Token(ref t)
                if found_dot && t.kind() == SyntaxKind::Ident =>
            {
                method_name = Some(interner.intern(t.text()));
                break;
            }
            _ => {}
        }
    }
    let method = method_name?;
    // Collect explicit arguments only. The receiver is lowered separately into
    // `receiver_id` and must NOT be duplicated into `args` — downstream
    // (glyim-typeck) treats `receiver` and `args` as disjoint (it builds
    // `thir::Call { func: recv_expr, args }`), so including the receiver here
    // would feed it as a leading explicit argument and corrupt the call's
    // argument list. Skip the receiver node itself.
    let mut arg_ids = Vec::new();
    for child in node.children() {
        if child == receiver {
            continue;
        }
        if child.kind() != SyntaxKind::PathExpr
            && (is_expr_node(&child) || child.kind() == SyntaxKind::Block)
            && let Some(id) = lower_expr(&child, interner, body, diags, struct_field_map)
        {
            arg_ids.push(id);
        }
    }
    let expr = Expr::MethodCall {
        receiver: receiver_id,
        method,
        args: arg_ids,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_unary_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let op_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::Bang | SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::And
            )
        })?;
    let inner = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)?;
    let expr_id = lower_expr(&inner, interner, body, diags, struct_field_map)?;
    if op_token.kind() == SyntaxKind::And {
        // `&x` (immutable borrow) vs `&mut x` (mutable borrow). Detect the
        // `mut` keyword so `&mut x` lowers to a `Mut` reference; otherwise the
        // `&mut` case would always be recorded as immutable (and the dedicated
        // `lower_ref_expr` mutability detection would be bypassed).
        let is_mut = node
            .children_with_tokens()
            .any(|c| matches!(&c, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwMut));
        let expr = Expr::Ref {
            expr: expr_id,
            mutability: if is_mut {
                Mutability::Mut
            } else {
                Mutability::Not
            },
        };
        let eid = body.alloc_expr(expr, node_span(node));
        return Some(eid);
    }
    let op = match op_token.kind() {
        SyntaxKind::Bang => UnOp::Not,
        SyntaxKind::Minus => UnOp::Neg,
        SyntaxKind::Star => UnOp::Deref,
        _ => return None,
    };
    let expr = Expr::Unary { op, expr: expr_id };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_ref_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let inner = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)?;
    let expr_id = lower_expr(&inner, interner, body, diags, struct_field_map)?;
    let mutability = if node.children_with_tokens().any(
        |c| matches!(&c, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwMut),
    ) {
        Mutability::Mut
    } else {
        Mutability::Not
    };
    let expr = Expr::Ref {
        expr: expr_id,
        mutability,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}
fn lower_match_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let scrutinee = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)?;
    let scrutinee_id = lower_expr(&scrutinee, interner, body, diags, struct_field_map)?;
    let mut arms = Vec::new();
    if let Some(arm_list) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::MatchArmList)
    {
        for arm_node in arm_list
            .children()
            .filter(|c| c.kind() == SyntaxKind::MatchArm)
        {
            let mut pat_id = None;
            let mut guard = None;
            let mut body_id = None;
            let mut prev_expr: Option<ExprId> = None;
            for part in arm_node.children() {
                match part.kind() {
                    SyntaxKind::PatIdent
                    | SyntaxKind::PatWild
                    | SyntaxKind::PatLit
                    | SyntaxKind::PatRange  // <--- ADDED
                    | SyntaxKind::PatTuple
                    | SyntaxKind::PatStruct
                    | SyntaxKind::PatOr
                    | SyntaxKind::PatSlice
                    | SyntaxKind::UsePath => {
                        pat_id = lower_pat(&part, interner, &mut body.pats, diags)
                    }
                    _ if is_expr_node(&part) => {
                        // In a match arm, the structure is:
                        //   <pat> [if <guard-expr>] => <body-expr>
                        // The first expression node after the pattern is the
                        // guard (when `if` is present); everything after the
                        // `=>` is the body. Since the parser represents the arm
                        // as pattern + expr-node(s), the LAST expr node is the
                        // body and (if there are two) the first is the guard.
                        let eid = lower_expr(&part, interner, body, diags, struct_field_map);
                        if let Some(eid) = eid {
                            if body_id.is_some() {
                                // Already have a body; this can't be a guard.
                                // Defensive: keep the last as body.
                                body_id = Some(eid);
                            } else if let Some(prev) = prev_expr {
                                // Second expr node -> body; first was the guard.
                                guard = Some(prev);
                                body_id = Some(eid);
                            } else {
                                // First expr node: tentatively a guard, but may
                                // turn out to be the body if no second appears.
                                prev_expr = Some(eid);
                            }
                        }
                    }
                    _ => {}
                }
            }
            // If only one expr node was seen, it is the body (no guard).
            if body_id.is_none() {
                body_id = prev_expr.take();
                guard = None;
            }
            if let (Some(pat), Some(body_id_val)) = (pat_id, body_id) {
                arms.push(MatchArm {
                    pat,
                    guard,
                    body: body_id_val,
                });
            } else {
                // Arm had neither a pattern nor a body; skip it.
            }
        }
    }
    let expr = Expr::Match {
        scrutinee: scrutinee_id,
        arms,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_while_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .collect();
    if children.len() < 2 {
        return None;
    }
    let cond = children.remove(0);
    let body_expr = children.remove(0);
    let cond_id = lower_expr(&cond, interner, body, diags, struct_field_map)?;
    let body_id = lower_expr(&body_expr, interner, body, diags, struct_field_map)?;
    let expr = Expr::While {
        cond: cond_id,
        body: body_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_loop_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let body_node = node.children().find(|c| c.kind() == SyntaxKind::Block)?;
    let body_id = lower_expr(&body_node, interner, body, diags, struct_field_map)?;
    let expr = Expr::Loop { body: body_id };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_for_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut children = node.children();
    let pat_node = children.find(|c| {
        matches!(
            c.kind(),
            SyntaxKind::PatIdent
                | SyntaxKind::PatWild
                | SyntaxKind::PatLit       // <--- ADDED
                | SyntaxKind::PatRange     // <--- ADDED
                | SyntaxKind::PatTuple
                | SyntaxKind::PatStruct
                | SyntaxKind::PatOr        // <--- ADDED
                | SyntaxKind::PatSlice // <--- ADDED
        )
    })?;
    let iterable_node = children.find(|c| is_expr_node(c) || c.kind() == SyntaxKind::RangeExpr)?;
    let body_node = children.find(|c| c.kind() == SyntaxKind::Block)?;
    let pat_id = lower_pat(&pat_node, interner, &mut body.pats, diags)?;
    let iterable_id = lower_expr(&iterable_node, interner, body, diags, struct_field_map)?;
    let body_id = lower_expr(&body_node, interner, body, diags, struct_field_map)?;
    let expr = Expr::For {
        pat: pat_id,
        iterable: iterable_id,
        body: body_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}
fn lower_assign_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::PathExpr)
        .collect();
    if children.len() < 2 {
        return None;
    }
    let lhs = children.remove(0);
    let rhs = children.remove(0);
    let lhs_id = lower_expr(&lhs, interner, body, diags, struct_field_map)?;
    let rhs_id = lower_expr(&rhs, interner, body, diags, struct_field_map)?;
    let expr = Expr::Assign {
        lhs: lhs_id,
        rhs: rhs_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_return_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let value = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .and_then(|n| lower_expr(&n, interner, body, diags, struct_field_map));
    let expr = Expr::Return { value };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_break_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let value = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .and_then(|n| lower_expr(&n, interner, body, diags, struct_field_map));
    let expr = Expr::Break { value };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_cast_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut expr_node = None;
    let mut type_node = None;
    for child in node.children() {
        if is_expr_node(&child) && expr_node.is_none() {
            expr_node = Some(child);
        } else if is_type_node(&child) {
            type_node = Some(child);
        }
    }
    let expr_id = lower_expr(&expr_node?, interner, body, diags, struct_field_map)?;
    let ty = lower_type_ref(&type_node?, interner)?;
    let expr = Expr::Cast { expr: expr_id, ty };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_field_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    // Find the receiver: any expression node, not just PathExpr.
    // This supports tuple field access like `(1, 2).0` where the
    // receiver is a TupleExpr, as well as chained field access like
    // `a.b.c` where the inner receiver is another FieldExpr.
    let receiver = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)?;
    let receiver_id = lower_expr(&receiver, interner, body, diags, struct_field_map)?;
    let mut found_dot = false;
    let mut field_name = None;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(ref t) if t.kind() == SyntaxKind::Dot => {
                found_dot = true
            }
            glyim_syntax::SyntaxElement::Token(ref t)
                if found_dot && t.kind() == SyntaxKind::Ident =>
            {
                field_name = Some(interner.intern(t.text()));
                break;
            }
            // Tuple field access: `.0`, `.1`, etc. The integer literal
            // is interned as a string name (e.g., "0", "1") so that
            // typeck can distinguish tuple indices from struct fields
            // based on the receiver's type.
            glyim_syntax::SyntaxElement::Token(ref t)
                if found_dot && t.kind() == SyntaxKind::IntLit =>
            {
                field_name = Some(interner.intern(t.text()));
                break;
            }
            _ => {}
        }
    }
    let field = field_name?;
    let expr = Expr::Field {
        receiver: receiver_id,
        field,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_index_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .collect();
    if children.len() < 2 {
        return None;
    }
    let base = children.remove(0);
    let index = children.remove(0);
    let base_id = lower_expr(&base, interner, body, diags, struct_field_map)?;
    let index_id = lower_expr(&index, interner, body, diags, struct_field_map)?;
    let expr = Expr::Index {
        base: base_id,
        index: index_id,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_array_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut elems = Vec::new();
    for child in node.children().filter(is_expr_node) {
        if let Some(id) = lower_expr(&child, interner, body, diags, struct_field_map) {
            elems.push(id);
        }
    }
    let expr = Expr::Array(elems);
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_tuple_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let mut elems = Vec::new();
    for child in node.children().filter(is_expr_node) {
        if let Some(id) = lower_expr(&child, interner, body, diags, struct_field_map) {
            elems.push(id);
        }
    }
    let expr = Expr::Tuple(elems);
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}

fn lower_range_expr(
    node: &SyntaxNode,
    interner: &mut Interner,
    body: &mut Body,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<ExprId> {
    let children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::LitExpr)
        .collect();
    let start = children
        .first()
        .and_then(|n| lower_expr(n, interner, body, diags, struct_field_map));
    let end = children
        .get(1)
        .and_then(|n| lower_expr(n, interner, body, diags, struct_field_map));
    let inclusive = node.children_with_tokens().any(
        |c| matches!(&c, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::DotDotEq),
    );
    let expr = Expr::Range {
        start,
        end,
        inclusive,
    };
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}
