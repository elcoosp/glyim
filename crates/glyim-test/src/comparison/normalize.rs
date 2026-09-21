use std::path::Path;

#[derive(Clone, Debug, Default)]
/// NormalizeRules.
pub struct NormalizeRules {
    /// Struct.
    pub normalize_slashes: bool,
    /// Struct.
    pub normalize_line_endings: bool,
    /// Struct.
    pub substitute_dir: bool,
}

/// normalize_output.
pub fn normalize_output(output: &str, test_path: &Path, rules: &NormalizeRules) -> String {
    let mut result = output.to_string();
    if rules.normalize_line_endings {
        result = result.replace("\r\n", "\n");
    }
    if rules.normalize_slashes {
        result = result.replace('\\', "/");
    }
    if rules.substitute_dir
        && let Some(parent) = test_path.parent()
    {
        let dir_str = parent.to_string_lossy().replace('\\', "/");
        result = result.replace(&dir_str, "$DIR");
    }
    result
}
