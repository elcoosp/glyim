use std::path::PathBuf;
use std::fs;

/// A collection of programmatically-generated Glyim source files.
pub struct FixtureGenerator;

impl FixtureGenerator {
    /// Generate a single file with `function_count` functions, each returning its index.
    pub fn single_file(function_count: usize) -> Fixture {
        let mut source = String::from("// Auto-generated benchmark fixture\n\n");
        for i in 0..function_count {
            source.push_str(&format!(
                "fn fn_{}(x: i64) -> i64 {{ x + {} }}\n",
                i, i
            ));
        }
        source.push_str(
            &"fn main() -> i64 {\n    let mut sum = 0;\n"
        );
        for i in 0..function_count {
            source.push_str(&format!("    sum = sum + fn_{}({});\n", i, i));
        }
        source.push_str("    sum\n}\n");
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("fixture.g");
        fs::write(&path, &source).expect("write fixture");
        Fixture {
            source,
            path,
            _tmp_dir: tmp, // keep directory alive
        }
    }

    /// Generate a fixture with `count` generic functions.
    pub fn generic_functions(count: usize) -> Fixture {
        let mut source = String::from("// generic fixture\n");
        for i in 0..count {
            source.push_str(&format!(
                "fn id_{}<T>(x: T) -> T {{ x }}\n",
                i
            ));
        }
        source.push_str("fn main() -> i64 {\n");
        for i in 0..count {
            source.push_str(&format!("    let _ = id_{}({});\n", i, i));
        }
        source.push_str("    0\n}\n");
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("generic.g");
        fs::write(&path, &source).expect("write generic");
        Fixture {
            source,
            path,
            _tmp_dir: tmp,
        }
    }

    /// Generate a workspace with `package_count` packages, each containing a lib and a main.
    pub fn workspace(package_count: usize) -> Fixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("ws");
        fs::create_dir_all(&root).expect("ws dir");
        // write workspace manifest
        let members: Vec<String> = (0..package_count)
            .map(|i| format!("pkg{}", i))
            .collect();
        let toml = format!(
            "[workspace]\nmembers = [{}]\n",
            members.iter().map(|m| format!("\"{}\"", m)).collect::<Vec<_>>().join(", ")
        );
        fs::write(root.join("glyim.toml"), &toml).expect("ws toml");

        let mut all_sources = String::new();
        for i in 0..package_count {
            let pkg_dir = root.join(format!("pkg{}", i));
            let src_dir = pkg_dir.join("src");
            fs::create_dir_all(&src_dir).expect("pkg dir");
            let pkg_toml = format!(
                "[package]\nname = \"pkg{}\"\nversion = \"0.1.0\"\n",
                i
            );
            fs::write(pkg_dir.join("glyim.toml"), &pkg_toml).expect("pkg toml");
            let lib_src = format!(
                "pub fn add_{}(a: i64, b: i64) -> i64 {{ a + b }}\n",
                i
            );
            fs::write(src_dir.join("lib.g"), &lib_src).expect("lib g");
            all_sources.push_str(&lib_src);
            let main_src = format!(
                "fn main() -> i64 {{ let x = add_{}(1, 2); x }}\n",
                i
            );
            fs::write(src_dir.join("main.g"), &main_src).expect("main g");
            all_sources.push_str(&main_src);
        }
        // Return a Fixture that references the root path. We'll keep _tmp_dir alive.
        Fixture {
            source: all_sources,
            path: root,
            _tmp_dir: tmp,
        }
    }
}

/// A self-contained fixture that lives as long as the struct.
pub struct Fixture {
    pub source: String,
    pub path: PathBuf,
    _tmp_dir: tempfile::TempDir,
}

impl Fixture {
    /// Create a fixture whose lifetime is tied to a temporary directory.
    pub fn from_source(source: &str) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.g");
        fs::write(&path, source).expect("write");
        Fixture {
            source: source.to_string(),
            path,
            _tmp_dir: tmp,
        }
    }
}
