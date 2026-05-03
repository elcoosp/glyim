use crate::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool) -> i32 {
    let out_dir = output.unwrap_or_else(|| PathBuf::from("doc"));
    match pipeline::generate_doc(&input, Some(&out_dir)) {
        Ok(()) => {
            if open {
                let index_html = out_dir.join("index.html");
                let abs_path = std::fs::canonicalize(&index_html).unwrap_or_else(|_| {
                    // If canonicalize fails (e.g., file doesn't exist), fall back to absolute path
                    let mut abs = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    abs.push(&index_html);
                    abs
                });
                if let Err(e) = webbrowser::open(&abs_path.to_string_lossy()) {
                    eprintln!("warning: could not open browser: {e}");
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
