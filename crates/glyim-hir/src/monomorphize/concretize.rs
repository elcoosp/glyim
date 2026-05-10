//! Type concretization: the ONLY place where Generic → Named conversion happens.
//!
//! After monomorphization, the codegen NEVER sees a `Generic` type.
//! It only sees `Named(mangled)`, primitives, `RawPtr(Named(mangled))`, etc.
//!
//! Every other component that needs to concretize a type MUST call
//! `concretize_type` from this module. No ad-hoc Generic resolution
//! is permitted anywhere else in the compiler.

use crate::monomorphize::index::MonoIndex;
use crate::monomorphize::mangle_table::MangleTable;
use crate::types::HirType;
use glyim_interner::{Interner, Symbol};

/// Convert a type to its fully concrete form.
///
/// Rules:
/// - `Generic("Vec", [Int])` → `Named("Vec__i64")` if Vec is a known struct/enum
/// - `Generic("Option", [Int])` → `Named("Option__i64")` if Option is a known enum
/// - `Generic("Result", [Int, Str])` → `Named("Result__int_str")` if Result is a known enum
/// - `RawPtr(Generic("Vec", [Int]))` → `RawPtr(Named("Vec__i64"))`
/// - `Named("T")` where T is a single uppercase letter → `Named("T")` with a warning
/// - Primitives → themselves
/// - `Tuple([...])` → recursively concretize elements
/// - `Func([...], ...)` → recursively concretize params and return
///
/// **This function takes ownership of `ty` and returns a new `HirType`.**
/// It does not mutate any input.
pub fn concretize_type(
    ty: HirType,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> HirType {
    match ty {
        // ── The core conversion ──────────────────────────────────────
        HirType::Generic(sym, args) => {
            // Recursively concretize the type arguments first.
            let concrete_args: Vec<HirType> = args
                .into_iter()
                .map(|a| concretize_type(a, index, mangle_table, interner))
                .collect();

            // If this is a known struct or enum, mangle the name.
            if index.find_struct(sym).is_some() || index.find_enum(sym).is_some() {
                let mangled = mangle_table.mangle(sym, &concrete_args, interner);
                HirType::Named(mangled)
            } else {
                // Unknown generic — leave as Generic. This shouldn't happen
                // in sound output but we don't panic here; the assertion
                // pass catches it later.
                HirType::Generic(sym, concrete_args)
            }
        }

        // ── Wrapper types: recurse into inner ───────────────────────
        HirType::RawPtr(inner) => {
            let concrete_inner = concretize_type(*inner, index, mangle_table, interner);
            HirType::RawPtr(Box::new(concrete_inner))
        }

        HirType::Option(inner) => {
            let concrete_inner = concretize_type(*inner, index, mangle_table, interner);

            // Check if Option is a known enum in the program.
            let opt_sym = interner.intern("Option");
            if index.find_enum(opt_sym).is_some() {
                let mangled = mangle_table.mangle(opt_sym, &[concrete_inner.clone()], interner);
                HirType::Named(mangled)
            } else {
                HirType::Option(Box::new(concrete_inner))
            }
        }

        HirType::Result(ok, err) => {
            let concrete_ok = concretize_type(*ok, index, mangle_table, interner);
            let concrete_err = concretize_type(*err, index, mangle_table, interner);

            let res_sym = interner.intern("Result");
            if index.find_enum(res_sym).is_some() {
                let mangled = mangle_table.mangle(
                    res_sym,
                    &[concrete_ok.clone(), concrete_err.clone()],
                    interner,
                );
                HirType::Named(mangled)
            } else {
                HirType::Result(Box::new(concrete_ok), Box::new(concrete_err))
            }
        }

        HirType::Tuple(elems) => {
            let concrete_elems: Vec<HirType> = elems
                .into_iter()
                .map(|e| concretize_type(e, index, mangle_table, interner))
                .collect();
            HirType::Tuple(concrete_elems)
        }

        HirType::Func(params, ret) => {
            let concrete_params: Vec<HirType> = params
                .into_iter()
                .map(|p| concretize_type(p, index, mangle_table, interner))
                .collect();
            let concrete_ret = concretize_type(*ret, index, mangle_table, interner);
            HirType::Func(concrete_params, Box::new(concrete_ret))
        }

        // ── Leaf types: pass through unchanged ──────────────────────
        HirType::Named(sym) => {
            // Warn if this looks like an unresolved type parameter.
            // This indicates a bug in the substitution logic.
            let s = interner.resolve(sym);
            if s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase()) {
                tracing::warn!(
                    "concretize_type: unresolved type param '{}' leaked through — \
                     substitution bug?",
                    s
                );
            }
            HirType::Named(sym)
        }

        HirType::Int
        | HirType::Bool
        | HirType::Float
        | HirType::Str
        | HirType::Unit
        | HirType::Never
        | HirType::Error
        | HirType::Opaque(_) => ty,
    }
}

/// Check if a type contains an unresolved type parameter.
///
/// An unresolved type parameter is a `Named(sym)` where `sym` resolves
/// to a single uppercase letter (e.g., "T", "K", "V").
///
/// This is used as a guard before enqueuing specialization work items:
/// if any type arg has an unresolved param, the specialization cannot
/// proceed and must be deferred or skipped.
pub fn has_unresolved_type_param(ty: &HirType, interner: &Interner) -> bool {
    match ty {
        HirType::Named(sym) => {
            let s = interner.resolve(*sym);
            s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase())
        }
        HirType::Generic(_, args) => args.iter().any(|a| has_unresolved_type_param(a, interner)),
        HirType::RawPtr(inner) => has_unresolved_type_param(inner, interner),
        HirType::Option(inner) => has_unresolved_type_param(inner, interner),
        HirType::Result(ok, err) => {
            has_unresolved_type_param(ok, interner) || has_unresolved_type_param(err, interner)
        }
        HirType::Tuple(elems) => elems.iter().any(|e| has_unresolved_type_param(e, interner)),
        HirType::Func(params, ret) => {
            params.iter().any(|p| has_unresolved_type_param(p, interner))
                || has_unresolved_type_param(ret, interner)
        }
        // Primitives, Opaque, Never, Error are always concrete
        _ => false,
    }
}

/// Build a type substitution map from formal type parameters to concrete type arguments.
///
/// Zips the two vectors. If `args` is shorter than `params`, the remaining
/// params are left unsubstituted (not mapped). If `args` is longer, the
/// extra args are ignored.
///
/// This is a pure function with no side effects.
pub fn build_subst(params: &[Symbol], args: &[HirType]) -> std::collections::HashMap<Symbol, HirType> {
    params.iter().zip(args.iter()).map(|(p, a)| (*p, a.clone())).collect()
}

/// Apply substitution then concretization in one step.
///
/// Convenience function used throughout the specialization pipeline.
/// Equivalent to `concretize_type(substitute_type(ty, sub), ...)`.
pub fn substitute_and_concretize(
    ty: &HirType,
    sub: &std::collections::HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> HirType {
    let substituted = crate::types::substitute_type(ty, sub);
    concretize_type(substituted, index, mangle_table, interner)
}

/// Apply substitution and concretization to a slice of types.
pub fn substitute_and_concretize_slice(
    types: &[HirType],
    sub: &std::collections::HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> Vec<HirType> {
    types
        .iter()
        .map(|t| substitute_and_concretize(t, sub, index, mangle_table, interner))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{EnumDef, StructDef, StructField};
    use crate::types::HirType;
    use glyim_diag::Span;
    use glyim_interner::Interner;

    /// Build a minimal MonoIndex containing a `Vec<T>` struct and `Option<T>` enum.
    fn build_test_index(interner: &mut Interner) -> (crate::Hir, MonoIndex) {
        let vec_sym = interner.intern("Vec");
        let opt_sym = interner.intern("Option");
        let t_sym = interner.intern("T");

        let hir = crate::Hir {
            items: vec![
                crate::HirItem::Struct(StructDef {
                    doc: None,
                    name: vec_sym,
                    type_params: vec![t_sym],
                    fields: vec![StructField {
                        name: interner.intern("data"),
                        ty: HirType::Int,
                        doc: None,
                    }],
                    span: Span::new(0, 0),
                    is_pub: false,
                }),
                crate::HirItem::Enum(EnumDef {
                    doc: None,
                    name: opt_sym,
                    type_params: vec![t_sym],
                    variants: vec![],
                    span: Span::new(0, 0),
                    is_pub: false,
                }),
            ],
        };

        let index = MonoIndex::build(&hir);
        (hir, index)
    }

    #[test]
    fn concretize_primitive_passthrough() {
        let mut interner = Interner::new();
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        assert_eq!(
            concretize_type(HirType::Int, &index, &mut mangle_table, &mut interner),
            HirType::Int
        );
        assert_eq!(
            concretize_type(HirType::Bool, &index, &mut mangle_table, &mut interner),
            HirType::Bool
        );
        assert_eq!(
            concretize_type(HirType::Float, &index, &mut mangle_table, &mut interner),
            HirType::Float
        );
        assert_eq!(
            concretize_type(HirType::Str, &index, &mut mangle_table, &mut interner),
            HirType::Str
        );
        assert_eq!(
            concretize_type(HirType::Unit, &index, &mut mangle_table, &mut interner),
            HirType::Unit
        );
    }

    #[test]
    fn concretize_generic_vec_int() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        let input = HirType::Generic(vec_sym, vec![HirType::Int]);
        let result = concretize_type(input, &index, &mut mangle_table, &mut interner);

        match &result {
            HirType::Named(sym) => {
                let name = interner.resolve(*sym);
                assert!(
                    name.starts_with("Vec"),
                    "Expected Vec prefix, got {}",
                    name
                );
                assert!(
                    name.contains("i64"),
                    "Expected i64 in name, got {}",
                    name
                );
            }
            other => panic!("Expected Named, got {:?}", other),
        }
    }

    #[test]
    fn concretize_raw_ptr_generic() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        let input = HirType::RawPtr(Box::new(HirType::Generic(vec_sym, vec![HirType::Int])));
        let result = concretize_type(input, &index, &mut mangle_table, &mut interner);

        match &result {
            HirType::RawPtr(inner) => {
                match inner.as_ref() {
                    HirType::Named(sym) => {
                        let name = interner.resolve(*sym);
                        assert!(
                            name.starts_with("Vec"),
                            "Expected Vec prefix inside RawPtr, got {}",
                            name
                        );
                    }
                    other => panic!("Expected Named inside RawPtr, got {:?}", other),
                }
            }
            other => panic!("Expected RawPtr, got {:?}", other),
        }
    }

    #[test]
    fn concretize_option_int() {
        let mut interner = Interner::new();
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        let input = HirType::Option(Box::new(HirType::Int));
        let result = concretize_type(input, &index, &mut mangle_table, &mut interner);

        match &result {
            HirType::Named(sym) => {
                let name = interner.resolve(*sym);
                assert!(
                    name.starts_with("Option"),
                    "Expected Option prefix, got {}",
                    name
                );
            }
            other => panic!("Expected Named, got {:?}", other),
        }
    }

    #[test]
    fn concretize_nested_generic() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        // Vec<Option<Int>> — inner Option gets concretized first
        let inner = HirType::Option(Box::new(HirType::Int));
        let input = HirType::Generic(vec_sym, vec![inner]);
        let result = concretize_type(input, &index, &mut mangle_table, &mut interner);

        match &result {
            HirType::Named(sym) => {
                let name = interner.resolve(*sym);
                assert!(
                    name.starts_with("Vec__"),
                    "Expected Vec__ prefix, got {}",
                    name
                );
            }
            other => panic!("Expected Named, got {:?}", other),
        }
    }

    #[test]
    fn concretize_unknown_generic_stays_generic() {
        let mut interner = Interner::new();
        let unknown_sym = interner.intern("Unknown");
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        let input = HirType::Generic(unknown_sym, vec![HirType::Int]);
        let result = concretize_type(input, &index, &mut mangle_table, &mut interner);

        // Unknown generic stays as Generic (not in the index)
        assert!(
            matches!(result, HirType::Generic(sym, _) if sym == unknown_sym),
            "Unknown generic should stay as Generic, got {:?}",
            result
        );
    }

    #[test]
    fn has_unresolved_detects_type_param() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let k = interner.intern("K");

        assert!(has_unresolved_type_param(&HirType::Named(t), &interner));
        assert!(has_unresolved_type_param(&HirType::Named(k), &interner));
        assert!(!has_unresolved_type_param(&HirType::Int, &interner));
        assert!(!has_unresolved_type_param(&HirType::Named(interner.intern("Vec")), &interner));
    }

    #[test]
    fn has_unresolved_nested() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let vec_sym = interner.intern("Vec");

        // Generic("Vec", [Named("T")]) has unresolved T
        assert!(has_unresolved_type_param(
            &HirType::Generic(vec_sym, vec![HirType::Named(t)]),
            &interner
        ));

        // Generic("Vec", [Int]) is fully concrete
        assert!(!has_unresolved_type_param(
            &HirType::Generic(vec_sym, vec![HirType::Int]),
            &interner
        ));
    }

    #[test]
    fn has_unresolved_raw_ptr() {
        let mut interner = Interner::new();
        let t = interner.intern("T");

        assert!(has_unresolved_type_param(
            &HirType::RawPtr(Box::new(HirType::Named(t))),
            &interner
        ));
        assert!(!has_unresolved_type_param(
            &HirType::RawPtr(Box::new(HirType::Int)),
            &interner
        ));
    }

    #[test]
    fn build_subst_basic() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let u = interner.intern("U");

        let sub = build_subst(&[t, u], &[HirType::Int, HirType::Bool]);

        assert_eq!(sub.get(&t), Some(&HirType::Int));
        assert_eq!(sub.get(&u), Some(&HirType::Bool));
    }

    #[test]
    fn build_subst_uneven() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let u = interner.intern("U");

        // More params than args: U is not mapped
        let sub = build_subst(&[t, u], &[HirType::Int]);
        assert_eq!(sub.get(&t), Some(&HirType::Int));
        assert_eq!(sub.get(&u), None);

        // More args than params: extra arg ignored
        let sub2 = build_subst(&[t], &[HirType::Int, HirType::Bool]);
        assert_eq!(sub2.len(), 1);
    }

    #[test]
    fn substitute_and_concretize_combines() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let t_sym = interner.intern("T");
        let (_, index) = build_test_index(&mut interner);
        let mut mangle_table = MangleTable::new();

        let ty = HirType::Generic(vec_sym, vec![HirType::Named(t_sym)]);
        let sub = std::collections::HashMap::from([(t_sym, HirType::Int)]);

        let result = substitute_and_concretize(&ty, &sub, &index, &mut mangle_table, &mut interner);

        // Named("Vec__i64") — substitution applied then concretized
        match &result {
            HirType::Named(sym) => {
                let name = interner.resolve(*sym);
                assert!(name.contains("Vec"), "Expected Vec in name, got {}", name);
                assert!(name.contains("i64"), "Expected i64 in name, got {}", name);
            }
            other => panic!("Expected Named, got {:?}", other),
        }
    }
}
