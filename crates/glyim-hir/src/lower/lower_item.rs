use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::*;
use glyim_diag::GlyimDiagnostic;
use glyim_syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

use crate::{
    AssociatedTy, Body, BodyId, ConstItem, EnumItem, Field, FnItem, GenericParam, GenericParamKind,
    ImplItem, ImplMethod, Item, ItemId, ItemKind, ModItem, Param, Pat, PatId, Path, StructItem,
    TraitItem, TraitMethod, TypeAliasItem, TypeRef, Variant, Visibility,
};

/// Collect generic type parameters from a `TypeParamList` child node (e.g. the
/// `<T, U>` of `struct S<T, U>` / `enum E<T>` / `fn f<T>`). The parser emits a
/// `TypeParamList` of `TypeParam` nodes; a type-param whose body is a bare
/// identifier exposes its name only as a *token* (not a child node), so use
/// `first_ident_text`.
pub(crate) fn collect_generic_params(
    node: &SyntaxNode,
    interner: &mut Interner,
) -> Vec<GenericParam> {
    let mut generic_params = Vec::new();
    if let Some(tp_list) = node.children().find(|c| c.kind() == SyntaxKind::TypeParamList) {
        for tp in tp_list.children().filter(|c| c.kind() == SyntaxKind::TypeParam) {
            if let Some(name_str) = first_ident_text(&tp) {
                let name = interner.intern(&name_str);
                // Capture trait bounds declared after `:`, e.g. the `MyFuture`
                // in `F: MyFuture`. The bound is a type node child of the
                // `TypeParam` (the `:` itself is a token, not a node).
                let bounds: Vec<TypeRef> = tp
                    .children()
                    .filter(is_type_node)
                    .filter_map(|b| lower_type_ref(&b, interner))
                    .collect();
                generic_params.push(GenericParam {
                    name,
                    kind: GenericParamKind::Type {
                        default: None,
                        bounds,
                    },
                    span: node_span(&tp),
                });
            }
        }
    }
    generic_params
}
use super::{
    first_ident_text, first_ident_text_with_depth, is_type_node, lower_expr::lower_block_to_expr,
    lower_expr::lower_expr, lower_type::lower_type_ref, next_local_def_id, node_span,
};

pub(crate) fn collect_struct_fields(
    node: &SyntaxNode,
    interner: &mut Interner,
) -> Option<(Name, Vec<Name>)> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);
    let mut fields = Vec::new();
    let tokens: Vec<_> = node.children_with_tokens().collect();
    let mut i = 0;
    while i < tokens.len() {
        if let glyim_syntax::SyntaxElement::Token(t) = &tokens[i]
            && t.kind() == SyntaxKind::Ident
            && i + 2 < tokens.len()
            && let glyim_syntax::SyntaxElement::Token(col) = &tokens[i + 1]
            && col.kind() == SyntaxKind::Colon
        {
            fields.push(interner.intern(t.text()));
            i += 3;
            continue;
        }
        i += 1;
    }
    Some((name, fields))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_fn_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);

    let mut params = Vec::new();
    let mut return_ty = None;
    let owner = next_local_def_id(local_def_counter);
    let mut body_params = Vec::new();

    // Temporary pats storage for parameters (will be moved into body if present)
    let mut temp_pats = IndexVec::new();

    // Collect parameters
    for child in node.children() {
        if child.kind() == SyntaxKind::ParamList {
            for param_node in child.children().filter(|c| c.kind() == SyntaxKind::Param) {
                let (p, pat_id) = lower_param(&param_node, interner, &mut temp_pats);
                params.push(p);
                body_params.push(pat_id);
            }
        }
    }

    // Collect generic type parameters (plan §9.A): the parser emits a
    // `TypeParamList` of `TypeParam` nodes for `fn f<T, U>(...)`. Previously
    // this was hardcoded to `Vec::new()`, so a generic parameter name like `T`
    // never reached typeck and resolved as `unresolved type T`. A `TypeParam`
    // whose body is a bare identifier exposes that name only as a *token*
    // (not a child node), so use `first_ident_text`, which scans
    // `children_with_tokens`.
    let mut generic_params = Vec::new();
    if let Some(tp_list) = node.children().find(|c| c.kind() == SyntaxKind::TypeParamList) {
        for tp in tp_list.children().filter(|c| c.kind() == SyntaxKind::TypeParam) {
            if let Some(name_str) = first_ident_text(&tp) {
                let name = interner.intern(&name_str);
                let bounds: Vec<TypeRef> = tp
                    .children()
                    .filter(is_type_node)
                    .filter_map(|b| lower_type_ref(&b, interner))
                    .collect();
                generic_params.push(crate::GenericParam {
                    name,
                    kind: crate::GenericParamKind::Type {
                        default: None,
                        bounds,
                    },
                    span: node_span(&tp),
                });
            }
        }
    }

    // Parse return type
    let mut arrow_seen = false;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Arrow => {
                arrow_seen = true;
            }
            glyim_syntax::SyntaxElement::Node(n) if arrow_seen && is_type_node(&n) => {
                return_ty = lower_type_ref(&n, interner);
                arrow_seen = false;
            }
            _ => {}
        }
    }

    // Body or foreign
    let body_id = if let Some(block_node) = node.children().find(|c| c.kind() == SyntaxKind::Block)
    {
        tracing::debug!("Found Block node in FnDef, lowering to expr");
        let mut body = Body {
            owner,
            exprs: IndexVec::new(),
            pats: temp_pats,
            params: body_params,
            span: node_span(node),
            expr_spans: IndexVec::new(),
        };
        lower_block_to_expr(&block_node, interner, &mut body, diags, struct_field_map);
        let bid = bodies.push(body);
        body_owners.push(owner);
        Some(bid)
    } else {
        tracing::debug!("FnDef without Block node treated as foreign function");
        None
    };

    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;

    // Plan §6.1: detect `const fn` / `async fn` modifiers (the parser embeds
    // the leading modifier keyword token inside the FnDef node, before `fn`).
    let mut is_const = false;
    let mut is_async = false;
    let mut abi: Option<Name> = None;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) => match t.kind() {
                SyntaxKind::KwFn => break,
                SyntaxKind::KwConst => is_const = true,
                SyntaxKind::KwAsync => is_async = true,
                SyntaxKind::KwExtern => {
                    // An `extern "C"` fn. The ABI string literal (if any) is a
                    // sibling token inside the FnDef node (unstub-5 Phase 4).
                    if let Some(next) = node
                        .children_with_tokens()
                        .find(|c| c.kind() == SyntaxKind::StringLit)
                        && let glyim_syntax::SyntaxElement::Token(st) = next
                    {
                        // Strip the surrounding quotes from the string
                        // literal so the ABI name is `C`, not `"C"`.
                        let text = st.text();
                        let trimmed = text
                            .strip_prefix('"')
                            .and_then(|t| t.strip_suffix('"'))
                            .unwrap_or(text);
                        abi = Some(interner.intern(trimmed));
                    }
                    if abi.is_none() {
                        // Bare `extern fn` defaults to the C ABI.
                        abi = Some(interner.intern("C"));
                    }
                }
                _ => {}
            },
            glyim_syntax::SyntaxElement::Node(_) => {}
        }
    }

    Some(Item {
        id,
        name,
        kind: ItemKind::Fn(FnItem {
            params,
            return_ty,
            body: body_id,
            is_unsafe: false,
            is_async,
            is_const,
            generic_params,
            where_clauses: Vec::new(),
            abi,
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}
pub(crate) fn lower_param(
    node: &SyntaxNode,
    interner: &mut Interner,
    pats: &mut IndexVec<PatId, Pat>,
) -> (Param, PatId) {
    let name_text = first_ident_text_with_depth(node).unwrap_or_else(|| "_".to_string());
    let name = interner.intern(&name_text);
    let ty = node
        .children()
        .find(is_type_node)
        .and_then(|n| lower_type_ref(&n, interner));
    let pat = if name_text == "_" {
        Pat::Wild
    } else {
        let mutability = if node.children_with_tokens().any(
            |c| matches!(&c, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwMut),
        ) {
            Mutability::Mut
        } else {
            Mutability::Not
        };
        Pat::Binding {
            name,
            mutability,
            subpattern: None,
        }
    };
    let pat_id = pats.push(pat);
    (
        Param {
            name,
            ty,
            span: node_span(node),
        },
        pat_id,
    )
}

pub(crate) fn lower_struct_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    _local_def_counter: &mut u32,
    item_id_counter: &mut u32,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);
    let mut fields = Vec::new();
    let kind;
    let tokens: Vec<_> = node.children_with_tokens().collect();
    let mut i = 0;
    let mut has_fields = false;
    while i < tokens.len() {
        if let glyim_syntax::SyntaxElement::Token(t) = &tokens[i]
            && t.kind() == SyntaxKind::Ident
            && i + 2 < tokens.len()
            && let glyim_syntax::SyntaxElement::Token(col) = &tokens[i + 1]
            && col.kind() == SyntaxKind::Colon
            && let glyim_syntax::SyntaxElement::Node(ty) = &tokens[i + 2]
            && is_type_node(ty)
        {
            let fname = interner.intern(t.text());
            let fty = lower_type_ref(ty, interner)?;
            fields.push(Field {
                name: fname,
                ty: fty,
                span: node_span(node),
            });
            has_fields = true;
            i += 3;
            continue;
        }
        i += 1;
    }
    if !has_fields {
        let mut tuple_types = Vec::new();
        for child in node.children() {
            if child.kind() == SyntaxKind::TupleType {
                for ty_node in child.children().filter(is_type_node) {
                    if let Some(fty) = lower_type_ref(&ty_node, interner) {
                        tuple_types.push(fty);
                    }
                }
            }
        }
        if !tuple_types.is_empty() {
            for fty in tuple_types {
                fields.push(Field {
                    name: interner.intern(""),
                    ty: fty,
                    span: node_span(node),
                });
            }
            kind = StructKind::Tuple;
        } else {
            kind = StructKind::Unit;
        }
    } else {
        kind = StructKind::Record;
    }
    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Struct(StructItem {
            fields,
            kind,
            generic_params: collect_generic_params(node, interner),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

pub(crate) fn lower_enum_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    _local_def_counter: &mut u32,
    item_id_counter: &mut u32,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);
    let mut variants = Vec::new();
    if let Some(variant_list) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::VariantList)
    {
        for vnode in variant_list
            .children()
            .filter(|c| c.kind() == SyntaxKind::EnumVariant)
        {
            if let Some(variant) = lower_variant(&vnode, interner) {
                variants.push(variant);
            }
        }
    }
    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Enum(EnumItem {
            variants,
            generic_params: collect_generic_params(node, interner),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

pub(crate) fn lower_variant(node: &SyntaxNode, interner: &mut Interner) -> Option<Variant> {
    let vname_str = first_ident_text(node)?;
    let vname = interner.intern(&vname_str);
    let mut fields = Vec::new();
    let kind;
    let mut in_paren = false;
    let mut has_tuple = false;
    for child in node.children_with_tokens() {
        match child {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::LParen => {
                in_paren = true;
            }
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::RParen => {
                in_paren = false;
            }
            glyim_syntax::SyntaxElement::Node(n) if in_paren && is_type_node(&n) => {
                let fty = lower_type_ref(&n, interner)?;
                fields.push(Field {
                    name: interner.intern(""),
                    ty: fty,
                    span: node_span(node),
                });
                has_tuple = true;
            }
            _ => {}
        }
    }
    let mut has_record = false;
    for child in node.children() {
        if child.kind() == SyntaxKind::FieldList {
            for fnode in child
                .children()
                .filter(|c| c.kind() == SyntaxKind::StructField)
            {
                let fname_str = first_ident_text(&fnode).unwrap_or_default();
                let fname = interner.intern(&fname_str);
                let fty = fnode
                    .children()
                    .find(is_type_node)
                    .and_then(|n| lower_type_ref(&n, interner))?;
                fields.push(Field {
                    name: fname,
                    ty: fty,
                    span: node_span(node),
                });
                has_record = true;
            }
        }
    }
    if has_record {
        kind = StructKind::Record;
    } else if has_tuple {
        kind = StructKind::Tuple;
    } else {
        kind = StructKind::Unit;
    }
    Some(Variant {
        name: vname,
        fields,
        kind,
        span: node_span(node),
    })
}

/// Lower an `impl` block into `ItemKind::Impl`. Inherent impls
/// (`impl Foo {}`) and trait impls (`impl Trait for Foo {}`) are both
/// supported; each method is lowered to an `ImplMethod` with its own body
/// (mirroring `lower_fn_def` body lowering) so the type checker and LSP can
/// resolve method receiver types (Tier 6.4).
pub(crate) fn lower_impl_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<Item> {
    let mut self_ty: Option<TypeRef> = None;
    let mut trait_ref: Option<Path> = None;
    // `for` is emitted as a *token* (not a node) by the parser for
    // `impl Trait for Self`. When present, the first type node before `for`
    // is the trait and the first type node after `for` is `Self`. When absent
    // (a plain `impl Foo`), the lone type node is `Self`.
    let saw_for_token = node
        .children_with_tokens()
        .any(|el| matches!(el, glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwFor));
    if saw_for_token {
        let mut saw_for = false;
        for el in node.children_with_tokens() {
            match el {
                glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwFor => {
                    saw_for = true;
                }
                glyim_syntax::SyntaxElement::Node(child) if is_type_node(&child) => {
                    if saw_for {
                        // After `for`: this is the `Self` type.
                        if self_ty.is_none() {
                            self_ty = lower_type_ref(&child, interner);
                        }
                    } else if trait_ref.is_none() {
                        // Before `for`: this is the trait.
                        if let TypeRef::Path(p) = lower_type_ref(&child, interner)? {
                            trait_ref = Some(p);
                        }
                    }
                }
                _ => {}
            }
        }
    } else if let Some(child) = node.children().find(is_type_node) {
        self_ty = lower_type_ref(&child, interner);
    }
    let self_ty = self_ty?;

    let mut methods = Vec::new();
    for method_node in node.children().filter(|c| c.kind() == SyntaxKind::FnDef) {
        let mname_str = first_ident_text(&method_node)?;
        let mname = interner.intern(&mname_str);

        let owner = next_local_def_id(local_def_counter);
        let mut params = Vec::new();
        let mut body_params = Vec::new();
        let mut temp_pats = IndexVec::new();
        for child in method_node.children() {
            if child.kind() == SyntaxKind::ParamList {
                for param_node in child.children().filter(|c| c.kind() == SyntaxKind::Param) {
                    let (p, pat_id) = lower_param(&param_node, interner, &mut temp_pats);
                    params.push(p);
                    body_params.push(pat_id);
                }
            }
        }

        let mut return_ty = None;
        let mut arrow_seen = false;
        for el in method_node.children_with_tokens() {
            match el {
                glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Arrow => {
                    arrow_seen = true;
                }
                glyim_syntax::SyntaxElement::Node(n) if arrow_seen && is_type_node(&n) => {
                    return_ty = lower_type_ref(&n, interner);
                    arrow_seen = false;
                }
                _ => {}
            }
        }

        let body_id = if let Some(block_node) = method_node
            .children()
            .find(|c| c.kind() == SyntaxKind::Block)
        {
            let mut body = Body {
                owner,
                exprs: IndexVec::new(),
                pats: temp_pats,
                params: body_params,
                span: node_span(&method_node),
                expr_spans: IndexVec::new(),
            };
            lower_block_to_expr(&block_node, interner, &mut body, diags, struct_field_map);
            let bid = bodies.push(body);
            body_owners.push(owner);
            Some(bid)
        } else {
            None
        };

        methods.push(ImplMethod {
            name: mname,
            body: body_id,
            params,
            return_ty,
        });
    }

    // Derive a diagnostic name for the impl from its self type.
    let name = match &self_ty {
        TypeRef::Path(p) => p
            .segments
            .last()
            .map(|s| s.name)
            .unwrap_or_else(|| interner.intern("impl")),
        _ => interner.intern("impl"),
    };

    // Capture associated-type definitions (`type Output = i32;`) from the impl
    // body so projection (`Self::Output` / `F::Output`) has something to
    // resolve against. The parser emits a `TypeAlias` node with an `Ident`
    // (name) and a `Type` (the `=` RHS).
    let mut associated_types = Vec::new();
    for ta_node in node.children().filter(|c| c.kind() == SyntaxKind::TypeAlias) {
        let name_str = first_ident_text(&ta_node);
        let Some(name_str) = name_str else { continue };
        let name = interner.intern(&name_str);
        // The defining type is the first type node child (the RHS after `=`).
        let ty = ta_node
            .children()
            .find(is_type_node)
            .and_then(|t| lower_type_ref(&t, interner));
        associated_types.push(AssociatedTy {
            name,
            bounds: Vec::new(),
            default: ty,
        });
    }

    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Impl(ImplItem {
            trait_ref,
            self_ty,
            methods,
            generic_params: collect_generic_params(node, interner),
            where_clauses: Vec::new(),
            associated_types,
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

/// Lower a `trait Name { fn method(...) -> ...; ... }` item into
/// `ItemKind::Trait`. Mirrors `lower_impl_def` for method extraction; the trait
/// body's `fn`s become `TraitMethod` entries (with optional default bodies),
/// which typeck registers so trait paths resolve and default-method bodies are
/// reachable.
pub(crate) fn lower_trait_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);

    let mut methods = Vec::new();
    for method_node in node.children().filter(|c| c.kind() == SyntaxKind::FnDef) {
        let mname_str = first_ident_text(&method_node)?;
        let mname = interner.intern(&mname_str);

        let owner = next_local_def_id(local_def_counter);
        let mut params = Vec::new();
        let mut body_params = Vec::new();
        let mut temp_pats = IndexVec::new();
        for child in method_node.children() {
            if child.kind() == SyntaxKind::ParamList {
                for param_node in child.children().filter(|c| c.kind() == SyntaxKind::Param) {
                    let (p, pat_id) = lower_param(&param_node, interner, &mut temp_pats);
                    params.push(p);
                    body_params.push(pat_id);
                }
            }
        }

        let mut return_ty = None;
        let mut arrow_seen = false;
        for el in method_node.children_with_tokens() {
            match el {
                glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Arrow => {
                    arrow_seen = true;
                }
                glyim_syntax::SyntaxElement::Node(n) if arrow_seen && is_type_node(&n) => {
                    return_ty = lower_type_ref(&n, interner);
                    arrow_seen = false;
                }
                _ => {}
            }
        }

        let default_body = if let Some(block_node) = method_node
            .children()
            .find(|c| c.kind() == SyntaxKind::Block)
        {
            let mut body = Body {
                owner,
                exprs: IndexVec::new(),
                pats: temp_pats,
                params: body_params,
                span: node_span(&method_node),
                expr_spans: IndexVec::new(),
            };
            lower_block_to_expr(&block_node, interner, &mut body, diags, struct_field_map);
            let bid = bodies.push(body);
            body_owners.push(owner);
            Some(bid)
        } else {
            None
        };

        methods.push(TraitMethod {
            name: mname,
            params,
            return_ty,
            default_body,
        });
    }

    // Plan unstub-5 P5: capture the trait's associated-type declarations
    // (`type Output;`) so `TraitDef` knows the trait carries them. The parser
    // emits a `TypeAlias` node; the trait form has no RHS (`default = None`),
    // while an impl's `type Output = i32;` (lowered by `lower_impl_def`) does.
    let mut associated_types = Vec::new();
    for ta_node in node.children().filter(|c| c.kind() == SyntaxKind::TypeAlias) {
        let name_str = first_ident_text(&ta_node);
        let Some(name_str) = name_str else { continue };
        let name = interner.intern(&name_str);
        let ty = ta_node
            .children()
            .find(is_type_node)
            .and_then(|t| lower_type_ref(&t, interner));
        associated_types.push(AssociatedTy {
            name,
            bounds: Vec::new(),
            default: ty,
        });
    }

    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Trait(TraitItem {
            associated_types,
            methods,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

/// Lower a `const NAME: TYPE = EXPR;` item into `ItemKind::Const`.
///
/// The type annotation is lowered to a `TypeRef` (used by typeck to register
/// the constant's value type for value-namespace path resolution), and the
/// initializer expression is lowered into a `Body` so the constant can be
/// evaluated later (const value materialization is a follow-up).
pub(crate) fn lower_const_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);
    let owner = next_local_def_id(local_def_counter);

    // Type annotation: `const X: TYPE = ...`. The type node follows a `:`.
    let mut ty: Option<TypeRef> = None;
    let mut saw_colon = false;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Colon => {
                saw_colon = true;
            }
            glyim_syntax::SyntaxElement::Node(n) if saw_colon && is_type_node(&n) => {
                ty = lower_type_ref(&n, interner);
                break;
            }
            _ => {}
        }
    }
    let ty = ty?;

    // Const initializer: `const X: TYPE = EXPR;`. Find the `=` token, then
    // the expression node immediately following it, and lower that expression
    // into a fresh `Body` so the constant can be const-evaluated later (Part C:
    // const value materialization).
    let mut init_node: Option<SyntaxNode> = None;
    let mut saw_eq = false;
    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Eq => {
                saw_eq = true;
            }
            glyim_syntax::SyntaxElement::Node(n) if saw_eq && !is_type_node(&n) => {
                init_node = Some(n);
                break;
            }
            _ => {}
        }
    }
    let mut root_expr: Option<crate::ExprId> = None;
    let body_id: Option<BodyId> = if let Some(init) = init_node {
        let mut body = Body {
            owner,
            exprs: IndexVec::new(),
            pats: IndexVec::new(),
            params: Vec::new(),
            span: node_span(node),
            expr_spans: IndexVec::new(),
        };
        root_expr = lower_expr(&init, interner, &mut body, diags, struct_field_map);
        let bid = bodies.push(body);
        body_owners.push(owner);
        Some(bid)
    } else {
        None
    };

    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Const(ConstItem { ty, body: body_id, root_expr }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

/// Lower an inline `mod name { ... }` block into `ItemKind::Mod`.
///
/// The module's inner items are lowered by the same per-item lower functions
/// used at the crate root, and pushed into the shared `items`/`bodies`
/// collections so that they participate in type checking in source order
/// (which keeps their `LocalDefId`s aligned with the def-map that the
/// value-namespace path resolver walks). The returned `ModItem` records the
/// `ItemId`s of its children so the module structure is preserved for later
/// passes (privacy, path resolution).
pub(crate) fn lower_mod_def(
    node: &SyntaxNode,
    interner: &mut Interner,
    local_def_counter: &mut u32,
    item_id_counter: &mut u32,
    items: &mut IndexVec<ItemId, Item>,
    bodies: &mut IndexVec<BodyId, Body>,
    body_owners: &mut IndexVec<BodyId, LocalDefId>,
    diags: &mut Vec<GlyimDiagnostic>,
    struct_field_map: &HashMap<Name, Vec<Name>>,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);

    let mut children = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::FnDef => {
                if let Some(item) = lower_fn_def(
                    &child,
                    interner,
                    local_def_counter,
                    item_id_counter,
                    bodies,
                    body_owners,
                    diags,
                    struct_field_map,
                ) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::StructDef => {
                if let Some(item) =
                    lower_struct_def(&child, interner, local_def_counter, item_id_counter)
                {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::EnumDef => {
                if let Some(item) =
                    lower_enum_def(&child, interner, local_def_counter, item_id_counter)
                {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::ImplDef => {
                if let Some(item) = lower_impl_def(
                    &child,
                    interner,
                    local_def_counter,
                    item_id_counter,
                    bodies,
                    body_owners,
                    diags,
                    struct_field_map,
                ) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::TraitDef => {
                if let Some(item) = lower_trait_def(
                    &child,
                    interner,
                    local_def_counter,
                    item_id_counter,
                    bodies,
                    body_owners,
                    diags,
                    struct_field_map,
                ) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::Module => {
                if let Some(item) = lower_mod_def(
                    &child,
                    interner,
                    local_def_counter,
                    item_id_counter,
                    items,
                    bodies,
                    body_owners,
                    diags,
                    struct_field_map,
                ) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::ConstDef => {
                if let Some(item) = lower_const_def(
                    &child,
                    interner,
                    local_def_counter,
                    item_id_counter,
                    bodies,
                    body_owners,
                    diags,
                    struct_field_map,
                ) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            SyntaxKind::TypeAlias => {
                if let Some(item) = lower_type_alias(&child, interner, item_id_counter) {
                    children.push(item.id);
                    items.push(item);
                }
            }
            _ => {}
        }
    }

    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::Mod(ModItem { children }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}

/// Lower a top-level `type Name<..> = RHS;` type alias into
/// `ItemKind::TypeAlias` so the pipeline can register it for expansion during
/// type resolution (e.g. `type Result<T> = Result<T, Error>` → 1-arg
/// `Result<usize>` expands to `Result<usize, Error>`). stdlib-completion.
pub(crate) fn lower_type_alias(
    node: &SyntaxNode,
    interner: &mut Interner,
    item_id_counter: &mut u32,
) -> Option<Item> {
    let name_str = first_ident_text(node)?;
    let name = interner.intern(&name_str);
    let generic_params = collect_generic_params(node, interner);
    let ty = node
        .children()
        .find(is_type_node)
        .and_then(|t| lower_type_ref(&t, interner));
    let id = ItemId::from_raw(*item_id_counter);
    *item_id_counter += 1;
    Some(Item {
        id,
        name,
        kind: ItemKind::TypeAlias(TypeAliasItem {
            ty,
            generic_params,
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: node_span(node),
    })
}
