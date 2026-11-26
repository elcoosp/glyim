use crate::def_id::{AdtId, CrateId, DefId, LocalDefId};

#[test]
fn crate_id_roundtrip() {
    let raw = 42u32;
    let id = CrateId::from_usize(raw as usize);
    assert_eq!(id.to_raw(), raw);
    assert_eq!(id.index(), raw as usize);
    assert_eq!(format!("{id}"), "crate[42]");
}

#[test]
fn local_def_id_roundtrip() {
    let raw = 99u32;
    let id = LocalDefId::from_usize(raw as usize);
    assert_eq!(id.to_raw(), raw);
    assert_eq!(id.index(), raw as usize);
}

#[test]
fn def_id_new_display() {
    let krate = CrateId::from_usize(1);
    let local = LocalDefId::from_usize(2);
    let def = DefId::new(krate, local);
    assert_eq!(def.krate, krate);
    assert_eq!(def.local_id, local);
    assert_eq!(format!("{def}"), "crate[1]::2");
}

#[test]
fn other_def_ids() {
    let adt = AdtId::from_usize(10);
    assert_eq!(adt.to_raw(), 10);
    assert_eq!(adt.index(), 10);
}
