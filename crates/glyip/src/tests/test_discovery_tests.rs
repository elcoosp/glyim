//! Tests for the `test_discovery` module — S23.

use crate::test_discovery::{DiscoveredTest, FileTestDiscovery};
use tempfile::TempDir;

#[test]
fn discover_test_with_attribute() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("attr_test.g");
    std::fs::write(&file, "#[test]\nfn my_test() {\n    assert(true);\n}\n").unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert_eq!(discovery.tests[0].name, "my_test");
}

#[test]
fn discover_test_with_naming_convention() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("naming_test.g");
    std::fs::write(
        &file,
        "fn test_first() {}\nfn test_second() {}\nfn main() {}\n",
    )
    .unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 2);
    assert_eq!(discovery.tests[0].name, "test_first");
    assert_eq!(discovery.tests[1].name, "test_second");
}

#[test]
fn discover_mixed_attribute_and_convention() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("mixed_test.g");
    std::fs::write(
        &file,
        "#[test]\nfn attr_test() {}\nfn test_convention() {}\nfn helper() {}\n",
    )
    .unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 2);
}

#[test]
fn discover_no_tests_in_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("no_tests.g");
    std::fs::write(&file, "fn main() {}\nfn helper() {}\n").unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 0);
}

#[test]
fn discover_line_numbers() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lines_test.g");
    std::fs::write(
        &file,
        "fn main() {}\n\n#[test]\nfn test_line_4() {}\n\n\nfn test_line_7() {}\n",
    )
    .unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 2);
    assert_eq!(discovery.tests[0].line, 4);
    assert_eq!(discovery.tests[1].line, 7);
}

#[test]
fn discover_pub_fn_test() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("pub_test.g");
    std::fs::write(&file, "pub fn test_visible() {}\n").unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert_eq!(discovery.tests[0].name, "test_visible");
}

#[test]
fn discover_test_with_generics() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("generic_test.g");
    std::fs::write(&file, "fn test_generic<T>() {}\n").unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert_eq!(discovery.tests[0].name, "test_generic");
}

#[test]
fn discover_empty_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.g");
    std::fs::write(&file, "").unwrap();

    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 0);
}

#[test]
fn discovered_test_equality() {
    let t1 = DiscoveredTest {
        name: "test_foo".to_string(),
        file: std::path::PathBuf::from("test.g"),
        line: 1,
        ignored: false,
    };
    let t2 = DiscoveredTest {
        name: "test_foo".to_string(),
        file: std::path::PathBuf::from("test.g"),
        line: 1,
        ignored: false,
    };
    assert_eq!(t1, t2);
}

#[test]
fn scan_nonexistent_file_returns_error() {
    let result = FileTestDiscovery::scan(std::path::Path::new("/nonexistent/file.g"));
    assert!(result.is_err());
}

// §2.1: the `#[ignore]` heuristic must be an exact allow-list, not a
// substring search, so ordinary prose comments mentioning "ignore" do not
// accidentally mark a test ignored.
#[test]
fn ordinary_comment_mentioning_ignore_does_not_mark_ignored() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("prose.g");
    std::fs::write(&file, "// we ignore errors here\n#[test]\nfn t() {}\n").unwrap();
    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert!(!discovery.tests[0].ignored);
}

#[test]
fn exact_ignore_marker_still_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ignore.g");
    std::fs::write(&file, "// #[ignore]\n#[test]\nfn t() {}\n").unwrap();
    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert!(discovery.tests[0].ignored);
}

#[test]
fn ignore_with_reason_comment_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("reason.g");
    std::fs::write(
        &file,
        "// #[ignore = \"flaky on CI\"]\n#[test]\nfn t() {}\n",
    )
    .unwrap();
    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert!(discovery.tests[0].ignored);
}

#[test]
fn plain_ignore_word_comment_still_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("pi.g");
    std::fs::write(&file, "// ignore\n#[test]\nfn t() {}\n").unwrap();
    let discovery = FileTestDiscovery::scan(&file).unwrap();
    assert_eq!(discovery.count(), 1);
    assert!(discovery.tests[0].ignored);
}
