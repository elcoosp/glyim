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
        // Like 'cargo doc', default to the current package (where glyim.toml is)
        let package_dir = if input.join("glyim.toml").exists() {
            input
        } else if input.join("src/main.g").exists() {
            // Single file: use its parent as the package root
            input.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf()
        } else {
            // Default to current directory
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };

        let out_dir = output.unwrap_or_else(|| PathBuf::from("target/doc"));
        let version_ref = version.as_deref();

        match pipeline::generate_doc(&package_dir, Some(&out_dir), version_ref) {
            Ok(()) => {
                eprintln!("Documentation generated in {}", out_dir.display());
                if open {
                    let index_html = out_dir.join("index.html");
                    if index_html.exists() {
                        let _ = webbrowser::open(&format!("file://{}", index_html.display()));
                    }
                }
                0
            }
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
    }
}
