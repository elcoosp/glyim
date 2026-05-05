use glyim_compiler::lockfile_integration;

pub fn cmd_fetch() -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let packages = lockfile_integration::read_lockfile_packages(&dir).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        if packages.is_empty() {
            eprintln!("No dependencies to fetch (glyim.lock not found or empty)");
            return Ok(0);
        }
        eprintln!("Fetching {} package(s)...", packages.len());
        for pkg in &packages {
            eprintln!("  {} {} ({:?})", pkg.name, pkg.version, pkg.source);
        }
        eprintln!("Done.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
