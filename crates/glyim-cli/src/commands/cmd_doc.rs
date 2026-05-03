use crate::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool) -> i32 {
    let out_dir = output.unwrap_or_else(|| PathBuf::from("doc"));
    match pipeline::generate_doc(&input, Some(&out_dir)) {
        Ok(()) => {
            if open {
                let index_html = out_dir.join("index.html");
                // Try to get the absolute path
                let abs = std::fs::canonicalize(&index_html).unwrap_or_else(|_| {
                    let mut cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    cwd.push(&index_html);
                    cwd
                });
                if abs.exists() {
                    eprintln!("Opening {}", abs.display());
                    if let Err(e) = webbrowser::open(&format!("file://{}", abs.display())) {
                        eprintln!("warning: could not open browser: {e}");
                    }
                } else {
                    eprintln!("error: generated file not found at {}", abs.display());
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
