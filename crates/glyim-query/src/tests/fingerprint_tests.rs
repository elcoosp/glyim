use crate::fingerprint::Fingerprint;

#[test]
fn fingerprint_of_same_data_is_equal() {
    let a = Fingerprint::of(b"hello glyim");
    let b = Fingerprint::of(b"hello glyim");
    assert_eq!(a, b);
}

#[test]
fn fingerprint_of_different_data_is_not_equal() {
    let a = Fingerprint::of(b"hello");
    let b = Fingerprint::of(b"world");
    assert_ne!(a, b);
}

#[test]
fn fingerprint_of_empty_is_deterministic() {
    let a = Fingerprint::of(b"");
    let b = Fingerprint::of(b"");
    assert_eq!(a, b);
}

#[test]
fn fingerprint_to_hex_round_trips() {
    let fp = Fingerprint::of(b"test data");
    let hex = fp.to_hex();
    let restored = Fingerprint::from_hex(&hex).expect("parse hex");
    assert_eq!(fp, restored);
}

#[test]
fn fingerprint_from_hex_rejects_bad_input() {
    assert!(Fingerprint::from_hex("nothex").is_err());
    assert!(Fingerprint::from_hex("abcd").is_err()); // too short
}

#[test]
fn fingerprint_is_copy() {
    let a = Fingerprint::of(b"x");
    let _b = a;
    let _c = a; // Copy, not move
}

#[test]
fn fingerprint_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<Fingerprint>();
}

#[test]
fn fingerprint_hash_is_stable() {
    let fp = Fingerprint::of(b"glyim");
    assert_eq!(fp.to_hex().len(), 64);
}

#[test]
fn fingerprint_combine_two() {
    let a = Fingerprint::of(b"hello");
    let b = Fingerprint::of(b"world");
    let combined = Fingerprint::combine(a, b);
    assert_ne!(combined, a);
    assert_ne!(combined, b);
    let combined2 = Fingerprint::combine(a, b);
    assert_eq!(combined, combined2);
}

#[test]
fn fingerprint_combine_order_matters() {
    let a = Fingerprint::of(b"hello");
    let b = Fingerprint::of(b"world");
    assert_ne!(Fingerprint::combine(a, b), Fingerprint::combine(b, a));
}

#[test]
fn fingerprint_combine_list() {
    let fps: Vec<Fingerprint> = vec![
        Fingerprint::of(b"a"),
        Fingerprint::of(b"b"),
        Fingerprint::of(b"c"),
    ];
    let combined = Fingerprint::combine_all(&fps);
    assert_ne!(combined, fps[0]);
    let empty = Fingerprint::combine_all(&[]);
    assert_eq!(empty, Fingerprint::ZERO);
}
