use crate::LspState;
use glyim_db::{CrateConfig, Database};
use tokio::runtime::Runtime;

#[test]
fn run_server_wires_router() {
    // This test just ensures LspState and driver can be initialized.
    // We need a Tokio runtime for the driver to spawn.
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let config = CrateConfig {
            name: "test".to_string(),
            target_triple: "x86_64".to_string(),
            opt_level: 0,
        };
        let db = Database::new(config);
        let mut state = LspState::new(db);
        state.start_driver();
        // If we get here without panic, the router is wired enough.
    });
}
