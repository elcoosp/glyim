use glyim_cli::cross;

#[test]
fn host_target_is_valid() {
    let host = cross::host_target();
    assert!(cross::validate_target(&host).is_ok());
}

#[test]
fn all_supported_targets_are_valid() {
    for target in cross::SUPPORTED_TARGETS {
        assert!(cross::validate_target(target).is_ok(), "target {} should be valid", target);
    }
}

#[test]
fn invalid_target_fails_validation() {
    assert!(cross::validate_target("mips-unknown-none").is_err());
    assert!(cross::validate_target("bogus-triple").is_err());
}
