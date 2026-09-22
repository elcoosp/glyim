//! Dumps the assembled stdlib source + its top-level module names.
//!
//! Used by scripts/glyim-with-stdlib.sh to build a combined
//! stdlib + user source file that resolves `println`, `Vec`, `Option`,
//! etc. without explicit `use` statements. Interim mechanism until
//! `glyim-cli` grows a native `--with-stdlib` flag.

fn main() {
    let src = glyim_lang_std::std_source_assembled();
    let src_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/glyim_assembled_stdlib.g".to_string());
    std::fs::write(&src_path, &src).expect("write assembled source");
    eprintln!("wrote {} bytes to {}", src.len(), src_path);

    let mut mods: Vec<String> = Vec::new();
    for line in src.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("pub mod ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                mods.push(name.to_string());
            }
        }
    }
    mods.sort();
    mods.dedup();
    let mods_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "/tmp/glyim_assembled_modules.txt".to_string());
    std::fs::write(&mods_path, mods.join("\n")).expect("write module list");
    eprintln!("top-level modules: {}", mods.join(", "));
}
