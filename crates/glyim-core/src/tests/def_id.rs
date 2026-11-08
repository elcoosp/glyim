use crate::def_id::{AdtId, CrateId, DefId, LocalDefId};

#[test]
fn crate_id_roundtrip() {
    let raw = 42;
    let id = CrateId::from_raw(raw);
    assert_eq!(id.to_raw(), raw);
    assert_eq!(id.index(), raw as usize);
    assert_eq!(format!("{id}"), "crate[42]");
}

#[test]
fn local_def_id_roundtrip() {
    let raw = 99;
    let id = LocalDefId::from_raw(raw);
    assert_eq!(id.to_raw(), raw);
    assert_eq!(id.index(), raw as usize);
}

#[test]
fn def_id_new_display() {
    let krate = CrateId::from_raw(1);
    let local = LocalDefId::from_raw(2);
    let def = DefId::new(krate, local);
    assert_eq!(def.krate, krate);
    assert_eq!(def.local_id, local);
    assert_eq!(format!("{def}"), "crate[1]::2");
}

#[test]
fn other_def_ids() {
    let adt = AdtId::from_raw(10);
    assert_eq!(adt.to_raw(), 10);
    assert_eq!(adt.index(), 10);
}
