use crate::ast::*;
use crate::parser::Parser;
use glyim_interner::{Interner, Symbol};
use glyim_lex::tokenize;

/// Holds the signatures of all top‑level items, without body lowering.
#[derive(Debug, Clone)]
pub struct DeclaredItems {
    pub structs: Vec<StructDefDecl>,
    pub enums: Vec<EnumDefDecl>,
    pub fns: Vec<FnSigDecl>,
    pub impls: Vec<ImplDecl>,
    pub externs: Vec<ExternBlockDecl>,
    pub interner: Interner,
}

/// Minimal struct declaration (name, type params, fields).
#[derive(Debug, Clone)]
pub struct StructDefDecl {
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub fields: Vec<(Symbol, Option<TypeExpr>)>,
    pub is_pub: bool,
}

/// Minimal enum declaration (name, type params, variant names and fields).
#[derive(Debug, Clone)]
pub struct EnumDefDecl {
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub variants: Vec<EnumVariantRepr>,
    pub is_pub: bool,
}

/// Minimal function signature.
#[derive(Debug, Clone)]
pub struct FnSigDecl {
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, Option<TypeExpr>, bool)>, // name, type, mutable
    pub ret: Option<TypeExpr>,
    pub attrs: Vec<Attribute>,
    pub is_pub: bool,
}

/// Minimal impl block declaration.
#[derive(Debug, Clone)]
pub struct ImplDecl {
    pub target_name: Symbol,
    pub type_params: Vec<Symbol>,
    pub methods: Vec<FnSigDecl>,
    pub is_pub: bool,
}

/// Minimal extern block declaration.
#[derive(Debug, Clone)]
pub struct ExternBlockDecl {
    pub abi: String,
    pub functions: Vec<ExternFn>,
}

/// Parse only the declarations (headers) from a source file.
pub fn parse_declarations(source: &str) -> crate::parser::ParseOutput<DeclaredItems> {
    let tokens = tokenize(source);
    let mut parser = Parser::new(&tokens);
    let mut decls = DeclaredItems {
        structs: vec![],
        enums: vec![],
        fns: vec![],
        impls: vec![],
        externs: vec![],
        interner: Interner::new(),
    };

    // We'll use the existing parser's parse_item method but with a flag to skip bodies.
    // The parser currently builds AST items. We'll convert them to declarations.
    let original_items = parser.parse_source_file_declarations_only(); // new method

    for item in &original_items.items {
        match item {
            Item::StructDef { name, type_params, fields, .. } => {
                decls.structs.push(StructDefDecl {
                    name: *name,
                    type_params: type_params.clone(),
                    fields: fields.iter().map(|(s, _, ty)| (*s, ty.clone())).collect(),
                    is_pub: false,
                });
            }
            Item::EnumDef { name, type_params, variants, .. } => {
                decls.enums.push(EnumDefDecl {
                    name: *name,
                    type_params: type_params.clone(),
                    variants: variants.clone(),
                    is_pub: false,
                });
            }
            Item::FnDef { name, type_params, params, ret, attrs, .. } => {
                decls.fns.push(FnSigDecl {
                    name: *name,
                    type_params: type_params.clone(),
                    params: params.iter().map(|(s, _, ty, m)| (*s, ty.clone(), *m)).collect(),
                    ret: ret.clone(),
                    attrs: attrs.clone(),
                    is_pub: false,
                });
            }
            Item::ImplBlock { target, type_params, methods, is_pub, .. } => {
                let fn_decls: Vec<FnSigDecl> = methods.iter().filter_map(|m| {
                    if let Item::FnDef { name, type_params, params, ret, attrs, .. } = m {
                        Some(FnSigDecl {
                            name: *name,
                            type_params: type_params.clone(),
                            params: params.iter().map(|(s, _, ty, m)| (*s, ty.clone(), *m)).collect(),
                            ret: ret.clone(),
                            attrs: attrs.clone(),
                            is_pub: false,
                        })
                    } else { None }
                }).collect();
                decls.impls.push(ImplDecl {
                    target_name: *target,
                    type_params: type_params.clone(),
                    methods: fn_decls,
                    is_pub: *is_pub,
                });
            }
            Item::ExternBlock { abi, functions, .. } => {
                decls.externs.push(ExternBlockDecl {
                    abi: abi.clone(),
                    functions: functions.clone(),
                });
            }
            _ => {}
        }
    }

    crate::parser::ParseOutput {
        ast: decls,
        errors: parser.errors,
        interner: parser.interner,
    }
}
