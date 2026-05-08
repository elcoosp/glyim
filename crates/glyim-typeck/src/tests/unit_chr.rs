use crate::chr::{Goal, ChrRule, ChrStore};
use crate::ty::{Ty, TyKind, TyArena};
use glyim_interner::Interner;


#[test]
fn goal_equality() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let int_sym = interner.intern("Int");
    let mut arena = TyArena::new();
    let int_ty = arena.alloc(TyKind::Named(int_sym));
    let g1 = Goal::TraitImpl(display_sym, vec![int_ty]);
    let g2 = Goal::TraitImpl(display_sym, vec![int_ty]);
    assert_eq!(g1, g2);
}

#[test]
fn goal_inequality_different_trait() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let debug_sym = interner.intern("Debug");
    let g1 = Goal::TraitImpl(display_sym, vec![Ty(0)]);
    let g2 = Goal::TraitImpl(debug_sym, vec![Ty(0)]);
    assert_ne!(g1, g2);
}

#[test]
fn goal_hashable() {
    use std::collections::HashSet;
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let g = Goal::TraitImpl(display_sym, vec![Ty(0)]);
    let mut set = HashSet::new();
    set.insert(g.clone());
    assert!(set.contains(&g));
}

#[test]
fn chr_rule_simplify_creation() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let goal = Goal::TraitImpl(display_sym, vec![Ty(0)]);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![],
    };
    if let ChrRule::Simplify { goal: g, premises } = rule {
        assert_eq!(g, goal);
        assert!(premises.is_empty());
    }
}

#[test]
fn chr_rule_propagate_creation() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let debug_sym = interner.intern("Debug");
    let goal = Goal::TraitImpl(display_sym, vec![Ty(0)]);
    let premise = Goal::TraitImpl(debug_sym, vec![Ty(0)]);
    let new_goal = Goal::TraitImpl(display_sym, vec![Ty(1)]);
    let rule = ChrRule::Propagate {
        goal: goal.clone(),
        premises: vec![premise],
        new_goals: vec![new_goal],
    };
    if let ChrRule::Propagate { goal: g, premises, new_goals } = rule {
        assert_eq!(g, goal);
        assert_eq!(premises.len(), 1);
        assert_eq!(new_goals.len(), 1);
    }
}

// ── Solver tests ──────────────────────────────────────────

#[test]
fn chr_solve_unconditional_simplify() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let int_sym = interner.intern("Int");
    let mut arena = TyArena::new();
    let int_ty = arena.alloc(TyKind::Named(int_sym));

    let goal = Goal::TraitImpl(display_sym, vec![int_ty]);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![],
    };

    let mut store = ChrStore::new(vec![rule]);
    store.push_goal(goal.clone());
    let result = store.solve(&arena);
    assert!(result.is_ok());
    assert!(store.proven_goals().contains(&goal));
}

#[test]
fn chr_solve_fails_no_rule() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let int_sym = interner.intern("Int");
    let mut arena = TyArena::new();
    let int_ty = arena.alloc(TyKind::Named(int_sym));

    let goal = Goal::TraitImpl(display_sym, vec![int_ty]);

    let mut store = ChrStore::new(vec![]);
    store.push_goal(goal);
    let result = store.solve(&arena);
    assert!(result.is_err());
}

#[test]
fn chr_solve_simplify_with_proven_premise() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let debug_sym = interner.intern("Debug");
    let int_sym = interner.intern("Int");
    let mut arena = TyArena::new();
    let int_ty = arena.alloc(TyKind::Named(int_sym));

    let goal = Goal::TraitImpl(display_sym, vec![int_ty]);
    let premise = Goal::TraitImpl(debug_sym, vec![int_ty]);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![premise.clone()],
    };

    let mut store = ChrStore::new(vec![
        rule,
        ChrRule::Simplify {
            goal: premise.clone(),
            premises: vec![],
        },
    ]);

    store.push_goal(goal.clone());
    let result = store.solve(&arena);
    assert!(result.is_ok());
    assert!(store.proven_goals().contains(&goal));
}

#[test]
fn chr_solve_deduplicates_goals() {
    let mut interner = Interner::new();
    let display_sym = interner.intern("Display");
    let int_sym = interner.intern("Int");
    let mut arena = TyArena::new();
    let int_ty = arena.alloc(TyKind::Named(int_sym));

    let goal = Goal::TraitImpl(display_sym, vec![int_ty]);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![],
    };

    let mut store = ChrStore::new(vec![rule]);
    store.push_goal(goal.clone());
    store.push_goal(goal.clone());
    let result = store.solve(&arena);
    assert!(result.is_ok());
}

#[test]
fn chr_solve_reflectable() {
    let mut interner = Interner::new();
    let user_sym = interner.intern("User");
    let mut arena = TyArena::new();
    let user_ty = arena.alloc(TyKind::Named(user_sym));

    let goal = Goal::Reflectable(user_ty);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![],
    };

    let mut store = ChrStore::new(vec![rule]);
    store.push_goal(goal.clone());
    let result = store.solve(&arena);
    assert!(result.is_ok());
    assert!(store.proven_goals().contains(&goal));
}

#[test]
fn chr_solve_has_field() {
    let mut interner = Interner::new();
    let user_sym = interner.intern("User");
    let name_sym = interner.intern("name");
    let mut arena = TyArena::new();
    let user_ty = arena.alloc(TyKind::Named(user_sym));

    let goal = Goal::HasField(user_ty, name_sym);
    let rule = ChrRule::Simplify {
        goal: goal.clone(),
        premises: vec![Goal::Reflectable(user_ty)],
    };

    let reflectable_rule = ChrRule::Simplify {
        goal: Goal::Reflectable(user_ty),
        premises: vec![],
    };

    let mut store = ChrStore::new(vec![rule, reflectable_rule]);
    store.push_goal(goal.clone());
    let result = store.solve(&arena);
    assert!(result.is_ok());
    assert!(store.proven_goals().contains(&goal));
}
