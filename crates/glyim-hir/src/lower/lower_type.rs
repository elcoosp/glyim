use glyim_core::interner::Interner;
use glyim_core::path::PathKind;
use glyim_core::primitives::*;
use glyim_syntax::{SyntaxKind, SyntaxNode};

use crate::{ConstRef, Literal, Path as HirPath, PathSegment, TypeRef};

use super::is_type_node;

pub(crate) fn lower_type_ref(node: &SyntaxNode, interner: &mut Interner) -> Option<TypeRef> {
    match node.kind() {
        SyntaxKind::PathType => {
            let path = lower_path_from_type(node, interner)?;
            Some(TypeRef::Path(path))
        }
        SyntaxKind::RefType => {
            let inner_node = node.children().find(is_type_node)?;
            let inner = lower_type_ref(&inner_node, interner)?;
            let mutability = if node.children_with_tokens().any(|c| {
                matches!(&c, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwMut)
            }) { Mutability::Mut } else { Mutability::Not };
            Some(TypeRef::Ref {
                inner: Box::new(inner),
                mutability,
            })
        }
        SyntaxKind::FnType => {
            let mut params = Vec::new();
            let mut ret = None;
            let mut after_arrow = false;
            for child in node.children_with_tokens() {
                if child.kind() == SyntaxKind::Arrow {
                    after_arrow = true;
                    continue;
                }
                if let Some(n) = child.as_node() {
                    if is_type_node(&n) {
                        if let Some(ty) = lower_type_ref(&n, interner) {
                            if after_arrow {
                                ret = Some(Box::new(ty));
                            } else {
                                params.push(ty);
                            }
                        }
                    }
                }
            }
            Some(TypeRef::Fn { params, ret })
        }
        SyntaxKind::SliceType => {
            let inner = node
                .children()
                .find(is_type_node)
                .and_then(|n| lower_type_ref(&n, interner))?;
            Some(TypeRef::Slice(Box::new(inner)))
        }
        SyntaxKind::ArrayType => {
            let mut inner = None;
            let mut len = None;
            for child in node.children() {
                if is_type_node(&child) {
                    inner = lower_type_ref(&child, interner);
                } else if child.kind() == SyntaxKind::LitExpr {
                    let text = child
                        .first_token()
                        .map(|t| t.text().to_string())
                        .unwrap_or_default();
                    if let Ok(n) = text.parse::<u128>() {
                        len = Some(ConstRef::Literal(Literal::Uint(n, None)));
                    } else if let Ok(n) = text.parse::<i128>() {
                        len = Some(ConstRef::Literal(Literal::Int(n, None)));
                    } else {
                        len = Some(ConstRef::Error);
                    }
                }
            }
            let inner = inner?;
            Some(TypeRef::Array {
                inner: Box::new(inner),
                len: len.unwrap_or(ConstRef::Error),
            })
        }
        SyntaxKind::TupleType => {
            let mut elems = Vec::new();
            for child in node.children().filter(is_type_node) {
                if let Some(ty) = lower_type_ref(&child, interner) {
                    elems.push(ty);
                }
            }
            Some(TypeRef::Tuple(elems))
        }
        SyntaxKind::NeverType => Some(TypeRef::Never),
        SyntaxKind::InferType => Some(TypeRef::Infer),
        SyntaxKind::DynType => {
            let inner = node.children().find(is_type_node);
            let inner_ref = if let Some(ty_node) = inner {
                lower_type_ref(&ty_node, interner)
            } else {
                let path = lower_path_from_type(node, interner)?;
                Some(TypeRef::Path(path))
            };
            inner_ref.map(|r| TypeRef::Dyn(Box::new(r)))
        }
        _ => {
            panic!("unhandled type node {:?}", node.kind());
        }
    }
}

pub(crate) fn lower_path_from_type(node: &SyntaxNode, interner: &mut Interner) -> Option<HirPath> {
    // NOTE: generic args belong to the *preceding* path segment; type nodes
    // emitted by parse_type_arg_list are siblings of the Ident tokens.
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
            glyim_syntax::SyntaxElement::Node(n) if super::is_type_node(&n) => {
                if let Some(ty) = lower_type_ref(&n, interner) {
                    pending_args.push(ty);
                }
            }
            _ => {}
        }
    }
    flush_pending(&mut segments, &mut pending_args);
    if segments.is_empty() {
        None
    } else {
        Some(HirPath {
            segments,
            kind: PathKind::Plain,
        })
    }
}
