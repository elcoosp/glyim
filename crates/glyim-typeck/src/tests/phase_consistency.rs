use crate::staging::Level;

#[test]
fn level_ordering() {
    assert!(Level::Comptime < Level::Buildtime);
    assert!(Level::Buildtime < Level::Runtime);
    assert!(Level::Comptime < Level::Runtime);
}

#[test]
fn phase_consistency_same_level_ok() {
    // A value defined at Runtime can be used at Runtime
    assert!(Level::Runtime <= Level::Runtime);
    assert!(Level::Buildtime <= Level::Buildtime);
    assert!(Level::Comptime <= Level::Comptime);
}

#[test]
fn phase_violation_runtime_at_comptime_fails() {
    // A value defined at Runtime cannot be used at Comptime
    assert!(Level::Runtime > Level::Comptime);
    assert!(!(Level::Runtime <= Level::Comptime));
}

#[test]
fn cross_stage_persistence_ok() {
    // Earlier stage values are available at later stages
    assert!(Level::Comptime <= Level::Runtime); // comptime value at runtime
    assert!(Level::Comptime <= Level::Buildtime); // comptime value at buildtime
    assert!(Level::Buildtime <= Level::Runtime); // buildtime value at runtime
}
