use crate::rep::Rep;
use crate::ty::Ty;
use glyim_interner::Symbol;

/// A lens: a first-class getter/setter pair for a field.
#[derive(Clone, Debug)]
pub struct Lens {
    pub field_name: Symbol,
    pub source_ty: Ty,
    pub target_ty: Ty,
    pub path: Vec<usize>,
}

/// A prism: a first-class constructor/matcher for a variant.
#[derive(Clone, Debug)]
pub struct Prism {
    pub ctor_name: Symbol,
    pub source_ty: Ty,
    pub target_ty: Ty,
}

#[derive(Clone, Debug)]
pub enum Optic {
    Lens(Lens),
    Prism(Prism),
}

/// Collect all fields from a Rep tree, returning (field_name, field_ty, path).
fn collect_fields(rep: &Rep, path: &mut Vec<usize>) -> Vec<(Symbol, Ty, Vec<usize>)> {
    match rep {
        Rep::Meta(_, inner) => collect_fields(inner, path),
        Rep::Constructor(_, inner) => collect_fields(inner, path),
        Rep::Product(a, b) => {
            let mut result = Vec::new();
            path.push(0);
            result.extend(collect_fields(a, path));
            path.pop();
            path.push(1);
            result.extend(collect_fields(b, path));
            path.pop();
            result
        }
        Rep::Field(meta, ty) => vec![(meta.name, *ty, path.clone())],
        Rep::Sum(_, _) => vec![],
        Rep::Unit => vec![],
    }
}

/// Generate a Lens for each field in a struct-like Rep.
pub fn generate_lenses(rep: &Rep) -> Vec<Lens> {
    let fields = collect_fields(rep, &mut vec![]);
    fields.into_iter().map(|(name, ty, path)| Lens {
        field_name: name,
        source_ty: Ty(0),
        target_ty: ty,
        path,
    }).collect()
}

/// Collect all constructors from a Rep tree.
fn collect_constructors(rep: &Rep) -> Vec<(Symbol, Vec<(Symbol, Ty)>)> {
    match rep {
        Rep::Meta(_, inner) => collect_constructors(inner),
        Rep::Sum(a, b) => {
            let mut result = collect_constructors(a);
            result.extend(collect_constructors(b));
            result
        }
        Rep::Constructor(meta, inner) => {
            let fields = collect_fields(inner, &mut vec![]);
            vec![(meta.name, fields.into_iter().map(|(n, t, _)| (n, t)).collect())]
        }
        _ => vec![],
    }
}

/// Generate a Prism for each single-field constructor in an enum-like Rep.
pub fn generate_prisms(rep: &Rep) -> Vec<Prism> {
    let ctors = collect_constructors(rep);
    ctors.into_iter()
        .filter_map(|(name, fields)| {
            if fields.len() == 1 {
                let (_, target_ty) = fields[0];
                Some(Prism { ctor_name: name, source_ty: Ty(0), target_ty })
            } else {
                None
            }
        })
        .collect()
}
