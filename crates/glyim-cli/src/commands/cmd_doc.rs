use glyim_compiler::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool, test: bool, version: Option<String>) -> i32 {
    if test {
        match pipeline::run_doctests(&input) {
            Ok(failed) => {
                if failed == 0 { println!("All doc-tests passed."); 0 } else { eprintln!("{} doc-test(s) failed.", failed); 1 }
            }
            Err(e) => { eprintln!("error running doc-tests: {e}"); 1 }
        }
    } else {
        // Default output to crates/glyim-doc/site (relative to workspace root)
        let out_dir = output.unwrap_or_else(|| {
            let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            workspace_root.join("crates/glyim-doc/site")
        });
        let version_ref = version.as_deref();
        let result = if input.is_dir() {
            pipeline::generate_doc(&input, Some(&out_dir), version_ref)
        } else {
            let package_dir = input.parent().unwrap_or_else(|| std::path::Path::new("."));
            pipeline::generate_doc(package_dir, Some(&out_dir), version_ref)
        };
        match result {
            Ok(()) => {
                if open {
                    let index_html = out_dir.join("index.html");
                    if index_html.exists() {
                        let _ = webbrowser::open(&format!("file://{}", index_html.display()));
                    } else { eprintln!("error: generated file not found"); return 1; }
                }
                0
            }
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
    }
}
