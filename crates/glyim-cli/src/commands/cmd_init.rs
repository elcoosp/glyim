use crate::pipeline;

pub fn cmd_init(name: String) -> i32 {
    match pipeline::init(&name) {
        Ok(path) => {
            eprintln!("Created {}/", path.display());
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
