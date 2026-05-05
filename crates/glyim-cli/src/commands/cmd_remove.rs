use glyim_compiler::lockfile_integration;
use glyim_pkg::manifest::PackageManifest;

pub fn cmd_remove(package: String) -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        let manifest_path = dir.join("glyim.toml");
        let toml_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("error: glyim.toml not found");
            } else {
                eprintln!("error: {e}");
            }
            1
        })?;
        let mut m: PackageManifest =
            glyim_pkg::manifest::parse_manifest(&toml_str, &manifest_path.to_string_lossy())
                .map_err(|e| {
                    eprintln!("error: invalid glyim.toml: {e}");
                    1
                })?;
        m.dependencies.remove(&package);
        m.macros.remove(&package);
        let new_toml = toml::to_string_pretty(&m).unwrap_or_default();
        std::fs::write(&manifest_path, new_toml).map_err(|e| {
            eprintln!("error writing manifest: {e}");
            1
        })?;
        eprintln!("Removed {package} from dependencies");
        match lockfile_integration::resolve_and_write_lockfile(&dir, &m) {
            Ok(()) => {}
            Err(e) => eprintln!("warning: could not resolve dependencies: {e}"),
        }
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
