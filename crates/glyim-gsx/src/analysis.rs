//! Read-sets, write-sets and prefix interference (design doc §7.4, §7.6).
//!
//! §7.2: *State is owned data. Views read it by shared borrow. Handlers
//! mutate it by exclusive borrow. Since the borrow checker already proves
//! that no one else can observe a mutation without going through an exclusive
//! path, the set of places a handler can write is statically computable, and
//! the set of places a slot reads is statically computable. Their
//! intersection IS the change-detection system.*
//!
//! - **Read-set of a slot**: the state places (field paths rooted at the
//!   component's state parameter, e.g. `s.count`, `s.user.name`) the slot
//!   expression reads. Opaque calls read conservatively the whole subtree at
//!   their argument place (§7.4).
//! - **Write-set of a handler**: places written, from assignments and
//!   `&mut` reborrows (§7.4).
//! - **Interference is prefix-based**: a write to `s.user` interferes with a
//!   read of `s.user.name` and vice versa; `s.user.age` and `s.user.name` do
//!   not interfere. Computed at compile time into constants (§7.4).
//! - **§7.6**: `s.items[i].done = true` has write-set `s.items[*].done` with
//!   *runtime* index `i`; structural writes have write-set `s.items` (the
//!   container) and trigger the region's keyed reconcile.

use crate::expr::Expr;
use std::collections::BTreeSet;

/// One component of a place path: a field name or an index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceStep {
    /// `.field`
    Field(String),
    /// `[index]` with a *runtime* index — §7.6's `s.items[i]`. The index
    /// itself is not tracked; the wildcard aspect is the identity of this
    /// step (`[*]`).
    Index,
}

impl PlaceStep {
    fn matches(&self, other: &PlaceStep) -> bool {
        match (self, other) {
            (PlaceStep::Field(a), PlaceStep::Field(b)) => a == b,
            (PlaceStep::Index, PlaceStep::Index) => true,
            _ => false,
        }
    }
}

/// A state place: a field path rooted at the state parameter (§7.4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Place {
    /// Path steps below the state param.
    pub steps: Vec<PlaceStep>,
    /// Whether this place carries a runtime index (`s.items[i]`) — the
    /// patch for it is *index-parameterized* (§7.6).
    pub indexed: bool,
    /// Whether the read reaches below this place (opaque call on a subtree).
    pub subtree: bool,
}

impl Place {
    /// Root place (the whole state).
    pub fn root() -> Place {
        Place {
            steps: vec![],
            indexed: false,
            subtree: true,
        }
    }

    /// Extend with a field step.
    pub fn field(&self, name: &str) -> Place {
        let mut steps = self.steps.clone();
        steps.push(PlaceStep::Field(name.to_string()));
        Place {
            steps,
            indexed: self.indexed,
            subtree: false,
        }
    }

    /// Extend with an index step.
    pub fn index(&self) -> Place {
        let mut steps = self.steps.clone();
        steps.push(PlaceStep::Index);
        Place {
            steps,
            indexed: true,
            subtree: false,
        }
    }

    /// Is this place the root (whole state)?
    pub fn is_root(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Prefix-based interference (§7.4): `a` interferes with `b` iff one is a
/// prefix of the other (in step terms; `[*]` matches any field/index at its
/// position for interference purposes).
pub fn interferes(a: &Place, b: &Place) -> bool {
    let n = a.steps.len().min(b.steps.len());
    for (x, y) in a.steps.iter().zip(b.steps.iter()).take(n) {
        if !x.matches(y) {
            return false;
        }
    }
    // Prefix-compatible; extra steps on either side still interfere.
    true
}

/// Whether any place of `reads` interferes with any place of `writes`.
pub fn any_interference(reads: &BTreeSet<Place>, writes: &BTreeSet<Place>) -> bool {
    reads.iter().any(|r| writes.iter().any(|w| interferes(r, w)))
}

/// Whether a read-set touches an untrackable value (§7.7): reads through
/// `borrow()`/`clone()` of RefCell-ish receivers, or an opaque call. The
/// compiler answers with `G0201` + fix-it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadFlags {
    /// The read is untrackable — Signal<T> required (§7.7).
    pub untrackable: bool,
    /// The untrackable expression's source (for the diagnostic).
    pub untrackable_what: Option<String>,
    /// The expression contains an opaque call (conservative whole-subtree).
    pub opaque: bool,
}

/// Compute the read-set of an expression rooted at `state_param` (§7.4).
pub fn read_set(e: &Expr, state_param: &str) -> BTreeSet<Place> {
    let mut set = BTreeSet::new();
    collect_reads(e, state_param, Place::root(), &mut set, &mut ReadFlags::default());
    set
}

/// Read-set plus flags (G0201 detection).
pub fn read_set_with_flags(e: &Expr, state_param: &str) -> (BTreeSet<Place>, ReadFlags) {
    let mut set = BTreeSet::new();
    let mut flags = ReadFlags::default();
    collect_reads(e, state_param, Place::root(), &mut set, &mut flags);
    (set, flags)
}

fn collect_reads(
    e: &Expr,
    state_param: &str,
    base: Place,
    set: &mut BTreeSet<Place>,
    flags: &mut ReadFlags,
) {
    match e {
        Expr::Var(name) => {
            if name == state_param {
                set.insert(Place::root());
            }
        }
        Expr::Field(b, name) => {
            let place = place_of(b, state_param, base, set, flags);
            set.insert(place.field(name));
        }
        Expr::Index(b, idx) => {
            collect_reads(idx, state_param, base.clone(), set, flags);
            let place = place_of(b, state_param, base, set, flags);
            set.insert(place.index());
        }
        Expr::Method(recv, name, args) => {
            // Untrackable reads (§7.7): reads through RefCell-ish surfaces.
            if matches!(name.as_str(), "borrow" | "borrow_mut" | "clone" | "take" | "replace")
                && is_cellish(recv)
            {
                flags.untrackable = true;
                if flags.untrackable_what.is_none() {
                    flags.untrackable_what = Some(expr_display(recv, state_param));
                }
            }
            if matches!(name.as_str(), "iter" | "len" | "get" | "contains") {
                // Safe reads over the receiver place.
                let _ = place_of(recv, state_param, base.clone(), set, flags);
            } else {
                // Unknown method: conservative whole subtree at the place.
                let place = place_of(recv, state_param, base.clone(), set, flags);
                let mut p = place;
                p.subtree = true;
                set.insert(p);
            }
            for a in args {
                collect_reads(a, state_param, base.clone(), set, flags);
            }
        }
        Expr::Call(name, args) => {
            if name == "trusted" {
                // G6 escape hatch: reads flow through.
                for a in args {
                    collect_reads(a, state_param, base.clone(), set, flags);
                }
            } else {
                // Opaque call: everything under its argument places (§7.4).
                flags.opaque = true;
                for a in args {
                    collect_reads(a, state_param, base.clone(), set, flags);
                    // Arg place itself, subtree-wide (supersedes any shallow
                    // read of the same place).
                    if let Some(p) = arg_place(a, state_param) {
                        let mut p2 = p.clone();
                        p2.subtree = true;
                        let mut shallow = p;
                        shallow.subtree = false;
                        set.remove(&shallow);
                        set.insert(p2);
                    }
                }
            }
        }
        Expr::Closure { body, .. } => collect_reads(body, state_param, base, set, flags),
        Expr::Unary(_, inner) => collect_reads(inner, state_param, base, set, flags),
        Expr::Binary(_, l, r) => {
            collect_reads(l, state_param, base.clone(), set, flags);
            collect_reads(r, state_param, base, set, flags);
        }
        Expr::Assign { lhs, rhs, op } => {
            // A compound assign reads the lhs too.
            if op.is_some() {
                collect_reads(lhs, state_param, base.clone(), set, flags);
            }
            collect_reads(rhs, state_param, base, set, flags);
        }
        Expr::If { cond, then, els } => {
            collect_reads(cond, state_param, base.clone(), set, flags);
            collect_reads(then, state_param, base.clone(), set, flags);
            if let Some(e) = els {
                collect_reads(e, state_param, base, set, flags);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_reads(scrutinee, state_param, base.clone(), set, flags);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    collect_reads(g, state_param, base.clone(), set, flags);
                }
                collect_reads(b, state_param, base.clone(), set, flags);
            }
        }
        Expr::For { pat, iter, key, body } => {
            collect_reads(iter, state_param, base.clone(), set, flags);
            if let Some(k) = key {
                collect_reads(k, state_param, base.clone(), set, flags);
            }
            // The loop variable shadows `pat`; body reads through it are
            // item reads — conservatively the whole iterated place.
            let _ = pat;
            collect_reads(body, state_param, base, set, flags);
        }
        Expr::Block { stmts, tail } => {
            for s in stmts {
                collect_reads(s, state_param, base.clone(), set, flags);
            }
            if let Some(t) = tail {
                collect_reads(t, state_param, base, set, flags);
            }
        }
        Expr::Let { value, .. } => collect_reads(value, state_param, base, set, flags),
        // Region arm bodies are analyzed per sub-VIR (§6.3).
        Expr::Gsx(_) => {}
        Expr::Lit(_) | Expr::Flow | Expr::Opaque(_) => {}
    }
}

/// Resolve the place of an expression node, registering any reads seen.
fn place_of(
    e: &Expr,
    state_param: &str,
    _base: Place,
    set: &mut BTreeSet<Place>,
    flags: &mut ReadFlags,
) -> Place {
    match e {
        Expr::Var(name) if name == state_param => Place::root(),
        Expr::Field(b, name) => place_of(b, state_param, _base, set, flags).field(name),
        Expr::Index(b, idx) => {
            collect_reads(idx, state_param, _base.clone(), set, flags);
            place_of(b, state_param, _base, set, flags).index()
        }
        Expr::Var(_) => Place {
            steps: vec![PlaceStep::Field(e_var_name(e))],
            indexed: false,
            subtree: false,
        },
        other => {
            // Not a simple place: read it and return the root (conservative).
            collect_reads(other, state_param, _base, set, flags);
            Place {
                steps: vec![PlaceStep::Field(String::new())],
                indexed: false,
                subtree: true,
            }
        }
    }
}

fn e_var_name(e: &Expr) -> String {
    match e {
        Expr::Var(v) => v.clone(),
        _ => String::new(),
    }
}

fn arg_place(a: &Expr, state_param: &str) -> Option<Place> {
    fn build(e: &Expr, state_param: &str) -> Option<Place> {
        match e {
            Expr::Var(name) if name == state_param => Some(Place::root()),
            Expr::Field(b, name) => build(b, state_param).map(|p| p.field(name)),
            Expr::Index(b, _) => build(b, state_param).map(|p| p.index()),
            _ => None,
        }
    }
    build(a, state_param)
}

fn is_cellish(e: &Expr) -> bool {
    // Structural approximation of "reads through a shared mutable handle":
    // a method on a value named like a cell/ref-handle field.
    match e {
        Expr::Field(_, name) => {
            let n = name.to_ascii_lowercase();
            n.contains("cell") || n.contains("refcell") || n.contains("rc") || n.contains("arc")
        }
        Expr::Var(name) => {
            let n = name.to_ascii_lowercase();
            n.contains("cell") || n.contains("refcell") || n.contains("shared")
        }
        _ => false,
    }
}

/// Compute the write-set of a handler body (§7.4): places written through
/// assignments, compound assigns and calls that take `&mut` places.
pub fn write_set(body: &Expr, state_param: &str) -> BTreeSet<Place> {
    let mut set = BTreeSet::new();
    collect_writes(body, state_param, &mut set);
    set
}

fn collect_writes(e: &Expr, state_param: &str, set: &mut BTreeSet<Place>) {
    match e {
        Expr::Assign { lhs, rhs, .. } => {
            // §7.6: `s.items[i].done = true` → place with Index step.
            if let Some(p) = arg_place(lhs, state_param) {
                set.insert(p);
            } else if let Expr::Var(_) = lhs.as_ref() {
                // Rebinding a local — not a state write.
            } else {
                // Unknown lhs: conservatively everything.
                set.insert(Place::root());
            }
            collect_writes(rhs, state_param, set);
        }
        Expr::Call(name, args) => {
            // Calls that take `&mut` reborrows write their argument places
            // conservatively (§7.4 "resolved via write summaries … extern
            // / dyn → conservative 'writes everything below this place'").
            if is_mutating_call(name) {
                for a in args {
                    if let Some(p) = arg_place(a, state_param) {
                        let mut p2 = p;
                        p2.subtree = true;
                        set.insert(p2);
                    }
                }
            }
            for a in args {
                collect_writes(a, state_param, set);
            }
        }
        Expr::Method(recv, name, args) => {
            if is_mutating_method(name) {
                if let Some(p) = arg_place(recv, state_param) {
                    let mut p2 = p;
                    p2.subtree = true;
                    set.insert(p2);
                }
            }
            for a in args {
                collect_writes(a, state_param, set);
            }
        }
        Expr::Closure { body, .. } => collect_writes(body, state_param, set),
        Expr::Unary(_, inner) => collect_writes(inner, state_param, set),
        Expr::Binary(_, l, r) => {
            collect_writes(l, state_param, set);
            collect_writes(r, state_param, set);
        }
        Expr::If { cond, then, els } => {
            collect_writes(cond, state_param, set);
            collect_writes(then, state_param, set);
            if let Some(e) = els {
                collect_writes(e, state_param, set);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_writes(scrutinee, state_param, set);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    collect_writes(g, state_param, set);
                }
                collect_writes(b, state_param, set);
            }
        }
        Expr::For { iter, key, body, .. } => {
            collect_writes(iter, state_param, set);
            if let Some(k) = key {
                collect_writes(k, state_param, set);
            }
            collect_writes(body, state_param, set);
        }
        Expr::Block { stmts, tail } => {
            for s in stmts {
                collect_writes(s, state_param, set);
            }
            if let Some(t) = tail {
                collect_writes(t, state_param, set);
            }
        }
        Expr::Let { value, .. } => collect_writes(value, state_param, set),
        Expr::Gsx(_) => {}
        _ => {}
    }
}

fn is_mutating_call(name: &str) -> bool {
    // Known stdlib structural mutators (§7.6: push/remove/sort write the
    // container).
    matches!(
        name,
        "push" | "insert" | "remove" | "swap" | "sort" | "clear" | "truncate" | "pop"
    )
}

fn is_mutating_method(name: &str) -> bool {
    matches!(
        name,
        "push" | "insert" | "remove" | "swap" | "sort" | "clear" | "truncate" | "pop"
            | "set" | "borrow_mut" | "replace" | "take"
    )
}

/// Format an expression for diagnostics (`s.items[i]`).
pub fn expr_display(e: &Expr, state_param: &str) -> String {
    match e {
        Expr::Var(v) => v.clone(),
        Expr::Field(b, n) => format!("{}.{}", expr_display(b, state_param), n),
        Expr::Index(b, i) => format!("{}[{}]", expr_display(b, state_param), expr_display(i, state_param)),
        Expr::Method(r, n, _) => format!("{}.{}(..)", expr_display(r, state_param), n),
        Expr::Call(n, _) => format!("{n}(..)"),
        Expr::Lit(l) => match l {
            crate::expr::Lit::Str(s) => format!("\"{s}\""),
            other => format!("{other:?}"),
        },
        _ => format!("{e:?}"),
    }
}

/// The read-set of a whole slot kind (§7.4), including region scrutinees.
pub fn slot_read_set(kind: &crate::vir::SlotKind, state_param: &str) -> (BTreeSet<Place>, ReadFlags) {
    let mut set = BTreeSet::new();
    let mut flags = ReadFlags::default();
    match kind {
        crate::vir::SlotKind::Text => {}
        crate::vir::SlotKind::Prop { expr, .. } => {
            let (s, f) = read_set_with_flags(expr, state_param);
            set.extend(s);
            flags |= f;
        }
        crate::vir::SlotKind::ClassToggle { expr, .. } => {
            let (s, f) = read_set_with_flags(expr, state_param);
            set.extend(s);
            flags |= f;
        }
        crate::vir::SlotKind::Style { expr, .. } => {
            let (s, f) = read_set_with_flags(expr, state_param);
            set.extend(s);
            flags |= f;
        }
        crate::vir::SlotKind::Event { .. } => {}
        crate::vir::SlotKind::Region { region } => {
            let _ = region; // region bodies analyzed separately (§6.3)
        }
    }
    (set, flags)
}

impl std::ops::BitOrAssign for ReadFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        if rhs.untrackable {
            self.untrackable = true;
            if self.untrackable_what.is_none() {
                self.untrackable_what = rhs.untrackable_what;
            }
        }
        self.opaque |= rhs.opaque;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Lit;

    /// §7.4: read-set of `s.count` is exactly `{s.count}`.
    #[test]
    fn read_set_of_simple_field() {
        let e = Expr::field("s", "count");
        let rs = read_set(&e, "s");
        assert_eq!(rs.len(), 1);
        let p = rs.iter().next().unwrap();
        assert_eq!(p.steps, vec![PlaceStep::Field("count".to_string())]);
    }

    /// §7.4: `s.user.name` and `s.user.age` do NOT interfere; `s.user` does
    /// with both.
    #[test]
    fn prefix_interference() {
        let user = Place::root().field("user");
        let name = user.field("name");
        let age = user.field("age");
        assert!(interferes(&user, &name));
        assert!(interferes(&name, &user));
        assert!(!interferes(&name, &age));
        assert!(!interferes(&age, &name));
    }

    /// §7.4: opaque calls read conservatively the whole subtree at their
    /// argument place.
    #[test]
    fn opaque_call_reads_subtree() {
        let e = Expr::Call(
            "format_name".to_string(),
            vec![Expr::field("s", "user")],
        );
        let rs = read_set(&e, "s");
        let user = rs.iter().find(|p| p.steps.len() == 1).unwrap();
        assert!(user.subtree, "opaque call reads the whole subtree");
        // and it interferes with a deep read
        let deep = Place::root().field("user").field("name");
        assert!(interferes(user, &deep));
    }

    /// §7.6: `s.items[i].done` write-set is an indexed place.
    #[test]
    fn indexed_write_place() {
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
        let ws = write_set(&body, "s");
        assert_eq!(ws.len(), 1);
        let p = ws.iter().next().unwrap();
        assert!(p.indexed);
        assert_eq!(
            p.steps,
            vec![
                PlaceStep::Field("items".to_string()),
                PlaceStep::Index,
                PlaceStep::Field("done".to_string()),
            ]
        );
    }

    /// §7.6: structural writes (push/remove/sort) write the container
    /// subtree → keyed reconcile.
    #[test]
    fn structural_write_hits_container() {
        let body = Expr::Method(
            Box::new(Expr::field("s", "items")),
            "push".to_string(),
            vec![Expr::Var("new_item".to_string())],
        );
        let ws = write_set(&body, "s");
        let p = ws.iter().next().unwrap();
        assert!(p.subtree);
        assert_eq!(p.steps, vec![PlaceStep::Field("items".to_string())]);
        // It interferes with a keyed row read.
        let row_done = Place::root().field("items").index().field("done");
        assert!(interferes(p, &row_done));
    }

    /// §7.5: the − and + buttons' handlers share the write-set `{s.count}`.
    #[test]
    fn minus_plus_share_write_set() {
        let minus = Expr::Assign {
            lhs: Box::new(Expr::field("s", "count")),
            op: Some(crate::expr::BinOp::Sub),
            rhs: Box::new(Expr::field("s", "step")),
        };
        let plus = Expr::Assign {
            lhs: Box::new(Expr::field("s", "count")),
            op: Some(crate::expr::BinOp::Add),
            rhs: Box::new(Expr::field("s", "step")),
        };
        let wm = write_set(&minus, "s");
        let wp = write_set(&plus, "s");
        assert_eq!(wm, wp);
        // and it drives exactly the count output + warn toggle (§7.5 example)
        let read_count = read_set(&Expr::field("s", "count"), "s");
        let read_warn = read_set(
            &Expr::Binary(
                crate::expr::BinOp::Lt,
                Box::new(Expr::field("s", "count")),
                Box::new(Expr::Lit(Lit::Int(0))),
            ),
            "s",
        );
        assert!(any_interference(&read_count, &wp));
        assert!(any_interference(&read_warn, &wp));
        let read_name = read_set(&Expr::field("s", "name"), "s");
        assert!(!any_interference(&read_name, &wp));
    }

    /// §7.7: reads through RefCell-ish surfaces are untrackable → G0201.
    #[test]
    fn untrackable_read_detected() {
        let e = Expr::Method(
            Box::new(Expr::field("s", "shared_cell")),
            "borrow".to_string(),
            vec![],
        );
        let (_, flags) = read_set_with_flags(&e, "s");
        assert!(flags.untrackable);
        assert!(flags.untrackable_what.as_deref().unwrap().contains("shared_cell"));
    }

    /// Deep vs shallow writes: `s.user` write interferes with `s.user.name`
    /// read; `s.other` write does not interfere with `s.user.name` read.
    #[test]
    fn interference_is_prefix_based_end_to_end() {
        let ws = write_set(
            &Expr::Assign {
                lhs: Box::new(Expr::field("s", "user")),
                op: None,
                rhs: Box::new(Expr::Var("u".to_string())),
            },
            "s",
        );
        let read_name = read_set(&Expr::Field(
            Box::new(Expr::field("s", "user")),
            "name".to_string(),
        ), "s");
        let read_other = read_set(&Expr::Field(
            Box::new(Expr::field("s", "other")),
            "x".to_string(),
        ), "s");
        assert!(any_interference(&read_name, &ws));
        assert!(!any_interference(&read_other, &ws));
    }
}
