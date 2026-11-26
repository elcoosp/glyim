use crate::{BasicBlockIdx, SwitchTargets};

#[test]
fn switch_targets_iter_yields_correct_branches() {
    let bb1 = BasicBlockIdx::from_usize(1);
    let bb2 = BasicBlockIdx::from_usize(2);
    let bb3 = BasicBlockIdx::from_usize(3);
    let branches = Box::new([(5, bb1), (10, bb2)]);
    let otherwise = bb3;
    let targets = SwitchTargets::new(branches, otherwise);
    let mut iter = targets.iter();
    assert_eq!(iter.next(), Some((5, bb1)));
    assert_eq!(iter.next(), Some((10, bb2)));
    assert_eq!(iter.next(), None);
    assert_eq!(targets.otherwise(), bb3);
}

#[test]
fn switch_targets_if_switch_works() {
    let then_bb = BasicBlockIdx::from_usize(7);
    let else_bb = BasicBlockIdx::from_usize(8);
    let targets = SwitchTargets::if_switch(then_bb, else_bb);
    let mut iter = targets.iter();
    assert_eq!(iter.next(), Some((1, then_bb)));
    assert_eq!(iter.next(), None);
    assert_eq!(targets.otherwise(), else_bb);
}
