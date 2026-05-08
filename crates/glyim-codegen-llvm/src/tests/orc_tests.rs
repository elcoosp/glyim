use crate::orc::OrcSession;

#[test]
fn orc_session_create_and_drop() {
    let session = OrcSession::new();
    drop(session);
}

#[test]
fn orc_session_create_dylib() {
    let mut session = OrcSession::new();
    let dylib = session.create_dylib("main_lib");
    assert!(dylib.is_ok());
}

#[test]
fn orc_session_create_multiple_dylibs() {
    let mut session = OrcSession::new();
    let d1 = session.create_dylib("lib_a");
    let d2 = session.create_dylib("lib_b");
    assert!(d1.is_ok());
    assert!(d2.is_ok());
}

#[test]
fn orc_dylib_has_name() {
    let mut session = OrcSession::new();
    let dylib = session.create_dylib("test_lib").unwrap();
    assert_eq!(dylib.name(), "test_lib");
}

#[test]
fn orc_session_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<OrcSession>();
}
