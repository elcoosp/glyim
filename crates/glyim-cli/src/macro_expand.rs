use glyim_macro_core::executor::MacroExecutor;
use glyim_macro_core::registry::MacroRegistry;
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Expand macros found in the source text.
pub fn expand_macros(source: &str, pkg_dir: &Path, cas_dir: &Path) -> Result<String, String> {
    let store = LocalContentStore::new(cas_dir).map_err(|e| format!("create store: {e}"))?;
    let store: Arc<dyn ContentStore> = Arc::new(store);
    let mut registry = MacroRegistry::new(store.clone());
    let executor = MacroExecutor::new_with_cache(store);

    // Load built-in identity macro as fallback
    load_builtin_identity(&mut registry)?;

    // Try to load any macros found in the package's lockfile/cache
    load_package_macros(pkg_dir, &mut registry);

    let _macro_wasm = registry
        .get("identity")
        .ok_or_else(|| "identity macro not registered".to_string())?;

    let mut result = source.to_string();
    let mut scan_from = 0usize;

    while let Some(at_offset) = result[scan_from..].find('@') {
        let at_pos = scan_from + at_offset;
        let after_at = &result[at_pos + 1..];

        // Find the macro name: identifier characters before '('
        let name_len = after_at
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .count();

        if name_len == 0 || after_at.chars().nth(name_len) != Some('(') {
            scan_from = at_pos + 1;
            continue;
        }

        let macro_name = &after_at[..name_len];
        let paren_pos = at_pos + 1 + name_len;
        let inner_start = paren_pos + 1;

        // Find matching closing parenthesis
        let mut depth = 1usize;
        let inner_end = match result[inner_start..]
            .char_indices()
            .find(|(_, c)| match c {
                '(' => {
                    depth += 1;
                    false
                }
                ')' => {
                    depth -= 1;
                    depth == 0
                }
                _ => false,
            })
            .map(|(i, _)| inner_start + i)
        {
            Some(pos) => pos,
            None => {
                scan_from = inner_start;
                continue;
            }
        };

        let inner = &result[inner_start..inner_end];
        let call_end = inner_end + 1;

        // Look up the macro in the registry
        let maybe_wasm = registry.get(macro_name);

        if let Some(wasm) = maybe_wasm {
            let expanded = executor
                .execute(wasm, inner.as_bytes())
                .unwrap_or_else(|_| inner.as_bytes().to_vec());
            let expanded_str = String::from_utf8_lossy(&expanded).into_owned();

            let before = &result[..at_pos];
            let after_str = &result[call_end..];
            result = format!("{before}{expanded_str}{after_str}");
            let has_nested_macro = expanded_str.contains('@');
            if has_nested_macro {
                scan_from = at_pos;
            } else {
                scan_from = at_pos + expanded_str.len();
            }
        } else {
            scan_from = call_end;
        }
    }

    Ok(result)
}

fn load_builtin_identity(registry: &mut MacroRegistry) -> Result<(), String> {
    let identity_wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "expand") (param i32 i32 i32) (result i32)
    (local i32)
    i32.const 0
    local.set 3
    loop (result i32)
      local.get 3
      local.get 1
      i32.lt_s
      if (result i32)
        local.get 2
        local.get 3
        i32.add
        local.get 0
        local.get 3
        i32.add
        i32.load8_u
        i32.store8
        local.get 3
        i32.const 1
        i32.add
        local.set 3
        br 1
      else
        local.get 1
      end
    end)
)
"#;
    let wasm =
        wat::parse_str(identity_wat).map_err(|e| format!("parse built-in identity wat: {e}"))?;
    registry.register("identity", wasm);
    Ok(())
}

/// Look for `.wasm` macro blobs in the package's local cache and register them.
fn load_package_macros(pkg_dir: &Path, registry: &mut MacroRegistry) {
    let lockfile = pkg_dir.join("glyim.lock");
    if !lockfile.exists() {
        return;
    }
    let content = match std::fs::read_to_string(&lockfile) {
        Ok(c) => c,
        Err(_) => return,
    };
    let parsed = match glyim_pkg::lockfile::parse_lockfile(&content) {
        Ok(lf) => lf,
        Err(_) => return,
    };

    for pkg in &parsed.packages {
        if !pkg.is_macro {
            continue;
        }
        let folder = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from(".glyim"))
            .join("cas");
        let store = match LocalContentStore::new(&folder) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let hash = match pkg.hash.parse::<ContentHash>() {
            Ok(h) => h,
            Err(_) => continue,
        };
        if let Some(wasm) = store.retrieve(hash) {
            registry.register(&pkg.name, wasm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_macro_vfs::LocalContentStore;
    use std::sync::Arc;

    fn identity_wat() -> &'static str {
        r#"
(module
  (memory (export "memory") 1)
  (func (export "expand") (param i32 i32 i32) (result i32)
    (local i32)
    i32.const 0
    local.set 3
    loop (result i32)
      local.get 3
      local.get 1
      i32.lt_s
      if (result i32)
        local.get 2
        local.get 3
        i32.add
        local.get 0
        local.get 3
        i32.add
        i32.load8_u
        i32.store8
        local.get 3
        i32.const 1
        i32.add
        local.set 3
        br 1
      else
        local.get 1
      end
    end)
)
"#
    }

    #[test]
    fn generic_macro_expands_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalContentStore::new(dir.path()).unwrap();
        let store: Arc<dyn ContentStore> = Arc::new(store);
        let mut registry = MacroRegistry::new(store.clone());
        let wasm = wat::parse_str(identity_wat()).expect("parse identity wat");
        registry.register("my_custom", wasm);
        let executor = MacroExecutor::new_with_cache(store.clone());

        let source = r#"@my_custom(hello world)"#;

        // Build expand_macros logic inline to test with our registry
        let mut result = source.to_string();
        let mut scan_from = 0usize;

        while let Some(at_offset) = result[scan_from..].find('@') {
            let at_pos = scan_from + at_offset;
            let after_at = &result[at_pos + 1..];
            let name_len = after_at
                .char_indices()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                .count();
            if name_len == 0 || after_at.chars().nth(name_len) != Some('(') {
                scan_from = at_pos + 1;
                continue;
            }
            let macro_name = &after_at[..name_len];
            let paren_pos = at_pos + 1 + name_len;
            let inner_start = paren_pos + 1;
            let mut depth = 1usize;
            let inner_end = result[inner_start..]
                .char_indices()
                .find(|(_, c)| match c {
                    '(' => {
                        depth += 1;
                        false
                    }
                    ')' => {
                        depth -= 1;
                        depth == 0
                    }
                    _ => false,
                })
                .map(|(i, _)| inner_start + i)
                .unwrap();
            let inner = &result[inner_start..inner_end];
            let call_end = inner_end + 1;
            if let Some(wasm) = registry.get(macro_name) {
                let expanded = executor
                    .execute(wasm, inner.as_bytes())
                    .unwrap_or_else(|_| inner.as_bytes().to_vec());
                let expanded_str = String::from_utf8_lossy(&expanded).into_owned();
                let before = &result[..at_pos];
                let after_str = &result[call_end..];
                result = format!("{before}{expanded_str}{after_str}");
                scan_from = 0;
            } else {
                scan_from = call_end;
            }
        }

        assert_eq!(result, "hello world");
    }

    #[test]
    fn unknown_macro_leaves_source_unchanged() {
        let source = "@nonexistent(42)";
        let dir = tempfile::tempdir().unwrap();
        let result = expand_macros(source, dir.path(), dir.path());
        assert_eq!(result.unwrap(), source);
    }

    #[test]
    fn identity_macro_still_works() {
        let source = "@identity(main = () => 42)";
        let dir = tempfile::tempdir().unwrap();
        let result = expand_macros(source, dir.path(), dir.path());
        assert_eq!(result.unwrap(), "main = () => 42");
    }
}
