//! Runtime values produced by const evaluation.

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
}

impl ConstValue {
    /// Validate that this value fits within the range of its declared type.
    pub fn validate_range(&self, pointer_width: u32) -> Option<ConstValue> {
        match self {
            ConstValue::Int(v, IntTy::I8) => {
                if *v >= i8::MIN as i128 && *v <= i8::MAX as i128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I16) => {
                if *v >= i16::MIN as i128 && *v <= i16::MAX as i128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I32) => {
                if *v >= i32::MIN as i128 && *v <= i32::MAX as i128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::I64) => {
                if *v >= i64::MIN as i128 && *v <= i64::MAX as i128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Int(v, IntTy::Isize) => {
                let max = if pointer_width == 64 {
                    i64::MAX as i128
                } else {
                    i32::MAX as i128
                };
                let min = if pointer_width == 64 {
                    i64::MIN as i128
                } else {
                    i32::MIN as i128
                };
                if *v >= min && *v <= max {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U8) => {
                if *v <= u8::MAX as u128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U16) => {
                if *v <= u16::MAX as u128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U32) => {
                if *v <= u32::MAX as u128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::U64) => {
                if *v <= u64::MAX as u128 {
                    Some(self.clone())
                } else {
                    None
                }
            }
            ConstValue::Uint(v, UintTy::Usize) => {
                let max = if pointer_width == 64 {
                    u64::MAX as u128
                } else {
                    u32::MAX as u128
                };
                if *v <= max { Some(self.clone()) } else { None }
            }
            ConstValue::Range(..) => Some(self.clone()),
            _ => Some(self.clone()),
        }
    }

    /// is_bool.
    pub fn is_bool(&self) -> bool {
        matches!(self, ConstValue::Bool(_))
    }

    /// as_bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ConstValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// as_i128.
    pub fn as_i128(&self) -> Option<i128> {
        match self {
            ConstValue::Int(v, _) => Some(*v),
            ConstValue::Uint(v, _) => Some(*v as i128),
            _ => None,
        }
    }

    /// as_u128.
    pub fn as_u128(&self) -> Option<u128> {
        match self {
            ConstValue::Int(v, _) => Some(*v as u128),
            ConstValue::Uint(v, _) => Some(*v),
            _ => None,
        }
    }

    /// as_f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ConstValue::FloatBits(bits, _) => Some(f64::from_bits(*bits)),
            _ => None,
        }
    }

    /// checked_add.
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

    /// checked_sub.
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

    /// checked_mul.
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

    /// checked_div.
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

    /// checked_rem.
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

    /// checked_neg.
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
    pub fn not(&self) -> Option<ConstValue> {
        match self {
            ConstValue::Bool(b) => Some(ConstValue::Bool(!b)),
            ConstValue::Int(v, ty) => Some(ConstValue::Int(!v, *ty)),
            ConstValue::Uint(v, ty) => Some(ConstValue::Uint(!v, *ty)),
            _ => None,
        }
    }
}
