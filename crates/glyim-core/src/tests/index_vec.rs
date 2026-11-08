use crate::arena::{Idx, IndexVec};

#[test]
fn index_vec_operations() {
    let mut vec: IndexVec<Idx<u32>, u32> = IndexVec::new();
    assert!(vec.is_empty());
    let idx0 = vec.push(100);
    let idx1 = vec.push(200);
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[idx0], 100);
    assert_eq!(vec[idx1], 200);
    assert_eq!(vec.get(idx0), Some(&100));
    assert_eq!(vec.get_mut(idx1), Some(&mut 200));
    *vec.get_mut(idx1).unwrap() = 300;
    assert_eq!(vec[idx1], 300);
    let slice = vec.as_slice();
    assert_eq!(slice, &[100, 300]);
    let mut iter = vec.iter_enumerated();
    assert_eq!(iter.next(), Some((idx0, &100)));
    assert_eq!(iter.next(), Some((idx1, &300)));
}

#[test]
fn index_vec_with_capacity() {
    let mut vec: IndexVec<Idx<u8>, String> = IndexVec::with_capacity(10);
    vec.push("a".to_string());
    vec.reserve(5);
    assert_eq!(vec.len(), 1);
    // capacity is not exposed; skipping assertion
    let raw = vec.into_raw();
    assert_eq!(raw, vec!["a".to_string()]);
}

#[test]
fn index_vec_default() {
    let vec: IndexVec<Idx<()>, i32> = IndexVec::default();
    assert!(vec.is_empty());
}
