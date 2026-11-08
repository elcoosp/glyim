use crate::mono::MonoItemData;
use crate::partition::partition;
use std::sync::Arc;

fn make_item(index: u32, module: u32) -> MonoItemData {
    use glyim_core::def_id::{CrateId, DefId, LocalDefId};
    let body = Arc::new(glyim_mir::Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(0),
    )));
    MonoItemData {
        item: crate::mono::MonoItem::Fn {
            def_id: glyim_core::def_id::FnDefId::from_raw(index),
            substs: glyim_type::Substitution::empty(),
        },
        body,
        symbol: format!("item_{index}"),
        source_module: module,
    }
}

#[test]
fn v25_t01_single_module_single_cgu() {
    let items = vec![make_item(0, 1), make_item(1, 1)];
    let cgus = partition(&items, 10);
    assert_eq!(cgus.len(), 1);
    assert_eq!(cgus[0].len(), 2);
    assert!(cgus[0].contains(&0));
    assert!(cgus[0].contains(&1));
}

#[test]
fn v25_t02_multiple_modules_separate_cgus() {
    let items = vec![make_item(0, 1), make_item(1, 2), make_item(2, 3)];
    let cgus = partition(&items, 10);
    assert_eq!(cgus.len(), 3);
    let mut found = [false; 3];
    for cgu in &cgus {
        assert_eq!(cgu.len(), 1);
        let idx = cgu[0];
        found[idx] = true;
    }
    assert!(found.iter().all(|x| *x));
}

#[test]
fn v25_t03_cross_module_call_no_merge() {
    let items = vec![make_item(0, 1), make_item(1, 2)];
    let cgus = partition(&items, 10);
    assert_eq!(cgus.len(), 2);
    assert_eq!(cgus[0].len(), 1);
    assert_eq!(cgus[1].len(), 1);
}

#[test]
fn v25_t04_large_crate_multiple_cgus_limit() {
    let mut items = Vec::new();
    for i in 0..10 {
        items.push(make_item(i, i));
    }
    let cgus = partition(&items, 3);
    assert!(cgus.len() <= 3);
    let total: usize = cgus.iter().map(|g| g.len()).sum();
    assert_eq!(total, 10);
}
