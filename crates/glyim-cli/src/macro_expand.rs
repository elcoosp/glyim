use std::path::Path;
use std::sync::Arc;
use glyim_macro_core::executor::MacroExecutor;
use glyim_macro_core::registry::MacroRegistry;
use glyim_macro_vfs::{ContentStore, LocalContentStore};

pub fn expand_macros(
    source: &str,
    _pkg_dir: &Path,
    cas_dir: &Path,
) -> Result<String, String> {

    let store = LocalContentStore::new(cas_dir)
        .map_err(|e| format!("create store: {e}"))?;
    let store: Arc<dyn ContentStore> = Arc::new(store);
    let mut registry = MacroRegistry::new(store.clone());
    let executor = MacroExecutor::new_with_cache(store);

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
    let wasm = wat::parse_str(identity_wat)
        .map_err(|e| format!("parse wat: {e}"))?;
    registry.register("identity", wasm);
    let macro_wasm = registry.get("identity").unwrap();

    let marker = "@identity(";
    let mut result = source.to_string();

    loop {
        let pos = match result.find(marker) {
            Some(p) => {
                p + marker.len()
            }
            None => {
                break;
            }
        };

        let mut depth = 1;
        let end = result[pos..]
            .char_indices()
            .find(|(_, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map(|(i, _)| pos + i)
            .ok_or_else(|| "unmatched parenthesis".to_string())?;

        let inner = &result[pos..end];

        let expanded = executor
            .execute(macro_wasm, inner.as_bytes())
            .unwrap_or_else(|_| inner.as_bytes().to_vec());
        let expanded_str = String::from_utf8_lossy(&expanded).into_owned();

        let call_start = pos - marker.len();
        let call_end = end + 1;
        let before = &result[..call_start];
        let after = &result[call_end..];
        result = format!("{before}{expanded_str}{after}");
    }

    Ok(result)
}
