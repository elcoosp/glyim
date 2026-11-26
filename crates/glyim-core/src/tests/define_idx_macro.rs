use crate::arena::IndexVec;

crate::define_idx!(TestIdx);

#[test]
fn define_idx_works() {
    let idx = TestIdx::from_usize(5);
    assert_eq!(idx.to_raw(), 5);
    assert_eq!(idx.index(), 5);
}

#[test]
fn test_idx_with_index_vec() {
    let mut vec: IndexVec<TestIdx, String> = IndexVec::new();
    let idx1 = vec.push("first".to_string());
    let idx2 = vec.push("second".to_string());
    assert_eq!(vec[idx1], "first");
    assert_eq!(vec[idx2], "second");
    assert_eq!(vec.len(), 2);
}
