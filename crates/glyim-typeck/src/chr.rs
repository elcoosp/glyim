use crate::ty::{Ty, TyArena};
use crate::unify::ErrorGuaranteed;
use glyim_interner::Symbol;

use std::collections::HashSet;

/// A logical goal we need to prove.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Goal {
    /// TraitImpl(Symbol, [Ty]) e.g., `Display(Vec<i64>)`
    TraitImpl(Symbol, Vec<Ty>),
    /// StateTransition(Symbol, CurrentStateTy, TargetStateTy)
    StateTransition(Symbol, Ty, Ty),
    /// Can we prove that type T is reflectable?
    Reflectable(Ty),
    /// Does type T have a field with the given name?
    HasField(Ty, Symbol),
}

/// A rewrite rule for the solver.
#[derive(Clone, Debug)]
pub enum ChrRule {
    /// If we need `Goal`, AND we have `Premises`, THEN `Goal` is proven.
    Simplify {
        goal: Goal,
        premises: Vec<Goal>,
    },
    /// If we need `Goal`, AND we have `Premises`, THEN emit `NewGoals`.
    Propagate {
        goal: Goal,
        premises: Vec<Goal>,
        new_goals: Vec<Goal>,
    },
}

impl ChrRule {
    /// Check if this rule matches the given goal.
    pub fn matches(&self, goal: &Goal) -> bool {
        match self {
            ChrRule::Simplify { goal: rule_goal, .. } => rule_goal == goal,
            ChrRule::Propagate { goal: rule_goal, .. } => rule_goal == goal,
        }
    }
}

pub struct ChrStore {
    rules: Vec<ChrRule>,
    pending_goals: Vec<Goal>,
    proven_goals: HashSet<Goal>,
}

impl ChrStore {
    pub fn new(rules: Vec<ChrRule>) -> Self {
        Self {
            rules,
            pending_goals: Vec::new(),
            proven_goals: HashSet::new(),
        }
    }

    pub fn add_rules(&mut self, rules: Vec<ChrRule>) {
        self.rules.extend(rules);
    }

    pub fn pending_goals(&self) -> &[Goal] {
        &self.pending_goals
    }

    pub fn proven_goals(&self) -> &HashSet<Goal> {
        &self.proven_goals
    }

    /// Add a goal to be proven.
    pub fn push_goal(&mut self, goal: Goal) {
        self.pending_goals.push(goal);
    }

    /// Run the solver to fixed point.
    pub fn solve(&mut self, _arena: &TyArena) -> Result<(), ErrorGuaranteed> {
        while let Some(goal) = self.pending_goals.pop() {
            if self.proven_goals.contains(&goal) {
                continue;
            }

            let mut rule_matched = false;
            for rule in &self.rules {
                if rule.matches(&goal) {
                    rule_matched = true;
                    match rule {
                        ChrRule::Simplify { premises, .. } => {
                            if premises.iter().all(|p| self.proven_goals.contains(p)) {
                                self.proven_goals.insert(goal.clone());
                            } else {
                                self.pending_goals.push(goal.clone());
                                for p in premises {
                                    if !self.proven_goals.contains(p) {
                                        self.pending_goals.push(p.clone());
                                    }
                                }
                            }
                        }
                        ChrRule::Propagate { premises, new_goals, .. } => {
                            if premises.iter().all(|p| self.proven_goals.contains(p)) {
                                self.proven_goals.insert(goal.clone());
                                for ng in new_goals {
                                    if !self.proven_goals.contains(ng) {
                                        self.pending_goals.push(ng.clone());
                                    }
                                }
                            } else {
                                self.pending_goals.push(goal.clone());
                                for p in premises {
                                    if !self.proven_goals.contains(p) {
                                        self.pending_goals.push(p.clone());
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            }

            if !rule_matched {
                return Err(ErrorGuaranteed(()));
            }
        }
        Ok(())
    }
}

/// A mapping from type variables to concrete types for CHR rule instantiation.
#[derive(Clone, Debug)]
pub struct Substitution {
    pub mappings: Vec<(Ty, Ty)>,
}

impl Substitution {
    pub fn new() -> Self {
        Self { mappings: vec![] }
    }

    pub fn add(&mut self, from: Ty, to: Ty) {
        self.mappings.push((from, to));
    }

    pub fn lookup(&self, ty: Ty) -> Option<Ty> {
        self.mappings.iter().find_map(|&(from, to)| {
            if from == ty { Some(to) } else { None }
        })
    }
}

impl Default for Substitution {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a substitution to a type, replacing mapped variables.
pub fn apply_substitution(arena: &mut crate::ty::TyArena, sub: &Substitution, ty: Ty) -> Ty {
    if let Some(replacement) = sub.lookup(ty) {
        return replacement;
    }

    match arena.get(ty).clone() {
        crate::ty::TyKind::App(sym, args) => {
            let new_args: Vec<Ty> = args.iter().map(|&a| apply_substitution(arena, sub, a)).collect();
            arena.alloc(crate::ty::TyKind::App(sym, new_args))
        }
        crate::ty::TyKind::Fn(params, ret) => {
            let new_params: Vec<Ty> = params.iter().map(|&p| apply_substitution(arena, sub, p)).collect();
            let new_ret = apply_substitution(arena, sub, ret);
            arena.alloc(crate::ty::TyKind::Fn(new_params, new_ret))
        }
        crate::ty::TyKind::RawPtr(inner) => {
            let new_inner = apply_substitution(arena, sub, inner);
            arena.alloc(crate::ty::TyKind::RawPtr(new_inner))
        }
        _ => ty,
    }
}


