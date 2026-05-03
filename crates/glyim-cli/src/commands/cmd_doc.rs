use crate::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool) -> i32 {
    match pipeline::generate_doc(&input, output.as_deref()) {
        Ok(()) => {
            let out_dir = output.unwrap_or_else(|| PathBuf::from("doc"));
            let index_html = out_dir.join("index.html");
            if open {
                if let Err(e) = webbrowser::open(&index_html.to_string_lossy()) {
                    eprintln!("warning: could not open browser: {}", e);
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
