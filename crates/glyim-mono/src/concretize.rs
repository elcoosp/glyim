use crate::mangle_table::MangleTable;
use glyim_hir::mangling;
use crate::metadata::{TypeMetadata, TypeStructure};
use glyim_diag::Span;
use glyim_hir::types::HirType;
use glyim_interner::{Interner, Symbol};

#[derive(Debug, Clone, PartialEq)]
pub enum ConcretizeErrorKind {
    UnresolvedParam,
    UnresolvedInfer,
    ManglingFailed,
    IndexLookupFailed,
    StructuralFailure,
}

#[derive(Debug, Clone)]
pub struct ConcretizeError {
    pub kind: ConcretizeErrorKind,
    pub ty: Box<HirType>,
    pub detail: String,
    pub span: Span,
}

impl std::fmt::Display for ConcretizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ConcretizeError({:?}): {} - {:?}",
            self.kind, self.detail, self.ty
        )
    }
}

impl std::error::Error for ConcretizeError {}

pub fn concretize_and_register(
    ty: HirType,
    interner: &mut Interner,
    mangle_table: &mut MangleTable,
    metadata: &mut TypeMetadata,
    span: Span,
) -> Result<HirType, ConcretizeError> {
    match ty {
        HirType::Generic(sym, args) => {
            let mut concrete_args = Vec::with_capacity(args.len());
            for a in args {
                concrete_args.push(concretize_and_register(
                    a,
                    interner,
                    mangle_table,
                    metadata,
                    span,
                )?);
            }
            let mangled = mangle_table
                .mangle(sym, &concrete_args, interner)
                .map_err(|e| ConcretizeError {
                    kind: ConcretizeErrorKind::ManglingFailed,
                    ty: Box::new(HirType::Generic(sym, concrete_args.clone())),
                    detail: format!("{:?}", e),
                    span,
                })?;
            metadata.record(
                mangled,
                TypeStructure::Generic {
                    base: sym,
                    args: concrete_args.clone(),
                },
            );
            Ok(HirType::Named(mangled))
        }
        HirType::Named(sym) => {
            if metadata.get(sym).is_none() {
                metadata.record(sym, TypeStructure::Plain { base: sym });
            }
            Ok(HirType::Named(sym))
        }
        HirType::Int
        | HirType::Bool
        | HirType::Float
        | HirType::Str
        | HirType::Unit
        | HirType::Never
        | HirType::Error
        | HirType::Opaque(_) => Ok(ty),
        HirType::Tuple(elems) => {
            let concrete: Vec<HirType> = elems
                .into_iter()
                .map(|e| concretize_and_register(e, interner, mangle_table, metadata, span))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(HirType::Tuple(concrete))
        }
        HirType::RawPtr(inner) => Ok(HirType::RawPtr(Box::new(concretize_and_register(
            *inner,
            interner,
            mangle_table,
            metadata,
            span,
        )?))),
        HirType::Func(params, ret) => {
            let cp: Vec<HirType> = params
                .into_iter()
                .map(|p| concretize_and_register(p, interner, mangle_table, metadata, span))
                .collect::<Result<Vec<_>, _>>()?;
            let cr = concretize_and_register(*ret, interner, mangle_table, metadata, span)?;
            Ok(HirType::Func(cp, Box::new(cr)))
        }
        HirType::Option(inner) => {
            let concrete_inner =
                concretize_and_register(*inner, interner, mangle_table, metadata, span)?;
            // Treat Option as Generic for mangling
            let opt_sym = interner.intern("Option");
            let mangled = mangle_table
                .mangle(opt_sym, &[concrete_inner.clone()], interner)
                .map_err(|e| ConcretizeError {
                    kind: ConcretizeErrorKind::ManglingFailed,
                    ty: Box::new(HirType::Option(Box::new(concrete_inner.clone()))),
                    detail: format!("{:?}", e),
                    span,
                })?;
            metadata.record(
                mangled,
                TypeStructure::Generic {
                    base: opt_sym,
                    args: vec![concrete_inner],
                },
            );
            Ok(HirType::Named(mangled))
        }
        HirType::Result(ok, err) => {
            let concrete_ok = concretize_and_register(*ok, interner, mangle_table, metadata, span)?;
            let concrete_err =
                concretize_and_register(*err, interner, mangle_table, metadata, span)?;
            let res_sym = interner.intern("Result");
            let mangled = mangle_table
                .mangle(
                    res_sym,
                    &[concrete_ok.clone(), concrete_err.clone()],
                    interner,
                )
                .map_err(|e| ConcretizeError {
                    kind: ConcretizeErrorKind::ManglingFailed,
                    ty: Box::new(HirType::Result(
                        Box::new(concrete_ok.clone()),
                        Box::new(concrete_err.clone()),
                    )),
                    detail: format!("{:?}", e),
                    span,
                })?;
            metadata.record(
                mangled,
                TypeStructure::Generic {
                    base: res_sym,
                    args: vec![concrete_ok, concrete_err],
                },
            );
            Ok(HirType::Named(mangled))
        }
        HirType::Param(sym) => Err(ConcretizeError {
            kind: ConcretizeErrorKind::UnresolvedParam,
            ty: Box::new(HirType::Param(sym)),
            detail: format!("Param: {:?}", interner.resolve(sym)),
            span,
        }),
        HirType::Infer(var) => Err(ConcretizeError {
            kind: ConcretizeErrorKind::UnresolvedInfer,
            ty: Box::new(HirType::Infer(var)),
            detail: format!("TypeVar: ?{}", var.raw_index()),
            span,
        }),
    }
}

/// Check if a type contains an unresolved type parameter.
pub fn has_unresolved_type_param(ty: &HirType, interner: &Interner) -> bool {
    match ty {
        HirType::Named(sym) => {
            let s = interner.resolve(*sym);
            s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase())
        }
        HirType::Generic(_, args) => args.iter().any(|a| has_unresolved_type_param(a, interner)),
        HirType::RawPtr(inner) | HirType::Option(inner) => {
            has_unresolved_type_param(inner, interner)
        }
        HirType::Result(ok, err) => {
            has_unresolved_type_param(ok, interner) || has_unresolved_type_param(err, interner)
        }
        HirType::Tuple(elems) => elems.iter().any(|e| has_unresolved_type_param(e, interner)),
        HirType::Func(params, ret) => {
            params
                .iter()
                .any(|p| has_unresolved_type_param(p, interner))
                || has_unresolved_type_param(ret, interner)
        }
        _ => false,
    }
}

/// Build a type substitution map from formal type parameters to concrete type arguments.
pub fn build_subst(
    params: &[Symbol],
    args: &[HirType],
) -> std::collections::HashMap<Symbol, HirType> {
    params
        .iter()
        .zip(args.iter())
        .map(|(p, a)| (*p, a.clone()))
        .collect()
}

/// Apply substitution then concretization in one step.
pub fn substitute_and_concretize(
    ty: &HirType,
    sub: &std::collections::HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> HirType {
    let substituted = glyim_hir::types::substitute_type(ty, sub);
    concretize_and_register(
        substituted,
        interner,
        mangle_table,
        &mut TypeMetadata::new(),
        Span::new(0, 0),
    )
    .unwrap_or(HirType::Error)
}

// Placeholder for MonoIndex - will be properly implemented in the driver
pub struct MonoIndex;
impl MonoIndex {
    pub fn new() -> Self {
        Self
    }
    pub fn find_struct(&self, _: Symbol) -> Option<bool> {
        None
    }
    pub fn find_enum(&self, _: Symbol) -> Option<bool> {
        None
    }
}
