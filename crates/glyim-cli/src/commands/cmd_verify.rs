use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore};
use glyim_pkg::lockfile::parse_lockfile;
use std::path::PathBuf;

pub fn cmd_verify() -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let lockfile_path = dir.join("glyim.lock");
        if !lockfile_path.exists() {
            eprintln!("No glyim.lock found.");
            return Ok(1);
        }
        let content = std::fs::read_to_string(&lockfile_path).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let lockfile = parse_lockfile(&content).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;

        let cas_dir = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from(".glyim/cas"))
            .join("cas");
        let store = LocalContentStore::new(&cas_dir).map_err(|e| {
            eprintln!("error opening CAS: {e}");
            1
        })?;

        let mut all_ok = true;
        for pkg in &lockfile.packages {
            match pkg.hash.parse::<ContentHash>() {
                Ok(hash) => {
                    if store.retrieve(hash).is_none() {
                        eprintln!("error: package {} {} - blob not found in CAS", pkg.name, pkg.version);
                        all_ok = false;
                    }
                }
                Err(e) => {
                    eprintln!("error: package {} {} - invalid hash: {e}", pkg.name, pkg.version);
                    all_ok = false;
                }
            }
        }

        if all_ok {
            eprintln!("Lockfile verified: {} packages, all blobs present.", lockfile.packages.len());
            Ok(0)
        } else {
            eprintln!("Lockfile verification failed.");
            Ok(1)
        }
    })();
    result.unwrap_or_else(|code| code)
}
