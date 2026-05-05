use crate::monomorphize::mangle_table::MangleTable;
use crate::types::HirType;
use glyim_interner::Interner;

#[test]
fn mangle_table_is_deterministic() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let args = vec![HirType::Int];
    let mut table = MangleTable::new();

    let s1 = table.mangle(vec_sym, &args, &mut interner);
    let s2 = table.mangle(vec_sym, &args, &mut interner);
    assert_eq!(
        s1, s2,
        "same (base, args) must produce identical mangled symbol"
    );
}

#[test]
fn mangle_table_different_args_different_names() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let mut table = MangleTable::new();

    let s_i64 = table.mangle(vec_sym, &[HirType::Int], &mut interner);
    let s_bool = table.mangle(vec_sym, &[HirType::Bool], &mut interner);
    assert_ne!(
        s_i64, s_bool,
        "different type args must produce distinct names"
    );
}

#[test]
fn mangle_table_handles_multiple_args() {
    let mut interner = Interner::new();
    let hashmap_sym = interner.intern("HashMap");
    let mut table = MangleTable::new();

    let s1 = table.mangle(hashmap_sym, &[HirType::Str, HirType::Int], &mut interner);
    let s2 = table.mangle(hashmap_sym, &[HirType::Str, HirType::Int], &mut interner);
    assert_eq!(s1, s2);

    let name = interner.resolve(s1);
    assert!(
        name.starts_with("HashMap__"),
        "expected HashMap__ prefix, got {}",
        name
    );
    assert!(
        name.contains("str") && name.contains("i64"),
        "expected str and i64 in name, got {}",
        name
    );
}
