use glyim_macro_vfs::LocalContentStore;
use glyim_pkg::wasm_publish::compile_and_store_macro_wasm;
use std::path::PathBuf;

pub fn cmd_publish(dry_run: bool, wasm: bool) -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let manifest_path = dir.join("glyim.toml");
        if !manifest_path.exists() {
            eprintln!("error: glyim.toml not found; run 'glyim init' first");
            return Ok(1);
        }
        if wasm {
            if dry_run {
                eprintln!("Dry run: would compile project to Wasm");
                return Ok(0);
            }
            let main_path = dir.join("src").join("main.g");
            if !main_path.exists() {
                eprintln!("error: no src/main.g found");
                return Ok(1);
            }
            let cas_dir = dirs_next::data_dir()
                .unwrap_or_else(|| PathBuf::from(".glyim/cas"))
                .join("cas");
            std::fs::create_dir_all(&cas_dir).map_err(|e| {
                eprintln!("error creating CAS dir: {e}");
                1
            })?;
            let store = LocalContentStore::new(&cas_dir).map_err(|e| {
                eprintln!("error opening CAS store: {e}");
                1
            })?;
            let hash =
                compile_and_store_macro_wasm(&main_path, "wasm32-wasi", &store).map_err(|e| {
                    eprintln!("error compiling to Wasm: {e}");
                    1
                })?;
            eprintln!("Macro Wasm content hash: {}", hash);
            return Ok(0);
        }
        if dry_run {
            eprintln!("Dry run: would publish from {}", dir.display());
        } else {
            eprintln!("error: publish not yet implemented");
            return Ok(1);
        }
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
