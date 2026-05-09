pub mod optics;

use crate::ty::Ty;
use glyim_interner::Symbol;

/// Metadata attached to Rep nodes (field names, constructor names, annotations).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepMeta {
    pub name: Symbol,
    pub annotations: Vec<Symbol>,
}

/// GHC-style generic representation of a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rep {
    /// Wrap with metadata (type name, annotations)
    Meta(RepMeta, Box<Rep>),
    /// Sum of two constructors (choice)
    Sum(Box<Rep>, Box<Rep>),
    /// Product of two fields (pair)
    Product(Box<Rep>, Box<Rep>),
    /// A single constructor with its fields
    Constructor(RepMeta, Box<Rep>),
    /// A single field
    Field(RepMeta, Ty),
    /// Empty / no fields
    Unit,
}

/// Build the Rep for a struct-like type with named fields.
pub fn build_rep_struct(
    type_name: Symbol,
    fields: &[(Symbol, Ty)],
    annotations: Vec<Symbol>,
) -> Rep {
    let inner = if fields.is_empty() {
        Rep::Unit
    } else {
        let mut iter = fields.iter().rev();
        let (last_name, last_ty) = iter.next().unwrap();
        let mut rep = Rep::Field(
            RepMeta {
                name: *last_name,
                annotations: vec![],
            },
            *last_ty,
        );
        for &(name, ty) in iter {
            rep = Rep::Product(
                Box::new(Rep::Field(
                    RepMeta {
                        name,
                        annotations: vec![],
                    },
                    ty,
                )),
                Box::new(rep),
            );
        }
        rep
    };
    Rep::Meta(
        RepMeta {
            name: type_name,
            annotations,
        },
        Box::new(Rep::Constructor(
            RepMeta {
                name: type_name,
                annotations: vec![],
            },
            Box::new(inner),
        )),
    )
}

/// Build the Rep for an enum-like type with multiple constructors.
pub fn build_rep_enum(
    type_name: Symbol,
    constructors: &[(Symbol, Vec<(Symbol, Ty)>)],
    annotations: Vec<Symbol>,
) -> Rep {
    // If no constructors, return a unit-like meta
    if constructors.is_empty() {
        return Rep::Meta(
            RepMeta {
                name: type_name,
                annotations,
            },
            Box::new(Rep::Unit),
        );
    }

    let mut iter = constructors.iter().rev();
    let &(first_ctor, ref first_fields) = iter.next().unwrap();
    let mut rep = Rep::Constructor(
        RepMeta {
            name: first_ctor,
            annotations: vec![],
        },
        Box::new(build_ctor_fields(first_fields)),
    );
    for &(ctor_name, ref fields) in iter {
        rep = Rep::Sum(
            Box::new(Rep::Constructor(
                RepMeta {
                    name: ctor_name,
                    annotations: vec![],
                },
                Box::new(build_ctor_fields(fields)),
            )),
            Box::new(rep),
        );
    }
    Rep::Meta(
        RepMeta {
            name: type_name,
            annotations,
        },
        Box::new(rep),
    )
}

fn build_ctor_fields(fields: &[(Symbol, Ty)]) -> Rep {
    if fields.is_empty() {
        Rep::Unit
    } else {
        let mut iter = fields.iter().rev();
        let (last_name, last_ty) = iter.next().unwrap();
        let mut rep = Rep::Field(
            RepMeta {
                name: *last_name,
                annotations: vec![],
            },
            *last_ty,
        );
        for &(name, ty) in iter {
            rep = Rep::Product(
                Box::new(Rep::Field(
                    RepMeta {
                        name,
                        annotations: vec![],
                    },
                    ty,
                )),
                Box::new(rep),
            );
        }
        rep
    }
}
