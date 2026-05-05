use glyim_compiler::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool, test: bool) -> i32 {
    let out_dir = output.unwrap_or_else(|| PathBuf::from("doc"));
    if test {
        match pipeline::run_doctests(&input) {
            Ok(failed) => {
                if failed == 0 {
                    println!("All doc-tests passed.");
                    0
                } else {
                    eprintln!("{} doc-test(s) failed.", failed);
                    1
                }
            }
            Err(e) => {
                eprintln!("error running doc-tests: {e}");
                1
            }
        }
    } else {
        match pipeline::generate_doc(&input, Some(&out_dir)) {
            Ok(()) => {
                if open {
                    let index_html = out_dir.join("index.html");
                    let abs_path = std::fs::canonicalize(&index_html).unwrap_or_else(|_| {
                        let mut abs =
                            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        abs.push(&index_html);
                        abs
                    });
                    if abs_path.exists() {
                        eprintln!("Opening {}", abs_path.display());
                        if let Err(e) = webbrowser::open(&format!("file://{}", abs_path.display()))
                        {
                            eprintln!("warning: could not open browser: {e}");
                        }
                    } else {
                        eprintln!("error: generated file not found at {}", abs_path.display());
                        return 1;
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        }
    }
}
