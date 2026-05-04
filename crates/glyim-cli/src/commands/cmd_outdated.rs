use glyim_pkg::lockfile::parse_lockfile;
use glyim_pkg::registry::RegistryClient;

pub fn cmd_outdated() -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let lockfile_path = dir.join("glyim.lock");
        if !lockfile_path.exists() {
            eprintln!("No glyim.lock found. Run 'glyim fetch' first.");
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

        let registry_url = std::env::var("GLYIM_REGISTRY")
            .unwrap_or_else(|_| "https://registry.glyim.dev".to_string());
        let client = match RegistryClient::new(&registry_url) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: cannot contact registry: {e}");
                return Ok(1);
            }
        };

        let mut any_outdated = false;
        for pkg in &lockfile.packages {
            match client.get_latest_version(&pkg.name) {
                Ok(Some(latest)) if latest != pkg.version => {
                    println!("{} {} -> {}", pkg.name, pkg.version, latest);
                    any_outdated = true;
                }
                Ok(_) => {} // up to date or not found
                Err(e) => {
                    eprintln!("warning: could not check {}: {}", pkg.name, e);
                }
            }
        }

        if !any_outdated {
            println!("All dependencies are up to date.");
        }
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
