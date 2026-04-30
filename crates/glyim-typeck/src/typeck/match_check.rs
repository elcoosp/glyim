use crate::typeck::error::TypeError;
use crate::TypeChecker;
use glyim_hir::node::HirExpr;
use glyim_hir::types::HirType;
use glyim_hir::HirPattern;
use glyim_interner::Symbol;

impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_match_exhaustiveness(
        &mut self,
        scrutinee_type: &HirType,
        arms: &[(HirPattern, Option<HirExpr>, HirExpr)],
    ) {
        let enum_variants = self.get_enum_variants(scrutinee_type);
        if enum_variants.is_empty() {
            return;
        }

        let has_wildcard = arms
            .iter()
            .any(|(pat, _, _)| matches!(pat, HirPattern::Wild));
        if has_wildcard {
            return;
        }

        let covered = self.collect_covered_variants(arms);
        let missing: Vec<String> = enum_variants
            .iter()
            .filter(|v| !covered.contains(v))
            .map(|v| self.interner.resolve(*v).to_string())
            .collect();

        if !missing.is_empty() {
            self.errors.push(TypeError::NonExhaustiveMatch { missing });
        }
    }

    fn get_enum_variants(&mut self, scrutinee_type: &HirType) -> Vec<Symbol> {
        match scrutinee_type {
            HirType::Named(name) => {
                if let Some(info) = self.enums.get(name) {
                    info.variants.iter().map(|v| v.name).collect()
                } else {
                    self.get_builtin_enum_variants(name)
                }
            }
            HirType::Option(_) => {
                vec![self.interner.intern("Some"), self.interner.intern("None")]
            }
            HirType::Result(_, _) => {
                vec![self.interner.intern("Ok"), self.interner.intern("Err")]
            }
            _ => vec![],
        }
    }

    fn get_builtin_enum_variants(&mut self, name: &Symbol) -> Vec<Symbol> {
        let name_str = format!("{:?}", name);
        if name_str.contains("Option") {
            vec![self.interner.intern("Some"), self.interner.intern("None")]
        } else if name_str.contains("Result") {
            vec![self.interner.intern("Ok"), self.interner.intern("Err")]
        } else {
            vec![]
        }
    }

    fn collect_covered_variants(
        &mut self,
        arms: &[(HirPattern, Option<HirExpr>, HirExpr)],
    ) -> Vec<Symbol> {
        let some_sym = self.interner.intern("Some");
        let none_sym = self.interner.intern("None");
        let ok_sym = self.interner.intern("Ok");
        let err_sym = self.interner.intern("Err");
        arms.iter()
            .filter_map(|(pat, _, _)| match pat {
                HirPattern::EnumVariant { variant_name, .. } => Some(*variant_name),
                HirPattern::OptionSome(_) => Some(some_sym),
                HirPattern::OptionNone => Some(none_sym),
                HirPattern::ResultOk(_) => Some(ok_sym),
                HirPattern::ResultErr(_) => Some(err_sym),
                _ => None,
            })
            .collect()
    }
}
