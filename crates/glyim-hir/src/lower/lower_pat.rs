use glyim_core::arena::IndexVec;
use glyim_core::interner::Interner;
use glyim_core::path::PathKind;
use glyim_core::primitives::*;
use glyim_syntax::{SyntaxKind, SyntaxNode};

use crate::{Pat, PatId, Path as HirPath, PathSegment};

use super::{first_ident_text, lower_expr::lower_literal};
use glyim_diag::GlyimDiagnostic;

#[allow(unused_assignments)]
pub(crate) fn lower_pat(
    node: &SyntaxNode,
    interner: &mut Interner,
    pats: &mut IndexVec<PatId, Pat>,
    diags: &mut Vec<GlyimDiagnostic>,
) -> Option<PatId> {
    match node.kind() {
        SyntaxKind::PatIdent => {
            let name_text = first_ident_text(node).unwrap_or_else(|| "_".to_string());
            let name = interner.intern(&name_text);
            if name_text.starts_with(|c: char| c.is_uppercase()) {
                let path = HirPath {
                    segments: vec![PathSegment {
                        name,
                        generic_args: None,
                    }],
                    kind: PathKind::Plain,
                };
                Some(pats.push(Pat::Path(path)))
            } else {
                let subpat = node
                    .children()
                    .find(|c| {
                        matches!(
                            c.kind(),
                            SyntaxKind::PatIdent
                                | SyntaxKind::PatWild
                                | SyntaxKind::PatLit
                                | SyntaxKind::PatTuple
                                | SyntaxKind::PatStruct
                                | SyntaxKind::PatOr
                                | SyntaxKind::PatRange
                                | SyntaxKind::PatSlice
                        )
                    })
                    .and_then(|n| lower_pat(&n, interner, pats, diags));
                Some(pats.push(Pat::Binding {
                    name,
                    mutability: Mutability::Not,
                    subpattern: subpat,
                }))
            }
        }
        SyntaxKind::PatWild => Some(pats.push(Pat::Wild)),
        SyntaxKind::PatLit => {
            // PatLit wraps a single literal token (simple literal pattern)
            let lit_token = node
                .children_with_tokens()
                .filter_map(|c| c.into_token())
                .find(|t| {
                    t.kind().is_literal()
                        || t.kind() == SyntaxKind::KwTrue
                        || t.kind() == SyntaxKind::KwFalse
                })?;
            let lit = lower_literal(&lit_token, interner);
            Some(pats.push(Pat::Literal(lit)))
        }
        SyntaxKind::PatRange => {
            // PatRange contains: start_literal_token, DotDot/DotDotEq, end PatLit node
            let mut start = None;
            let mut end = None;
            let mut inclusive = false;
            let mut after_dot = false;

            for child in node.children_with_tokens() {
                match child {
                    glyim_syntax::SyntaxElement::Token(t) if t.kind().is_literal() => {
                        let lit = lower_literal(&t, interner);
                        if !after_dot {
                            start = Some(lit);
                        } else {
                            end = Some(lit);
                        }
                    }
                    glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::DotDotEq => {
                        inclusive = true;
                        after_dot = true;
                    }
                    glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::DotDot => {
                        inclusive = false;
                        after_dot = true;
                    }
                    glyim_syntax::SyntaxElement::Node(n) => {
                        if let Some(pat_id) = lower_pat(&n, interner, pats, diags)
                            && let Pat::Literal(lit) = &pats[pat_id]
                        {
                            if !after_dot {
                                start = Some(lit.clone());
                            } else {
                                end = Some(lit.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some(pats.push(Pat::Range {
                start,
                end,
                inclusive,
            }))
        }
        SyntaxKind::PatOr => {
            let mut flat = Vec::new();
            for child in node.children() {
                if let Some(pat_id) = lower_pat(&child, interner, pats, diags) {
                    if let Pat::Or(inner) = &pats[pat_id] {
                        flat.extend(inner.iter().copied());
                    } else {
                        flat.push(pat_id);
                    }
                }
            }
            Some(pats.push(Pat::Or(flat)))
        }
        SyntaxKind::PatSlice => {
            let mut elems = Vec::new();
            for child in node.children() {
                if let Some(pat_id) = lower_pat(&child, interner, pats, diags) {
                    elems.push(pat_id);
                }
            }
            Some(pats.push(Pat::Slice(elems)))
        }
        SyntaxKind::PatTuple => {
            let mut elems = Vec::new();
            for child in node.children() {
                if let Some(pat_id) = lower_pat(&child, interner, pats, diags) {
                    elems.push(pat_id);
                }
            }
            Some(pats.push(Pat::Tuple(elems)))
        }
        SyntaxKind::PatStruct => {
            let mut path = None;
            let mut fields = Vec::new();
            let mut rest = false;

            for child in node.children_with_tokens() {
                match &child {
                    glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::UsePath => {
                        let mut segments = Vec::new();
                        for el in n.children_with_tokens() {
                            if let glyim_syntax::SyntaxElement::Token(t) = el
                                && t.kind() == SyntaxKind::Ident
                            {
                                segments.push(PathSegment {
                                    name: interner.intern(t.text()),
                                    generic_args: None,
                                });
                            }
                        }
                        if !segments.is_empty() {
                            path = Some(HirPath {
                                segments,
                                kind: PathKind::Plain,
                            });
                        }
                    }
                    glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::PatTuple => {
                        for el in n.children_with_tokens() {
                            if let glyim_syntax::SyntaxElement::Node(sub_n) = el
                                && matches!(
                                    sub_n.kind(),
                                    SyntaxKind::PatIdent
                                        | SyntaxKind::PatWild
                                        | SyntaxKind::PatLit
                                        | SyntaxKind::PatTuple
                                        | SyntaxKind::PatStruct
                                        | SyntaxKind::PatOr
                                        | SyntaxKind::UsePath
                                        | SyntaxKind::PatRange
                                        | SyntaxKind::PatSlice
                                )
                                && let Some(pat_id) = lower_pat(&sub_n, interner, pats, diags)
                            {
                                let field_name = {
                                    let s = sub_n.text().to_string();
                                    interner.intern(s.trim())
                                };
                                fields.push((field_name, pat_id));
                            }
                        }
                    }
                    glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::PatIdent => {
                        let field_name_text = first_ident_text(n).unwrap_or_default();
                        let name = interner.intern(&field_name_text);
                        let mut has_colon = false;
                        let mut subpattern_node = None;
                        let siblings: Vec<glyim_syntax::SyntaxElement> =
                            node.children_with_tokens().collect();
                        for (i, el) in siblings.iter().enumerate() {
                            if let glyim_syntax::SyntaxElement::Node(sn) = el
                                && *sn == *n
                            {
                                for sibling in siblings.iter().skip(i + 1) {
                                    match sibling {
                                        glyim_syntax::SyntaxElement::Token(t)
                                            if t.kind() == SyntaxKind::Colon =>
                                        {
                                            has_colon = true;
                                        }
                                        glyim_syntax::SyntaxElement::Token(t)
                                            if t.kind().is_trivia() =>
                                        {
                                            continue;
                                        }
                                        glyim_syntax::SyntaxElement::Node(pn)
                                            if matches!(
                                                pn.kind(),
                                                SyntaxKind::PatIdent
                                                    | SyntaxKind::PatWild
                                                    | SyntaxKind::PatLit
                                                    | SyntaxKind::PatTuple
                                                    | SyntaxKind::PatStruct
                                                    | SyntaxKind::PatOr
                                                    | SyntaxKind::PatRange
                                                    | SyntaxKind::PatSlice
                                            ) =>
                                        {
                                            if has_colon {
                                                subpattern_node = Some(pn.clone());
                                            }
                                            break;
                                        }
                                        _ => break,
                                    }
                                }
                                break;
                            }
                        }
                        if has_colon {
                            if let Some(sub_n) = subpattern_node
                                && let Some(pat_id) = lower_pat(&sub_n, interner, pats, diags)
                            {
                                fields.push((name, pat_id));
                            }
                        } else {
                            let binding_id = pats.push(Pat::Binding {
                                name,
                                mutability: Mutability::Not,
                                subpattern: None,
                            });
                            fields.push((name, binding_id));
                        }
                    }
                    glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::DotDot => {
                        rest = true;
                    }
                    _ => {}
                }
            }

            let path = path?;
            Some(pats.push(Pat::Struct { path, fields, rest }))
        }
        SyntaxKind::UsePath => {
            let mut segments = Vec::new();
            for el in node.children_with_tokens() {
                if let glyim_syntax::SyntaxElement::Token(t) = el
                    && t.kind() == SyntaxKind::Ident
                {
                    segments.push(PathSegment {
                        name: interner.intern(t.text()),
                        generic_args: None,
                    });
                }
            }
            if segments.is_empty() {
                return None;
            }
            let path = HirPath {
                segments,
                kind: PathKind::Plain,
            };
            Some(pats.push(Pat::Path(path)))
        }
        SyntaxKind::PathExpr => {
            let mut segments = Vec::new();
            for el in node.children_with_tokens() {
                if let glyim_syntax::SyntaxElement::Token(t) = el
                    && t.kind() == SyntaxKind::Ident
                {
                    segments.push(PathSegment {
                        name: interner.intern(t.text()),
                        generic_args: None,
                    });
                }
            }
            if segments.is_empty() {
                return None;
            }
            let path = HirPath {
                segments,
                kind: PathKind::Plain,
            };
            Some(pats.push(Pat::Path(path)))
        }
        _ => {
            diags.push(GlyimDiagnostic::internal_error(format!(
                "unhandled pattern kind: {:?}",
                node.kind()
            )));
            None
        }
    }
}
