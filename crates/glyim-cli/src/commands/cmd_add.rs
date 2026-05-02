use super::*;

pub fn cmd_add(package: String, macro_dep: bool) -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| { eprintln!("error: {e}"); 1 })?;
        let manifest_path = dir.join("glyim.toml");
        let toml_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("error: glyim.toml not found");
            } else { eprintln!("error: {e}"); }
            1
        })?;
        let mut m: PackageManifest = glyim_pkg::manifest::parse_manifest(
            &toml_str, &manifest_path.to_string_lossy(),
        ).map_err(|e| { eprintln!("error: invalid glyim.toml: {e}"); 1 })?;
        let target_deps = if macro_dep { &mut m.macros } else { &mut m.dependencies };
        target_deps.insert(
            package.clone(),
            Dependency {
                version: Some("*".into()),
                path: None, registry: None, workspace: false, is_macro: macro_dep,
            },
        );
        let new_toml = toml::to_string_pretty(&m).unwrap_or_default();
        std::fs::write(&manifest_path, new_toml).map_err(|e| { eprintln!("error writing manifest: {e}"); 1 })?;
        eprintln!("Added {package} to {}", if macro_dep { "[macros]" } else { "[dependencies]" });
        match glyim_cli::lockfile_integration::resolve_and_write_lockfile(&dir, &m) {
            Ok(()) => {}
            Err(e) => eprintln!("warning: could not resolve dependencies: {e}"),
        }
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
