use glyim_interner::Interner;
use glyim_hir::types::{HirType, TypeVar};
use crate::*;
use glyim_diag::Span;

fn sp() -> Span { Span::new(0, 1) }

// ── MangleTable deduplication ─────────────────────────────────

#[test]
fn test_mangle_table_deduplicates() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let mut table = MangleTable::new();

    let first = table.mangle(vec_sym, &[HirType::Int], &mut interner).unwrap();
    let second = table.mangle(vec_sym, &[HirType::Int], &mut interner).unwrap();
    assert_eq!(first, second, "Same base+args should produce same mangled symbol");

    let third = table.mangle(vec_sym, &[HirType::Bool], &mut interner).unwrap();
    assert_ne!(first, third, "Different args should produce different symbols");
}

// ── WorkQueue deduplication ───────────────────────────────────

#[test]
fn test_work_queue_deduplicates() {
    let mut interner = Interner::new();
    let foo = interner.intern("foo");
    let mut queue = WorkQueue::new();

    queue.push(
        WorkItem { kind: ItemKind::FnSpecialize, def_id: foo, type_args: vec![HirType::Int] },
        WorkItemContext { discovered_from: None, discovery_span: sp() },
        foo
    );
    queue.push(
        WorkItem { kind: ItemKind::FnSpecialize, def_id: foo, type_args: vec![HirType::Int] },
        WorkItemContext { discovered_from: None, discovery_span: sp() },
        foo // same dedup sym
    );

    let first = queue.pop();
    let second = queue.pop();
    assert!(first.is_some(), "Should have at least one item");
    assert!(second.is_none(), "Duplicate should be skipped");
}

// ── TypeMetadata records generic structures ───────────────────

#[test]
fn test_type_metadata_records_generic() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let mangled_vec_int = interner.intern("Vec__i64");

    let mut metadata = TypeMetadata::new();
    metadata.record(mangled_vec_int, TypeStructure::Generic {
        base: vec_sym,
        args: vec![HirType::Int],
    });

    assert!(metadata.get(mangled_vec_int).is_some());
    assert_eq!(metadata.get_base_symbol(mangled_vec_int), Some(vec_sym));
}

// ── Concretize error implements std::error::Error ─────────────

#[test]
fn test_concretize_error_is_std_error() {
    let err = ConcretizeError {
        kind: ConcretizeErrorKind::UnresolvedParam,
        ty: Box::new(HirType::Int),
        detail: "test".into(),
        span: sp(),
    };

    let _: &dyn std::error::Error = &err;
    assert!(format!("{}", err).contains("UnresolvedParam"));
}

// ── BFS Driver seeds non-generic functions ────────────────────

#[test]
fn test_mono_driver_processes_passthrough() {
    let mut interner = Interner::new();
    let add_sym = interner.intern("add");

    let mut fn_types_map = std::collections::HashMap::new();
    let mut expr_types = std::collections::HashMap::new();
    expr_types.insert(glyim_hir::types::ExprId::new(0), HirType::Int);

    fn_types_map.insert(add_sym, glyim_typeck::typeck::FnTypes {
        expr_types,
        call_type_args: std::collections::HashMap::new(),
        sizeof_types: std::collections::HashMap::new(),
        is_generic: false,
        type_params: vec![],
        span: sp(),
    });

    let driver = MonoDriver::new(&mut interner, &fn_types_map);
    let result = driver.run();

    assert!(result.failed_items.is_empty());
    assert!(result.metrics.fn_passthroughs >= 1);
}

// ── Mangling errors are std::error::Error ─────────────────────

#[test]
fn test_mangling_error_is_std_error() {
    let err = ManglingError::InferInType { type_var_index: 0 };
    let _: &dyn std::error::Error = &err;
    assert!(format!("{}", err).contains("Infer"));
}

// ── Mangling rejects Infer/Param ───────────────────────────────

#[test]
fn test_mangling_rejects_infer() {
    let mut interner = Interner::new();
    let result = mangling::type_to_short_string(
        &HirType::Infer(TypeVar::from_raw_unchecked(0)),
        &interner
    );
    assert!(result.is_err());
}

#[test]
fn test_mangling_rejects_param() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    let result = mangling::type_to_short_string(
        &HirType::Param(t),
        &interner
    );
    assert!(result.is_err());
}
