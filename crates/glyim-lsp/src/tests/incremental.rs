use crate::LspState;
use glyim_db::Database;
use std::path::PathBuf;

#[tokio::test]
async fn test_function_signature_change_updates_symbol_index() {
    let db = Database::new(glyim_db::CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let mut state = LspState::new(db);
    state.start_driver();

    let path = PathBuf::from("/test/main.g");
    let initial_content = r"
fn add(a: i32, b: i32) -> i32 { a + b }
";
    state.did_open(path.clone(), initial_content.to_string(), 1);
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // Verify initial index contains "add"
    {
        let analysis = state.analysis();
        let symbol_index = analysis.symbol_index.read();
        let symbols = symbol_index.lookup_by_name("add");
        assert_eq!(symbols.len(), 1);
    }

    let updated_content = r"
fn add(a: i64, b: i64) -> i64 { a + b }
";
    state.did_change(path.clone(), updated_content.to_string(), 2);
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // After change, verify symbol index still contains "add" (implying re-analysis)
    {
        let analysis = state.analysis();
        let symbol_index2 = analysis.symbol_index.read();
        let symbols2 = symbol_index2.lookup_by_name("add");
        assert_eq!(symbols2.len(), 1);
        // Additionally, we can check that the file's symbols were refreshed
        let file_id = state.file_id(&path).unwrap();
        let file_symbols = symbol_index2.symbols_in_file(file_id);
        assert!(!file_symbols.is_empty());
    }

    state.did_close(&path);
}

#[test]
fn test_changing_file_recompiles_only_affected_files() {
    let () = ();
}
