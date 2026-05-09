//! Pattern substitution for monomorphization.

use crate::types::{HirPattern, HirType};
use glyim_interner::Symbol;
use glyim_diag::Span;

/// Trait for name mangling and type concretization, used by pattern substitution.
pub trait MangleContext {
    fn mangle_name(&mut self, base: Symbol, args: &[HirType]) -> Symbol;
    fn concretize_type(&mut self, ty: HirType) -> HirType;
    fn intern_str(&mut self, s: &str) -> Symbol;
}

pub fn substitute_pattern(
    pattern: &HirPattern,
    scrutinee_ty: &HirType,
    ctx: &mut impl MangleContext,
) -> HirPattern {
    match pattern {
        HirPattern::EnumVariant {
            enum_name, variant_name, bindings, span,
        } => {
            let new_enum_name = mangle_enum_name(*enum_name, scrutinee_ty, ctx);
            HirPattern::EnumVariant {
                enum_name: new_enum_name,
                variant_name: *variant_name,
                bindings: bindings.clone(),
                span: *span,
            }
        }

        HirPattern::Struct { name, bindings, span } => {
            let new_name = mangle_struct_name(*name, scrutinee_ty, ctx);
            HirPattern::Struct {
                name: new_name,
                bindings: bindings.clone(),
                span: *span,
            }
        }

        HirPattern::OptionSome(inner) => {
            let opt_sym = ctx.intern_str("Option");
            if let HirType::Generic(_, args) = scrutinee_ty {
                if let Some(inner_ty) = args.first() {
                    let concrete_inner = ctx.concretize_type(inner_ty.clone());
                    let mangled = ctx.mangle_name(opt_sym, &[concrete_inner]);
                    let new_inner = Box::new(substitute_pattern(inner, &inner_ty.clone(), ctx));
                    return HirPattern::EnumVariant {
                        enum_name: mangled,
                        variant_name: ctx.intern_str("Some"),
                        bindings: vec![(ctx.intern_str("0"), *new_inner)],
                        span: Span::new(0, 0),
                    };
                }
            }
            let new_inner = Box::new(substitute_pattern(inner, scrutinee_ty, ctx));
            HirPattern::OptionSome(new_inner)
        }

        HirPattern::OptionNone => {
            let opt_sym = ctx.intern_str("Option");
            if let HirType::Generic(_, args) = scrutinee_ty {
                if let Some(inner_ty) = args.first() {
                    let concrete_inner = ctx.concretize_type(inner_ty.clone());
                    let mangled = ctx.mangle_name(opt_sym, &[concrete_inner]);
                    return HirPattern::EnumVariant {
                        enum_name: mangled,
                        variant_name: ctx.intern_str("None"),
                        bindings: vec![],
                        span: Span::new(0, 0),
                    };
                }
            }
            HirPattern::OptionNone
        }

        HirPattern::ResultOk(inner) => {
            let res_sym = ctx.intern_str("Result");
            if let HirType::Generic(_, args) = scrutinee_ty {
                if args.len() == 2 {
                    let concrete_ok = ctx.concretize_type(args[0].clone());
                    let concrete_err = ctx.concretize_type(args[1].clone());
                    let mangled = ctx.mangle_name(res_sym, &[concrete_ok, concrete_err]);
                    let new_inner = Box::new(substitute_pattern(inner, &args[0].clone(), ctx));
                    return HirPattern::EnumVariant {
                        enum_name: mangled,
                        variant_name: ctx.intern_str("Ok"),
                        bindings: vec![(ctx.intern_str("0"), *new_inner)],
                        span: Span::new(0, 0),
                    };
                }
            }
            let new_inner = Box::new(substitute_pattern(inner, scrutinee_ty, ctx));
            HirPattern::ResultOk(new_inner)
        }

        HirPattern::ResultErr(inner) => {
            let res_sym = ctx.intern_str("Result");
            if let HirType::Generic(_, args) = scrutinee_ty {
                if args.len() == 2 {
                    let concrete_ok = ctx.concretize_type(args[0].clone());
                    let concrete_err = ctx.concretize_type(args[1].clone());
                    let mangled = ctx.mangle_name(res_sym, &[concrete_ok, concrete_err]);
                    let new_inner = Box::new(substitute_pattern(inner, &args[1].clone(), ctx));
                    return HirPattern::EnumVariant {
                        enum_name: mangled,
                        variant_name: ctx.intern_str("Err"),
                        bindings: vec![(ctx.intern_str("0"), *new_inner)],
                        span: Span::new(0, 0),
                    };
                }
            }
            let new_inner = Box::new(substitute_pattern(inner, scrutinee_ty, ctx));
            HirPattern::ResultErr(new_inner)
        }

        HirPattern::Tuple { elements, span } => {
            let new_elements: Vec<HirPattern> = elements
                .iter()
                .map(|e| substitute_pattern(e, scrutinee_ty, ctx))
                .collect();
            HirPattern::Tuple { elements: new_elements, span: *span }
        }

        HirPattern::Wild
        | HirPattern::BoolLit(_)
        | HirPattern::IntLit(_)
        | HirPattern::FloatLit(_)
        | HirPattern::StrLit(_)
        | HirPattern::Unit
        | HirPattern::Var(_) => pattern.clone(),
    }
}

fn mangle_enum_name(
    original_name: Symbol,
    scrutinee_ty: &HirType,
    ctx: &mut impl MangleContext,
) -> Symbol {
    match scrutinee_ty {
        HirType::Named(mangled) => *mangled,
        HirType::Generic(base, args) => {
            let concrete_args: Vec<HirType> =
                args.iter().map(|a| ctx.concretize_type(a.clone())).collect();
            ctx.mangle_name(*base, &concrete_args)
        }
        _ => original_name,
    }
}

fn mangle_struct_name(
    original_name: Symbol,
    scrutinee_ty: &HirType,
    ctx: &mut impl MangleContext,
) -> Symbol {
    match scrutinee_ty {
        HirType::Named(mangled) => *mangled,
        HirType::Generic(base, args) => {
            let concrete_args: Vec<HirType> =
                args.iter().map(|a| ctx.concretize_type(a.clone())).collect();
            ctx.mangle_name(*base, &concrete_args)
        }
        _ => original_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HirType;
    use crate::monomorphize::mangling;
    use glyim_interner::Interner;

    struct TestCtx<'a> {
        interner: &'a mut Interner,
    }
    impl MangleContext for TestCtx<'_> {
        fn mangle_name(&mut self, base: Symbol, args: &[HirType]) -> Symbol {
            mangling::mangle_type_name(self.interner, base, args)
        }
        fn concretize_type(&mut self, ty: HirType) -> HirType { ty }
        fn intern_str(&mut self, s: &str) -> Symbol { self.interner.intern(s) }
    }

    #[test]
    fn substitute_pattern_enum_variant_with_generic_scrutinee() {
        let mut interner = Interner::new();
        let opt_sym = interner.intern("Option");
        let some_sym = interner.intern("Some");
        let pattern = HirPattern::EnumVariant {
            enum_name: opt_sym, variant_name: some_sym, bindings: vec![], span: Span::new(0,0),
        };
        let scrutinee_ty = HirType::Generic(opt_sym, vec![HirType::Int]);
        let mut ctx = TestCtx { interner: &mut interner };
        let result = substitute_pattern(&pattern, &scrutinee_ty, &mut ctx);
        if let HirPattern::EnumVariant { enum_name, .. } = result {
            let name = interner.resolve(enum_name);
            assert!(name.contains("Option") && name.contains("i64"));
        } else { panic!("Expected EnumVariant"); }
    }

    #[test]
    fn substitute_pattern_option_some_converts_to_enum_variant() {
        let mut interner = Interner::new();
        let x_sym = interner.intern("x");
        let pattern = HirPattern::OptionSome(Box::new(HirPattern::Var(x_sym)));
        let scrutinee_ty = HirType::Option(Box::new(HirType::Int));
        let mut ctx = TestCtx { interner: &mut interner };
        let result = substitute_pattern(&pattern, &scrutinee_ty, &mut ctx);
        match result {
            HirPattern::EnumVariant { enum_name, variant_name, .. } => {
                let name = interner.resolve(enum_name);
                assert!(name.contains("Option"));
                assert_eq!(interner.resolve(variant_name), "Some");
            }
            other => panic!("Expected EnumVariant, got {:?}", other),
        }
    }
}
