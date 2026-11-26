use crate::arena::{Idx, IdxLike};

#[test]
fn idx_roundtrip() {
    let raw = 42u32;
    let idx: Idx<()> = Idx::from_usize(raw as usize);
    assert_eq!(idx.to_raw(), raw);
    assert_eq!(idx.index(), raw as usize);
}

#[test]
fn idx_like_trait() {
    fn check<I: IdxLike>(raw: u32) {
        let idx = I::from_u32(raw);
        assert_eq!(idx.to_raw(), raw);
        assert_eq!(idx.index(), raw as usize);
    }
    check::<Idx<()>>(100);
}
