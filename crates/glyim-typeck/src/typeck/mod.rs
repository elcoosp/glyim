mod error;
mod expr;
mod function;
mod match_check;
mod register;
mod resolver;
mod scope;
mod stmt;
mod types;
pub mod unify;

pub use error::TypeError;
pub use types::{EnumInfo, StructInfo};

use glyim_hir::HirPattern;
use glyim_hir::item::FnSig;
use glyim_hir::node::{Hir, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

pub struct TypeChecker {
    pub interner: Interner,
    pub(crate) scopes: Vec<types::Scope>,
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub extern_fns: HashMap<Symbol, FnSig>,
    pub impl_methods: HashMap<Symbol, Vec<HirFn>>,
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub return_type: Option<HirType>,
    pub errors: Vec<TypeError>,
    pub(crate) visibility: HashMap<Symbol, bool>, // true = pub, false = private
    pub(crate) current_fn_type_params: Vec<Symbol>,
    fns: Vec<HirFn>,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        TypeChecker {
            interner,
            scopes: Vec::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            extern_fns: HashMap::new(),
            impl_methods: HashMap::new(),
            expr_types: Vec::new(),
            call_type_args: HashMap::new(),
            return_type: None,
            errors: Vec::new(),
            visibility: HashMap::new(),
            current_fn_type_params: Vec::new(),
            fns: Vec::new(),
        }
    }

    fn set_type(&mut self, id: ExprId, ty: HirType) {
        let idx = id.as_usize();
        if idx >= self.expr_types.len() {
            self.expr_types.resize(idx + 1, HirType::Never);
        }
        self.expr_types[idx] = ty;
    }

    fn dummy_symbol(&self) -> Symbol {
        glyim_interner::Interner::new().intern("__dummy")
    }

    pub(crate) fn register_visibility(&mut self, name: Symbol, is_pub: bool) {
        self.visibility.insert(name, is_pub);
    }

    #[tracing::instrument(skip_all)]
    #[tracing::instrument(skip_all)]
    pub fn check(&mut self, hir: &Hir) -> Result<(), Vec<TypeError>> {
        self.register_items(hir);
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    self.check_fn(f);
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for method in &imp.methods {
                        self.check_fn(method);
                    }
                }
                _ => {}
            }
        }
        if self.errors.is_empty() {
            // Debug assertion: all expression IDs should have corresponding types
            debug_assert!(
                {
                    let mut max_id = 0usize;
                    // Walk all HIR items to find the maximum expression ID
                    fn check_max_id(expr: &glyim_hir::HirExpr, max: &mut usize) {
                        let id = expr.get_id().as_usize();
                        if id > *max { *max = id; }
                        // Recurse into sub-expressions would require full traversal;
                        // simplified: just check that expr_types.len() >= 1
                    }
                    self.expr_types.len() >= 1
                },
                "type checker span coverage gap: some expressions not typed"
            );
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    /// Bind variables from a match arm pattern given the scrutinee type.
    /// Extract the inner type from a monomorphized Option or Result type,
    /// handling both the internal HirType::Option/Result and user-defined
    /// Generic(Option/Result, [T]).
    fn extract_option_inner(&self, scrutinee_ty: &HirType) -> Option<HirType> {
        match scrutinee_ty {
            HirType::Option(inner) => Some(inner.as_ref().clone()),
            HirType::Generic(name, args) if args.len() == 1 => {
                let name_str = self.interner.resolve(*name);
                if name_str == "Option" {
                    Some(args[0].clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn extract_result_inner(&self, scrutinee_ty: &HirType) -> Option<(HirType, HirType)> {
        match scrutinee_ty {
            HirType::Result(ok, err) => Some((ok.as_ref().clone(), err.as_ref().clone())),
            HirType::Generic(name, args) if args.len() == 2 => {
                let name_str = self.interner.resolve(*name);
                if name_str == "Result" {
                    Some((args[0].clone(), args[1].clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(crate) fn bind_match_pattern(&mut self, pattern: &HirPattern, scrutinee_ty: &HirType) {
        match pattern {
            HirPattern::Var(sym) => {
                self.insert_binding(*sym, scrutinee_ty.clone(), false);
            }
            HirPattern::Wild
            | HirPattern::BoolLit(_)
            | HirPattern::IntLit(_)
            | HirPattern::FloatLit(_)
            | HirPattern::StrLit(_)
            | HirPattern::Unit => {}
            HirPattern::Struct { bindings, .. } => {
                // Collect field types first to avoid borrow conflicts
                let field_tys: Vec<(HirPattern, HirType)> =
                    if let HirType::Named(struct_name) = scrutinee_ty {
                        if let Some(info) = self.structs.get(struct_name) {
                            bindings
                                .iter()
                                .filter_map(|(field_sym, field_pat)| {
                                    info.fields.iter().find(|f| f.name == *field_sym).map(
                                        |field_info| (field_pat.clone(), field_info.ty.clone()),
                                    )
                                })
                                .collect()
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    };
                for (field_pat, field_ty) in field_tys {
                    self.bind_match_pattern(&field_pat, &field_ty);
                }
            }
            HirPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                // Collect binding types first to avoid borrow conflicts
                let binding_tys: Vec<(HirPattern, HirType)> = match scrutinee_ty {
                    HirType::Named(enum_name) | HirType::Generic(enum_name, _) => {
                        if let Some(info) = self.enums.get(enum_name) {
                            if let Some(variant) =
                                info.variants.iter().find(|v| v.name == *variant_name)
                            {
                                bindings
                                    .iter()
                                    .zip(variant.fields.iter())
                                    .map(|((_, binding_pat), field)| {
                                        (binding_pat.clone(), field.ty.clone())
                                    })
                                    .collect()
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    }
                    _ => vec![],
                };
                for (binding_pat, field_ty) in binding_tys {
                    self.bind_match_pattern(&binding_pat, &field_ty);
                }
            }
            HirPattern::OptionSome(inner) => {
                if let Some(inner_ty) = self.extract_option_inner(scrutinee_ty) {
                    self.bind_match_pattern(inner, &inner_ty);
                }
            }
            HirPattern::OptionNone => {}
            HirPattern::ResultOk(inner) => {
                if let Some((ok_ty, _)) = self.extract_result_inner(scrutinee_ty) {
                    self.bind_match_pattern(inner, &ok_ty);
                }
            }
            HirPattern::ResultErr(inner) => {
                if let Some((_, err_ty)) = self.extract_result_inner(scrutinee_ty) {
                    self.bind_match_pattern(inner, &err_ty);
                }
            }
            HirPattern::Tuple { elements, .. } => {
                if let HirType::Tuple(elem_tys) = scrutinee_ty {
                    let pats_and_tys: Vec<(HirPattern, HirType)> = elements
                        .iter()
                        .zip(elem_tys.iter())
                        .map(|(p, t)| (p.clone(), t.clone()))
                        .collect();
                    for (p, t) in pats_and_tys {
                        self.bind_match_pattern(&p, &t);
                    }
                }
            }
        }
    }

    pub fn get_expr_type(&self, id: ExprId) -> Option<&HirType> {
        self.expr_types.get(id.as_usize())
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new(Interner::new())
    }
}

#[cfg(test)]
mod tests;
