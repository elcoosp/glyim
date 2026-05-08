use std::path::PathBuf;

pub fn cmd_coverage_report(input: PathBuf) -> i32 {
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };

    let cov_dir = input.parent().unwrap_or(std::path::Path::new("."));
    let cov_file = cov_dir.join("glyim-cov.json");
    if !cov_file.exists() {
        eprintln!("error: coverage file not found at {}", cov_file.display());
        eprintln!("Run `glyim run --coverage {}` first", input.display());
        return 1;
    }

    let data = match std::fs::read_to_string(&cov_file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error reading coverage file: {}", e);
            return 1;
        }
    };

    let dump: glyim_coverage::data::CoverageDump = match serde_json::from_str(&data) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error parsing coverage data: {}", e);
            return 1;
        }
    };

    let report = glyim_coverage::report::generate_text_report(&dump, &source);
    println!("{}", report);
    0
}

pub fn cmd_coverage_html(input: PathBuf, output: Option<PathBuf>) -> i32 {
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };

    let cov_dir = input.parent().unwrap_or(std::path::Path::new("."));
    let cov_file = cov_dir.join("glyim-cov.json");
    if !cov_file.exists() {
        eprintln!("error: coverage file not found at {}", cov_file.display());
        eprintln!("Run `glyim run --coverage {}` first", input.display());
        return 1;
    }

    let data = match std::fs::read_to_string(&cov_file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error reading coverage file: {}", e);
            return 1;
        }
    };

    let dump: glyim_coverage::data::CoverageDump = match serde_json::from_str(&data) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error parsing coverage data: {}", e);
            return 1;
        }
    };

    let html = glyim_coverage::html_report::generate_html_report(
        &dump,
        &source,
        &input.to_string_lossy(),
    );

    let out_path = output.unwrap_or_else(|| PathBuf::from("coverage.html"));
    if let Err(e) = std::fs::write(&out_path, &html) {
        eprintln!("error writing HTML report: {}", e);
        return 1;
    }
    eprintln!("HTML coverage report written to {}", out_path.display());
    0
}
