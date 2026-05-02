use super::*;

#[cfg(test)]
mod root_tests {
    use super::*;
    #[test]
    fn find_package_root_in_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(
            find_package_root(dir.path()),
            Some(dir.path().to_path_buf())
        );
    }
    #[test]
    fn find_package_root_in_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        let child = dir.path().join("src");
        std::fs::create_dir_all(&child).unwrap();
        assert_eq!(find_package_root(&child), Some(dir.path().to_path_buf()));
    }
    #[test]
    fn find_package_root_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_package_root(dir.path()), None);
    }
    #[test]
    fn find_package_root_stops_at_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        let file_path = dir.path().join("src/main.g");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(&file_path, "main = () => 42").unwrap();
        assert_eq!(
            find_package_root(&file_path),
            Some(dir.path().to_path_buf())
        );
    }
}