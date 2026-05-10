export interface DocManifest {
  package_name: string;
  version: string;
  items: DocItem[];
}

export interface DocItem {
  kind: string;
  name: string;
  qualified_name: string;
  doc: string | null;
  signature_html: string;
  source_file: string;
  source_line: number;
  highlighted_examples: HighlightedExample[];
  doc_test_results: DocTestResult[];
  is_pub: boolean;
}

export interface HighlightedExample {
  code: string;
  html: string;
  hash: string;
}

export interface DocTestResult {
  example_index: number;
  passed: boolean;
  output: string;
}

// Note: this file is outside docs/, so no default export needed.
