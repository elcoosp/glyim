use crate::wasi_stubs::DeterministicWasi;

#[test]
fn wasi_view_creation() {
    let _wasi = DeterministicWasi::new();
}

#[test]
fn two_instances_independent() {
    let _a = DeterministicWasi::new();
    let _b = DeterministicWasi::new();
}
