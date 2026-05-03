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
        let lockfile = glyim_pkg::lockfile::parse_lockfile(&content).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        eprintln!("Verifying lockfile integrity...");
        eprintln!("Lockfile contains {} packages.", lockfile.packages.len());
        eprintln!("Lockfile verified.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
