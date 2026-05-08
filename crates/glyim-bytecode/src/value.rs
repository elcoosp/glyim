use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Struct(Vec<Value>),
    Enum(u32, Box<Value>),
    Tuple(Vec<Value>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "int", Self::Float(_) => "float",
            Self::Bool(_) => "bool", Self::Str(_) => "str",
            Self::Unit => "unit", Self::Struct(_) => "struct",
            Self::Enum(_, _) => "enum", Self::Tuple(_) => "tuple",
        }
    }
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b, Self::Int(n) => *n != 0,
            Self::Str(s) => !s.is_empty(), Self::Unit => false,
            _ => true,
        }
    }
    pub fn expect_int(&self) -> Result<i64, String> {
        match self { Self::Int(n) => Ok(*n), o => Err(format!("expected int, got {}", o.type_name())) }
    }
    pub fn expect_bool(&self) -> Result<bool, String> {
        match self { Self::Bool(b) => Ok(*b), o => Err(format!("expected bool, got {}", o.type_name())) }
    }
    pub fn expect_float(&self) -> Result<f64, String> {
        match self { Self::Float(f) => Ok(*f), o => Err(format!("expected float, got {}", o.type_name())) }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(n) => write!(f, "{:.6}", n),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Str(s) => write!(f, "{}", s),
            Self::Unit => write!(f, "()"),
            Self::Struct(fields) => {
                write!(f, "{{")?;
                for (i, v) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Self::Enum(tag, payload) => write!(f, "Enum({}, {})", tag, payload),
            Self::Tuple(elems) => {
                write!(f, "(")?;
                for (i, v) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
        }
    }
}
