use crate::item::{HirVariant, StructField};
use crate::node::HirFn;
use crate::types::HirType;
use glyim_diag::Span;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// Pre‑resolved table of all top‑level declarations, built from parsing
/// headers only (phase 1).
#[derive(Debug, Clone)]
pub struct DeclTable {
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub functions: HashMap<Symbol, FnSigInfo>,
    pub impl_methods: HashMap<Symbol, Vec<HirFn>>,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: Vec<StructField>,
    pub field_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct FnSigInfo {
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, HirType, bool)>,
    pub ret: Option<HirType>,
    pub is_pub: bool,
}

impl DeclTable {
    /// Build the declaration table from DeclaredItems, using the given
    /// mutable Interner to create mangled names.
    pub fn from_declarations(decls: &crate::lower::DeclaredItems, interner: &mut Interner) -> Self {
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut functions = HashMap::new();
        let mut impl_methods: HashMap<Symbol, Vec<HirFn>> = HashMap::new();

        for s in &decls.structs {
            let mut field_map = HashMap::new();
            let fields: Vec<StructField> = s
                .fields
                .iter()
                .enumerate()
                .map(|(i, (sym, _ty))| {
                    field_map.insert(*sym, i);
                    StructField {
                        name: *sym,
                        ty: HirType::Int,
                        doc: None,
                    }
                })
                .collect();
            structs.insert(
                s.name,
                StructInfo {
                    fields,
                    field_map,
                    type_params: s.type_params.clone(),
                    is_pub: s.is_pub,
                },
            );
        }

        for e in &decls.enums {
            let mut variant_map = HashMap::new();
            let variants: Vec<HirVariant> = e
                .variants
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    variant_map.insert(v.name, i);
                    HirVariant {
                        name: v.name,
                        fields: vec![],
                        tag: i as u32,
                        doc: None,
                    }
                })
                .collect();
            enums.insert(
                e.name,
                EnumInfo {
                    variants,
                    variant_map,
                    type_params: e.type_params.clone(),
                    is_pub: e.is_pub,
                },
            );
        }

        for f in &decls.fns {
            let params: Vec<(Symbol, HirType, bool)> = f
                .params
                .iter()
                .map(|(sym, _ty, mutable)| (*sym, HirType::Int, *mutable))
                .collect();
            functions.insert(
                f.name,
                FnSigInfo {
                    type_params: f.type_params.clone(),
                    params,
                    ret: None,
                    is_pub: f.is_pub,
                },
            );
        }

        for imp in &decls.impls {
            let mut methods = Vec::new();
            for m in &imp.methods {
                let mangled = format!(
                    "{}_{}",
                    interner.resolve(imp.target_name),
                    interner.resolve(m.name)
                );
                let mangled_name = interner.intern(&mangled);
                let hir_params: Vec<(Symbol, HirType)> = m
                    .params
                    .iter()
                    .map(|(sym, _ty, _mut)| (*sym, HirType::Int))
                    .collect();
                let hir_method = HirFn {
                    doc: None,
                    name: mangled_name,
                    type_params: imp
                        .type_params
                        .iter()
                        .chain(&m.type_params)
                        .copied()
                        .collect(),
                    params: hir_params,
                    param_mutability: m.params.iter().map(|(_, _, mu)| *mu).collect(),
                    ret: None,
                    body: crate::node::HirExpr::IntLit {
                        id: crate::types::ExprId::new(0),
                        value: 0,
                        span: Span::new(0, 0),
                    },
                    span: Span::new(0, 0),
                    is_pub: imp.is_pub || m.is_pub,
                    is_macro_generated: false,
                    is_extern_backed: false,
                is_test: false,
                test_config: None,
                };
                methods.push(hir_method);
            }
            impl_methods.insert(imp.target_name, methods);
        }

        DeclTable {
            structs,
            enums,
            functions,
            impl_methods,
        }
    }
}
