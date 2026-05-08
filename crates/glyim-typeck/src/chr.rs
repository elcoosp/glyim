use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::{ErrorGuaranteed, UnificationTable};
use glyim_interner::Symbol;
use glyim_diag::Span;
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
                                for p in premises {
                                    if !self.proven_goals.contains(p) {
                                        self.pending_goals.push(p.clone());
                                    }
                                }
                                self.pending_goals.push(goal.clone());
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
                                for p in premises {
                                    if !self.proven_goals.contains(p) {
                                        self.pending_goals.push(p.clone());
                                    }
                                }
                                self.pending_goals.push(goal.clone());
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
