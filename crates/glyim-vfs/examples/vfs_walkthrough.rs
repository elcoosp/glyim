//! Consumer walkthrough of the virtual filesystem: the path → `FileId`
//! interning seam every driver (compiler driver, LSP, glyip) sits on.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-vfs --example vfs_walkthrough
//! ```
//!
//! The example exercises the three registration faces — a real file on
//! disk, in-memory content (the LSP open-document path), and the
//! mutation face (`set_file_content`, the LSP change path) — plus the
//! lookups both keys support. An optional first argument is read as the
//! on-disk file (default: this example's own source, so the walkthrough
//! is self-contained).

use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let disk_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/vfs_walkthrough.rs")
        });

    let vfs = glyim_vfs::Vfs::new();

    // 1. Disk registration: the compiler-driver face. A missing file is an
    //    io::Result, not a panic.
    let file_id = vfs
        .add_file_from_disk(&disk_path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", disk_path.display()));
    println!("registered {} as FileId({file_id:?})", disk_path.display());

    // 2. In-memory registration: the LSP open-document face. Content-keyed
    //    by path; re-registering the same path overwrites (idempotent).
    let buffer_path = PathBuf::from("file:///lsp-opened.g");
    let buffer_id = vfs.add_file_content(&buffer_path, Arc::from("fn main() -> i32 { 0 }"));
    println!(
        "registered LSP buffer {:?} as FileId({buffer_id:?})",
        buffer_path.display()
    );

    // 3. Mutation: the LSP change face — the same FileId, new content.
    vfs.set_file_content(buffer_id, Arc::from("fn main() -> i32 { 42 }"));
    let current = vfs
        .file_content(buffer_id)
        .expect("buffer still registered");
    assert!(current.contains("42"), "content updated in place");

    // 4. Lookups on both keys: path → id and id → content/path.
    assert_eq!(vfs.file_id(&disk_path), Some(file_id));
    assert_eq!(
        vfs.file_path(buffer_id).as_deref(),
        Some(buffer_path.as_path())
    );
    assert!(vfs.file_id(&PathBuf::from("never-registered.g")).is_none());

    println!("vfs holds {} file(s); all lookups agreed", vfs.len());
}
