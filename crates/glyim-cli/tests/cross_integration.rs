use glyim_cli::cross;

#[test]
fn ensure_sysroot_bogus_triple() {
    assert!(cross::ensure_sysroot("bogus-none-none").is_err());
}

#[test]
fn host_target_is_valid() {
    let host = cross::host_target();
    assert!(cross::validate_target(&host).is_ok());
}

#[test]
fn all_supported_targets_validate() {
    for target in cross::SUPPORTED_TARGETS {
        assert!(
            cross::validate_target(target).is_ok(),
            "target {} should be valid",
            target
        );
    }
}
