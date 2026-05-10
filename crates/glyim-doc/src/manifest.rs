use serde::Serialize;

#[derive(Serialize)]
pub struct DocManifest {
    pub package_name: String,
    pub version: String,
    pub items: Vec<DocItem>,
}

#[derive(Serialize)]
pub struct DocItem {
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub doc: Option<String>,
    pub signature_html: String,
    pub source_file: String,
    pub source_line: u32,
    pub highlighted_examples: Vec<HighlightedExample>,
    pub doc_test_results: Vec<DocTestResult>,
    pub is_pub: bool,
}

#[derive(Serialize)]
pub struct HighlightedExample {
    pub code: String,
    pub html: String,
    pub hash: String,
}

#[derive(Serialize)]
pub struct DocTestResult {
    pub example_index: usize,
    pub passed: bool,
    pub output: String,
}
