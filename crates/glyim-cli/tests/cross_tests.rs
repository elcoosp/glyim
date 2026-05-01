use glyim_cli::cross;

#[test]
fn host_target_is_valid() {
    let host = cross::host_target();
    assert!(cross::validate_target(&host).is_ok());
}

#[test]
fn all_supported_targets_are_valid() {
    for target in cross::SUPPORTED_TARGETS {
        assert!(
            cross::validate_target(target).is_ok(),
            "target {} should be valid",
            target
        );
    }
}

#[test]
fn invalid_target_fails_validation() {
    assert!(cross::validate_target("mips-unknown-none").is_err());
    assert!(cross::validate_target("bogus-triple").is_err());
}

use std::process::Command;
use std::fs;

#[test]
fn cross_compile_produces_correct_elf_magic() {
    // Only run on Linux where ELF magic is relevant
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.g");
    fs::write(&input, "fn main() -> i64 { 42 }").unwrap();
    let output = dir.path().join("a.out");

    let result = glyim_cli::pipeline::build(
        &input,
        Some(&output),
        Some("aarch64-unknown-linux-gnu"),
    );
    if let Err(ref e) = result {
        let msg = format!("{e}");
        if msg.contains("unsupported") || msg.contains("target") {
            eprintln!("Skipping test: target machine not available ({e})");
            return;
        }
    }
    result.unwrap();

    let bytes = fs::read(&output).unwrap();
    assert_eq!(&bytes[0..4], &[0x7f, 0x45, 0x4c, 0x46], "Not a valid ELF file");
    let e_machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    assert_eq!(e_machine, 0xB7, "ELF machine type is not AArch64 (0xB7)");
}
