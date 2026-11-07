//! Runtime values and the handler interpreter (design doc §7.3, §7.9).
//!
//! Handlers are **state transformers** `fn(&mut S, E)` (§7.3): the compiler
//! passes the component's state; the closure captures nothing mutable. To
//! make that semantics *executable and assertable* in tests (§11.4: the
//! headless host asserts the cost model from day one), this module provides
//! a small dynamic value model and an interpreter for [`crate::expr::Expr`]
//! handler bodies. The MIR-level story (handlers as real `.g` functions with
//! analyzable bodies) is unchanged: this interpreter *is* the observable
//! semantics those MIR bodies must reproduce.
//!
//! §7.9 `bind:value` desugars to a Prop slot reading `place` plus a
//! synthesized handler `|s, e| place = e.value()`; the interpreter runs that
//! synthesized handler exactly like a user-written one.

use crate::expr::{BinOp, Expr, Lit, UnOp};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A dynamic value: enough to run views and handlers in tests.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Signed integer.
    Int(i64),
    /// Float.
    Float(f64),
    /// Boolean.
    Bool(bool),
    /// String.
    Str(Rc<str>),
    /// Absent (`None`-ish).
    Null,
    /// A list (e.g. `s.items`).
    List(Rc<RefCell<Vec<Value>>>),
    /// A record (e.g. the state struct, a nested struct).
    Obj(Rc<RefCell<BTreeMap<String, Value>>>),
}

impl Value {
    /// Build an object value.
    pub fn obj(pairs: impl IntoIterator<Item = (impl Into<String>, Value)>) -> Value {
        Value::Obj(Rc::new(RefCell::new(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        )))
    }

    /// Build a list value.
    pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items.into_iter().collect())))
    }

    /// Numeric formatting for text interpolation (§6.1 Text slots).
    pub fn display(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => {
                // Shortest round-trip-ish display; integral floats drop `.0`.
                if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
                    format!("{}", *f as i64)
                } else {
                    format!("{}", f)
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.to_string(),
            Value::Null => String::new(),
            Value::List(_) | Value::Obj(_) => String::new(),
        }
    }

    /// Truthiness for `if`/guards.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Null => false,
            Value::List(l) => !l.borrow().is_empty(),
            Value::Obj(_) => true,
        }
    }

    fn num(&self) -> f64 {
        match self {
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            Value::Bool(b) => *b as i64 as f64,
            Value::Null => 0.0,
            _ => 0.0,
        }
    }

    fn is_int(&self) -> bool {
        matches!(self, Value::Int(_))
    }

    fn arith(&self, op: BinOp, rhs: &Value) -> Value {
        // Integer arithmetic stays integer; anything with a float goes float.
        if self.is_int() && rhs.is_int() {
            let (a, b) = (
                match self {
                    Value::Int(i) => *i,
                    _ => 0,
                },
                match rhs {
                    Value::Int(i) => *i,
                    _ => 0,
                },
            );
            let r = match op {
                BinOp::Add => a.wrapping_add(b),
                BinOp::Sub => a.wrapping_sub(b),
                BinOp::Mul => a.wrapping_mul(b),
                BinOp::Div => {
                    if b == 0 {
                        0
                    } else {
                        a / b
                    }
                }
                BinOp::Rem => {
                    if b == 0 {
                        0
                    } else {
                        a % b
                    }
                }
                _ => a,
            };
            Value::Int(r)
        } else {
            let (a, b) = (self.num(), rhs.num());
            let r = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => {
                    if b == 0.0 {
                        0.0
                    } else {
                        a / b
                    }
                }
                BinOp::Rem => {
                    if b == 0.0 {
                        0.0
                    } else {
                        a % b
                    }
                }
                _ => a,
            };
            Value::Float(r)
        }
    }

    fn compare(&self, op: BinOp, rhs: &Value) -> Value {
        let ord = match (self, rhs) {
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => self
                .num()
                .partial_cmp(&rhs.num())
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        Value::Bool(match op {
            BinOp::Lt => ord == std::cmp::Ordering::Less,
            BinOp::Le => ord != std::cmp::Ordering::Greater,
            BinOp::Gt => ord == std::cmp::Ordering::Greater,
            BinOp::Ge => ord != std::cmp::Ordering::Less,
            BinOp::Eq => self == rhs || self.num() == rhs.num(),
            BinOp::Ne => !(self == rhs || self.num() == rhs.num()),
            _ => false,
        })
    }
}

/// Errors from interpretation.
#[derive(Debug, Clone, PartialEq)]
pub struct InterpError(pub String);

type R<T> = Result<T, InterpError>;

/// The interpreter. Holds the named parameters of the enclosing context: the
/// state parameter (`s`) is just the first binding; handlers receive it
/// explicitly (§7.3), loop variables bind per iteration (§7.6).
#[derive(Debug, Clone)]
pub struct Interp {
    bindings: BTreeMap<String, Value>,
}

impl Interp {
    /// A fresh environment with named bindings.
    pub fn new(bindings: impl IntoIterator<Item = (impl Into<String>, Value)>) -> Interp {
        Interp {
            bindings: bindings
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        }
    }

    /// Read a named binding.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.bindings.get(name)
    }

    /// Bind a name (region scopes bind loop items, §6.3).
    pub fn bind(&mut self, name: &str, value: Value) {
        self.bindings.insert(name.to_string(), value);
    }

    /// Evaluate an expression to a value.
    pub fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Var(name) => {
                if let Some(v) = self.bindings.get(name) {
                    Ok(v.clone())
                } else if name == "true" {
                    Ok(Value::Bool(true))
                } else if name == "false" {
                    Ok(Value::Bool(false))
                } else {
                    Err(InterpError(format!("unbound name `{name}`")))
                }
            }
            Expr::Lit(l) => Ok(lit_value(l)),
            Expr::Field(base, name) => {
                let b = self.eval(base)?;
                match b {
                    Value::Obj(fields) => Ok(fields
                        .borrow()
                        .get(name)
                        .cloned()
                        .unwrap_or(Value::Null)),
                    _ => Err(InterpError(format!("field `{name}` of non-object"))),
                }
            }
            Expr::Index(base, idx) => {
                let b = self.eval(base)?;
                let i = self.eval(idx)?;
                match (b, i) {
                    (Value::List(l), Value::Int(i)) => Ok(l
                        .borrow()
                        .get(usize::try_from(i).unwrap_or(usize::MAX))
                        .cloned()
                        .unwrap_or(Value::Null)),
                    (v, i) => Err(InterpError(format!("index {i:?} of {v:?}"))),
                }
            }
            Expr::Call(name, args) => match name.as_str() {
                // The G6 escape hatch and a few intrinsic helpers used by
                // views; anything else is a foreign call.
                "trusted" => self.eval(&args[0]),
                "len" => {
                    let v = self.eval(&args[0])?;
                    Ok(match v {
                        Value::List(l) => Value::Int(l.borrow().len() as i64),
                        Value::Str(s) => Value::Int(s.chars().count() as i64),
                        _ => Value::Int(0),
                    })
                }
                _ => Err(InterpError(format!("opaque call `{name}`"))),
            },
            Expr::Method(recv, name, args) => {
                let r = self.eval(recv)?;
                match (name.as_str(), &r) {
                    // The iterator protocol surface views use (§3.1:
                    // `s.history.iter()`).
                    ("iter", _) | ("clone", _) => Ok(r),
                    ("len", _) | ("count", _) => match r {
                        Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                        Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                        _ => Ok(Value::Int(0)),
                    },
                    ("value", _) => {
                        // Event payloads for `bind:value` (§7.9): the current
                        // event value is carried by the `e` binding, but a
                        // method form `e.value()` must also work.
                        Ok(r)
                    }
                    ("push", _) => {
                        if let Value::List(l) = r {
                            let v = self.eval(&args[0])?;
                            l.borrow_mut().push(v);
                            Ok(Value::Null)
                        } else {
                            Err(InterpError("push on non-list".to_string()))
                        }
                    }
                    _ => Err(InterpError(format!("opaque method `{name}`"))),
                }
            }
            Expr::Closure { body, .. } => self.eval(body),
            Expr::Unary(op, inner) => {
                let v = self.eval(inner)?;
                Ok(match op {
                    UnOp::Neg => match v {
                        Value::Int(i) => Value::Int(-i),
                        Value::Float(f) => Value::Float(-f),
                        _ => Value::Null,
                    },
                    UnOp::Not => Value::Bool(!v.truthy()),
                    UnOp::Deref => v,
                    UnOp::Ref | UnOp::RefMut => v,
                })
            }
            Expr::Binary(op, lhs, rhs) => {
                if *op == BinOp::And {
                    let l = self.eval(lhs)?;
                    return Ok(if l.truthy() {
                        self.eval(rhs)?
                    } else {
                        Value::Bool(false)
                    });
                }
                if *op == BinOp::Or {
                    let l = self.eval(lhs)?;
                    return Ok(if l.truthy() {
                        Value::Bool(true)
                    } else {
                        self.eval(rhs)?
                    });
                }
                let l = self.eval(lhs)?;
                let r = self.eval(rhs)?;
                Ok(match op {
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                        l.compare(*op, &r)
                    }
                    BinOp::Add if matches!((&l, &r), (Value::Str(_), _)) => {
                        Value::Str(Rc::from(format!("{}{}", l.display(), r.display())))
                    }
                    BinOp::Add if matches!((&l, &r), (_, Value::Str(_))) => {
                        Value::Str(Rc::from(format!("{}{}", l.display(), r.display())))
                    }
                    _ => l.arith(*op, &r),
                })
            }
            Expr::Assign { lhs, op, rhs } => {
                let value = match op {
                    Some(o) => {
                        let cur = self.eval(lhs)?;
                        let rv = self.eval(rhs)?;
                        match o {
                            BinOp::Add if matches!((&cur, &rv), (Value::Str(_), _)) => {
                                Value::Str(Rc::from(format!(
                                    "{}{}",
                                    cur.display(),
                                    rv.display()
                                )))
                            }
                            _ => cur.arith(*o, &rv),
                        }
                    }
                    None => self.eval(rhs)?,
                };
                self.assign(lhs, value.clone())?;
                Ok(value)
            }
            Expr::If { cond, then, els } => {
                let c = self.eval(cond)?;
                if c.truthy() {
                    self.eval(then)
                } else if let Some(e) = els {
                    self.eval(e)
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::Match { scrutinee, arms } => {
                let s = self.eval(scrutinee)?;
                for (pat, guard, body) in arms {
                    if pattern_matches(pat, &s) {
                        if let Some(g) = guard
                            && !self.eval(g)?.truthy() {
                                continue;
                            }
                        return self.eval(body);
                    }
                }
                Ok(Value::Null)
            }
            Expr::For { pat, iter, body, .. } => {
                let it = self.eval(iter)?;
                if let Value::List(l) = it {
                    for item in l.borrow().iter() {
                        let mut scope = self.clone();
                        scope.bindings.insert(pat.clone(), item.clone());
                        scope.eval(body)?;
                    }
                    Ok(Value::Null)
                } else {
                    Err(InterpError("for over non-list".to_string()))
                }
            }
            Expr::Block { stmts, tail } => {
                let mut scope = self.clone();
                for s in stmts {
                    if let Expr::Let { pat, value } = s {
                        let v = scope.eval(value)?;
                        scope.bindings.insert(pat.clone(), v);
                    } else {
                        scope.eval(s)?;
                    }
                }
                match tail {
                    Some(t) => scope.eval(t),
                    None => Ok(Value::Null),
                }
            }
            Expr::Let { .. } => Ok(Value::Null), // handled by Block
            Expr::Flow => Ok(Value::Null),
            Expr::Gsx(_) => Err(InterpError("cannot evaluate markup as a value".to_string())),
            Expr::Opaque(what) => Err(InterpError(format!("opaque: {what}"))),
        }
    }

    /// Assign through a place expression (`s.count`, `s.items[i].done`).
    pub fn assign(&mut self, place: &Expr, value: Value) -> R<()> {
        match place {
            Expr::Var(name) => {
                self.bindings.insert(name.clone(), value);
                Ok(())
            }
            Expr::Field(base, name) => {
                // If the base is a plain state param, mutate through it.
                let b = self.eval(base)?;
                if let Value::Obj(fields) = b {
                    fields.borrow_mut().insert(name.clone(), value);
                    Ok(())
                } else {
                    Err(InterpError(format!("assign into non-object field `{name}`")))
                }
            }
            Expr::Index(base, idx) => {
                let b = self.eval(base)?;
                let i = self.eval(idx)?;
                match (b, i) {
                    (Value::List(l), Value::Int(i)) => {
                        let iu = usize::try_from(i).map_err(|_| {
                            InterpError("negative index".to_string())
                        })?;
                        let mut list = l.borrow_mut();
                        if iu < list.len() {
                            list[iu] = value;
                            Ok(())
                        } else {
                            Err(InterpError(format!("index {iu} out of bounds")))
                        }
                    }
                    _ => Err(InterpError("index assign into non-list".to_string())),
                }
            }
            _ => Err(InterpError("assign to non-place".to_string())),
        }
    }
}

fn lit_value(l: &Lit) -> Value {
    match l {
        Lit::Int(i) => Value::Int(i64::try_from(*i).unwrap_or(i64::MAX)),
        Lit::Uint(u) => Value::Int(i64::try_from(*u).unwrap_or(i64::MAX)),
        Lit::Float(f) => Value::Float(*f),
        Lit::Str(s) => Value::Str(Rc::from(s.as_str())),
        Lit::Char(c) => Value::Str(Rc::from(c.to_string())),
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Unit => Value::Null,
    }
}

/// Very small pattern matcher for match arms in views (`Some(x)`, variants).
fn pattern_matches(pat: &str, value: &Value) -> bool {
    let pat = pat.trim();
    match value {
        Value::Obj(fields) => {
            if pat == "_" {
                return true;
            }
            // Enum-ish: variant name is a field `__tag` in the dynamic model.
            let tag = fields
                .borrow()
                .get("__tag")
                .map(|t| t.display())
                .unwrap_or_default();
            if !tag.is_empty() {
                return pat.split_once('(').map(|(v, _)| v) == Some(tag.as_str())
                    || pat == tag
                    || pat == "_";
            }
            pat == "_" || pat == "Ok(..)" || pat == "Some(..)"
        }
        _ => pat == "_" || pat == value.display(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{split_text_holes, TextPiece};

    /// §3.1 handler semantics: `|s, _| s.count -= s.step` mutates state.
    #[test]
    fn handler_state_transformer() {
        let state = Value::obj([
            ("count", Value::Int(10)),
            ("step", Value::Int(3)),
        ]);
        let body = Expr::Assign {
            lhs: Box::new(Expr::field("s", "count")),
            op: Some(BinOp::Sub),
            rhs: Box::new(Expr::field("s", "step")),
        };
        let mut it = Interp::new([("s", state.clone())]);
        it.eval(&body).unwrap();
        match &state {
            Value::Obj(f) => {
                assert_eq!(f.borrow().get("count"), Some(&Value::Int(7)));
            }
            _ => panic!(),
        }
    }

    /// §7.6: `s.items[i].done = true` writes exactly one element.
    #[test]
    fn indexed_row_write() {
        let row = |done: bool| {
            Value::obj([
                ("done", Value::Bool(done)),
                ("label", Value::Str(Rc::from("x"))),
            ])
        };
        let state = Value::obj([("items", Value::list([row(false), row(false), row(false)]))]);
        let body = Expr::Assign {
            lhs: Box::new(Expr::Field(
                Box::new(Expr::Index(
                    Box::new(Expr::field("s", "items")),
                    Box::new(Expr::Var("i".to_string())),
                )),
                "done".to_string(),
            )),
            op: None,
            rhs: Box::new(Expr::Lit(Lit::Bool(true))),
        };
        let mut it = Interp::new([("s", state.clone()), ("i", Value::Int(1))]);
        it.eval(&body).unwrap();
        let items = match &state {
            Value::Obj(f) => f.borrow().get("items").cloned().unwrap(),
            _ => panic!(),
        };
        let list = match items {
            Value::List(l) => l,
            _ => panic!(),
        };
        let done = |v: &Value| match v {
            Value::Obj(f) => f.borrow().get("done").cloned().unwrap(),
            _ => panic!(),
        };
        assert_eq!(done(&list.borrow()[0]), Value::Bool(false));
        assert_eq!(done(&list.borrow()[1]), Value::Bool(true));
        assert_eq!(done(&list.borrow()[2]), Value::Bool(false));
    }

    /// §7.9: the synthesized bind handler `|s, e| s.name = e.value()`.
    #[test]
    fn bind_synthesized_handler() {
        let state = Value::obj([("name", Value::Str(Rc::from("old")))]);
        let event = Value::Str(Rc::from("typed text"));
        let body = Expr::Assign {
            lhs: Box::new(Expr::field("s", "name")),
            op: None,
            rhs: Box::new(Expr::Method(
                Box::new(Expr::Var("e".to_string())),
                "value".to_string(),
                vec![],
            )),
        };
        let mut it = Interp::new([("s", state.clone()), ("e", event)]);
        it.eval(&body).unwrap();
        match &state {
            Value::Obj(f) => {
                assert_eq!(
                    f.borrow().get("name").cloned().unwrap().display(),
                    "typed text"
                );
            }
            _ => panic!(),
        }
    }

    /// §6.1 Text slots: interpolation formatting of holes.
    #[test]
    fn text_interpolation_formatting() {
        let state = Value::obj([("count", Value::Int(-4)), ("name", Value::Str(Rc::from("Glyim")))]);
        let mut it = Interp::new([("s", state)]);
        let pieces = split_text_holes("{s.count} items for {s.name}");
        let out = pieces
            .iter()
            .map(|p| match p {
                TextPiece::Static(s) => s.clone(),
                TextPiece::Hole(e) => it.eval(e).unwrap().display(),
            })
            .collect::<String>();
        assert_eq!(out, "-4 items for Glyim");
    }

    /// Regions: if/else evaluation against state.
    #[test]
    fn if_region_evaluation() {
        let state = Value::obj([("count", Value::Int(12))]);
        let mut it = Interp::new([("s", state)]);
        let cond = Expr::Binary(
            BinOp::Gt,
            Box::new(Expr::field("s", "count")),
            Box::new(Expr::Lit(Lit::Int(10))),
        );
        let e = Expr::If {
            cond: Box::new(cond),
            then: Box::new(Expr::Lit(Lit::Str("big".to_string()))),
            els: Some(Box::new(Expr::Lit(Lit::Str("small".to_string())))),
        };
        assert_eq!(it.eval(&e).unwrap().display(), "big");
    }

    /// Class toggles: `class:warn={s.count < 0}` evaluates to a bool.
    #[test]
    fn class_toggle_bool() {
        let state = Value::obj([("count", Value::Int(-1))]);
        let mut it = Interp::new([("s", state)]);
        let e = Expr::Binary(
            BinOp::Lt,
            Box::new(Expr::field("s", "count")),
            Box::new(Expr::Lit(Lit::Int(0))),
        );
        assert_eq!(it.eval(&e).unwrap(), Value::Bool(true));
    }
}
