use glyim_db::Database;
use glyim_pipeline::compile_file_to_mir;
use std::io::Write;

#[test]
fn min_option_probe() {
    let src = r#"
enum Option<T> {
    None,
    Some(T),
}

impl<T> Option<T> {
    fn is_some(&self) -> bool {
        match self {
            Option::Some(_) => true,
            Option::None => false,
        }
    }

    fn map<U>(self, f: fn(T) -> U) -> Option<U> {
        match self {
            Option::Some(val) => Option::Some(f(val)),
            Option::None => Option::None,
        }
    }
}
"#;
    let path = std::env::temp_dir().join("glyim_min_probe.g");
    let mut f = std::fs::File::create(&path).unwrap();
    write!(f, "{}", src).unwrap();
    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-apple-darwin".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);
    match compile_file_to_mir(&mut db, &path) {
        Ok(_) => { println!("COMPILE OK"); }
        Err(diags) => {
            for d in &diags {
                eprintln!("DIAG: {} | {:?}", d.message, &src[(d.span.primary.lo.to_usize().saturating_sub(20)).min(src.len())..(d.span.primary.hi.to_usize()+10).min(src.len())]);
            }
            panic!("min probe failed with {} diagnostics", diags.len());
        }
    }
}
