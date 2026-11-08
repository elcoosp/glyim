//! Runtime values produced by const evaluation.

use glyim_core::def_id::FnDefId;
use glyim_core::interner::Name;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};

/// A value produced by constant evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    /// Signed integer value with its specific type.
    Int(i128, IntTy),
    /// Unsigned integer value with its specific type.
    Uint(u128, UintTy),
    /// Float value stored as bits with its specific type.
    FloatBits(u64, FloatTy),
    /// Boolean value.
    Bool(bool),
    /// Character value.
    Char(char),
    /// String value (interned name).
    String(Name),
    /// Unit value `()`.
    Unit,
    /// Tuple value.
    Tuple(Vec<ConstValue>),
    /// Array value.
    Array(Vec<ConstValue>),
    /// Struct value (fields in order of definition).
    Struct(Vec<(Name, ConstValue)>),
    /// Range value (`start..end` / `start..=end`). Either bound may be absent
    /// (open-ended range). Produced by constant evaluation of `Expr::Range`.
    Range(Option<Box<ConstValue>>, Option<Box<ConstValue>>, bool),
    /// A bare struct name used as a value — a unit (zero-field) struct's
    /// constructor, e.g. `const GLOBAL: Global = Global;`. Carries the
    /// struct's `AdtId` so lowering can build the (empty) aggregate. Only
    /// produced for zero-field structs; a struct with fields is not a value
    /// and the path falls through to the usual diagnostic.
    StructCtor(glyim_core::def_id::AdtId),
    /// A constant enum value: `adt_id` + `variant_idx` select the
    /// discriminant, `fields` are the variant's payload (empty for a unit
    /// variant). Produced by constant evaluation of a path naming an enum
    /// variant, e.g. `const K: C = C::B;`.
    Variant(
        glyim_core::def_id::AdtId,
        glyim_core::def_id::VariantIdx,
        Vec<ConstValue>,
    ),
    /// Reference to a top-level `fn` item (its def id). Produced by
    /// constant evaluation of a `Path` that resolves to an fn def, so a
    /// `const F: fn(..) = some_fn;` / `static F: fn(..) = some_fn;` can
    /// fold to a `MirConstKind::Fn`.
    Fn(FnDefId),
}

impl ConstValue {
    /// Validate that this value fits within the range of its declared type.
    #[must_use]
    pub fn validate_range(&self, pointer_width: u32) -> Option<ConstValue> {
        match self {
            ConstValue::Int(v, IntTy::I8) => {
                if *v >= i128::from(i8::MIN) && *v <= i128::from(i8::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I16) => {
                if *v >= i128::from(i16::MIN) && *v <= i128::from(i16::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I32) => {
                if *v >= i128::from(i32::MIN) && *v <= i128::from(i32::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I64) => {
                if *v >= i128::from(i64::MIN) && *v <= i128::from(i64::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::Isize) => {
                let max = if pointer_width == 64 {
                    i128::from(i64::MAX)
                } else {
                    i128::from(i32::MAX)
                };
                let min = if pointer_width == 64 {
                    i128::from(i64::MIN)
                } else {
                    i128::from(i32::MIN)
                };
                if *v >= min && *v <= max {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U8) => {
                if *v <= u128::from(u8::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U16) => {
                if *v <= u128::from(u16::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U32) => {
                if *v <= u128::from(u32::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U64) => {
                if *v <= u128::from(u64::MAX) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::Usize) => {
                let max = if pointer_width == 64 {
                    u128::from(u64::MAX)
                } else {
                    u128::from(u32::MAX)
                };
                if *v <= max { Some(self.clone()) } else { None }
            }
            ConstValue::Range(..) => Some(self.clone()),
            _ => Some(self.clone()),
        }
    }

    /// `is_bool`.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, ConstValue::Bool(_))
    }

    /// `as_bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ConstValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// `as_i128`.
    #[must_use]
    pub fn as_i128(&self) -> Option<i128> {
        match self {
            ConstValue::Int(v, _) => Some(*v),
            ConstValue::Uint(v, _) => Some(*v as i128),
            _ => None,
        }
    }

    /// `as_u128`.
    #[must_use]
    pub fn as_u128(&self) -> Option<u128> {
        match self {
            ConstValue::Int(v, _) => Some(*v as u128),
            ConstValue::Uint(v, _) => Some(*v),
            _ => None,
        }
    }

    /// `as_f64`.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ConstValue::FloatBits(bits, _) => Some(f64::from_bits(*bits)),
            _ => None,
        }
    }

    /// `checked_add`.
    #[must_use]
    pub fn checked_add(&self, other: &ConstValue) -> Option<ConstValue> {
        match (self, other) {
            (ConstValue::Int(a, ty_a), ConstValue::Int(b, ty_b)) if ty_a == ty_b => {
                a.checked_add(*b).map(|v| ConstValue::Int(v, *ty_a))
            }
            (ConstValue::Uint(a, ty_a), ConstValue::Uint(b, ty_b)) if ty_a == ty_b => {
                a.checked_add(*b).map(|v| ConstValue::Uint(v, *ty_a))
            }
            (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
                Some(ConstValue::FloatBits(
                    (f64::from_bits(*a) + f64::from_bits(*b)).to_bits(),
                    *ty_a,
                ))
            }
            _ => None,
        }
    }

    /// `checked_sub`.
    #[must_use]
    pub fn checked_sub(&self, other: &ConstValue) -> Option<ConstValue> {
        match (self, other) {
            (ConstValue::Int(a, ty_a), ConstValue::Int(b, ty_b)) if ty_a == ty_b => {
                a.checked_sub(*b).map(|v| ConstValue::Int(v, *ty_a))
            }
            (ConstValue::Uint(a, ty_a), ConstValue::Uint(b, ty_b)) if ty_a == ty_b => {
                a.checked_sub(*b).map(|v| ConstValue::Uint(v, *ty_a))
            }
            (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
                Some(ConstValue::FloatBits(
                    (f64::from_bits(*a) - f64::from_bits(*b)).to_bits(),
                    *ty_a,
                ))
            }
            _ => None,
        }
    }

    /// `checked_mul`.
    #[must_use]
    pub fn checked_mul(&self, other: &ConstValue) -> Option<ConstValue> {
        match (self, other) {
            (ConstValue::Int(a, ty_a), ConstValue::Int(b, ty_b)) if ty_a == ty_b => {
                a.checked_mul(*b).map(|v| ConstValue::Int(v, *ty_a))
            }
            (ConstValue::Uint(a, ty_a), ConstValue::Uint(b, ty_b)) if ty_a == ty_b => {
                a.checked_mul(*b).map(|v| ConstValue::Uint(v, *ty_a))
            }
            (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
                Some(ConstValue::FloatBits(
                    (f64::from_bits(*a) * f64::from_bits(*b)).to_bits(),
                    *ty_a,
                ))
            }
            _ => None,
        }
    }

    /// `checked_div`.
    #[must_use]
    pub fn checked_div(&self, other: &ConstValue) -> Option<ConstValue> {
        match (self, other) {
            (ConstValue::Int(a, ty_a), ConstValue::Int(b, ty_b)) if ty_a == ty_b => {
                a.checked_div(*b).map(|v| ConstValue::Int(v, *ty_a))
            }
            (ConstValue::Uint(a, ty_a), ConstValue::Uint(b, ty_b)) if ty_a == ty_b => {
                a.checked_div(*b).map(|v| ConstValue::Uint(v, *ty_a))
            }
            (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
                let bv = f64::from_bits(*b);
                if bv == 0.0 {
                    Some(ConstValue::FloatBits(f64::INFINITY.to_bits(), *ty_a)) // or NaN
                } else {
                    Some(ConstValue::FloatBits(
                        (f64::from_bits(*a) / bv).to_bits(),
                        *ty_a,
                    ))
                }
            }
            _ => None,
        }
    }

    /// `checked_rem`.
    #[must_use]
    pub fn checked_rem(&self, other: &ConstValue) -> Option<ConstValue> {
        match (self, other) {
            (ConstValue::Int(a, ty_a), ConstValue::Int(b, ty_b)) if ty_a == ty_b => {
                a.checked_rem(*b).map(|v| ConstValue::Int(v, *ty_a))
            }
            (ConstValue::Uint(a, ty_a), ConstValue::Uint(b, ty_b)) if ty_a == ty_b => {
                a.checked_rem(*b).map(|v| ConstValue::Uint(v, *ty_a))
            }
            (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
                let bv = f64::from_bits(*b);
                if bv == 0.0 {
                    Some(ConstValue::FloatBits(f64::NAN.to_bits(), *ty_a))
                } else {
                    Some(ConstValue::FloatBits(
                        (f64::from_bits(*a) % bv).to_bits(),
                        *ty_a,
                    ))
                }
            }
            _ => None,
        }
    }

    /// `checked_neg`.
    #[must_use]
    pub fn checked_neg(&self) -> Option<ConstValue> {
        match self {
            ConstValue::Int(v, ty) => v.checked_neg().map(|r| ConstValue::Int(r, *ty)),
            ConstValue::FloatBits(bits, ty) => Some(ConstValue::FloatBits(
                (-f64::from_bits(*bits)).to_bits(),
                *ty,
            )),
            _ => None,
        }
    }

    /// not.
    #[must_use]
    pub fn not(&self) -> Option<ConstValue> {
        match self {
            ConstValue::Bool(b) => Some(ConstValue::Bool(!b)),
            ConstValue::Int(v, ty) => Some(ConstValue::Int(!v, *ty)),
            ConstValue::Uint(v, ty) => Some(ConstValue::Uint(!v, *ty)),
            _ => None,
        }
    }
}
