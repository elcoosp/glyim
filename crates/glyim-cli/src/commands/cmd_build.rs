use super::*;

pub fn cmd_build(
    input: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    release: bool,
    debug: bool,
) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
    let result = if input.is_dir() {
        pipeline::build_package(&input, output.as_deref(), mode, target.as_deref())
    } else {
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref())
    };
    match result {
        Ok(path) => {
            eprintln!("Built: {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
