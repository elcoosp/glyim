use crate::comptime::{FuelBudget, ComptimeContext};

#[test]
fn fuel_budget_default() {
    let budget = FuelBudget::default();
    assert_eq!(budget.max_instructions, 1_000_000);
}

#[test]
fn fuel_budget_custom() {
    let budget = FuelBudget::new(500_000);
    assert_eq!(budget.max_instructions, 500_000);
}

#[test]
fn fuel_budget_consume() {
    let mut budget = FuelBudget::new(100);
    assert!(!budget.consume(50));
    assert_eq!(budget.remaining(), 50);
    assert!(budget.consume(60)); // Exceeded
}

#[test]
fn comptime_context_records_dependencies() {
    let arena = crate::ty::TyArena::new();
    let mut ctx = ComptimeContext::new(&arena);
    let _ = ctx.trait_is_implemented("Display", "Int");
    let _ = ctx.get_fields("Vec");
    let deps = ctx.dependencies();
    assert_eq!(deps.len(), 2);
}
