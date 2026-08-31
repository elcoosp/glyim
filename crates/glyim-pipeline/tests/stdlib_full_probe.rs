//! Full diagnostic dump for the assembled glyim standard library. Verifies the
//! modular (Option A) stdlib compiles through the real pipeline. Temporarily
//! raises the diagnostic cap so every error surfaces at once.
use glyim_db::Database;
use glyim_diag::ErrorCategory;
use glyim_lang_std::std_source_assembled;
use glyim_pipeline::compile_file_to_mir;

#[test]
fn assembled_stdlib_compiles() {
    let src = std_source_assembled();
    let path = std::env::temp_dir().join("glyim_stdlib_assembled_full.g");
    std::fs::write(&path, &src).unwrap();
    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-apple-darwin".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);
    match compile_file_to_mir(&mut db, &path) {
        Ok(_) => {}
        Err(diags) => {
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            // Precompute module content-start offsets: each `pub mod X {` then
            // content begins after the `{`.
            let mut mods: Vec<(usize, &str)> = Vec::new();
            for (i, _) in src.match_indices("pub mod ") {
                let brace = match src[i..].find('{') {
                    Some(b) => i + b,
                    None => continue,
                };
                let name = &src[i + "pub mod ".len()..brace];
                let name = name.trim();
                mods.push((brace + 1, name));
            }
            mods.sort_by_key(|&(start, _)| start);
            for d in &diags {
                {
                    let off = d.span.primary.lo.to_usize();
                    // Find the module containing this offset.
                    let mut idx = 0;
                    for (k, (start, _)) in mods.iter().enumerate() {
                        if *start <= off {
                            idx = k;
                        } else {
                            break;
                        }
                    }
                    let (mod_start, module) = if mods.is_empty() {
                        (0usize, "?")
                    } else {
                        mods[idx]
                    };
                    let safe_start = mod_start.min(off);
                    let line_in_mod = src[safe_start..off].matches('\n').count() + 1;
                    let line_start = src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let line_end = src[off..].find('\n').map(|i| off + i).unwrap_or(src.len());
                    let line_no = src[..off].matches('\n').count() + 1;
                    eprintln!(
                        "DETAIL [{}:L{} (assembled L{})] {} | {}",
                        module, line_in_mod, line_no, d.message, src[line_start..line_end].trim()
                    );
                }
                let key = d
                    .message
                    .split('`')
                    .nth(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| d.message.clone());
                *counts.entry(key).or_insert(0) += 1;
            }
            let total = diags.len();
            eprintln!("ASSEMBLED_FAIL total={total}");
            let mut items: Vec<_> = counts.into_iter().collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            for (k, c) in items {
                eprintln!("  {c:>3}  {k}");
            }
            panic!("assembled stdlib failed with {total} diagnostics");
        }
    }
}
