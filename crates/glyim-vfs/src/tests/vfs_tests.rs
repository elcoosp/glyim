use crate::Vfs;
use glyim_span::FileId;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_file(content: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    fs::write(&path, content).unwrap();
    (path, dir)
}

#[test]
fn add_file_content_and_file_content_ref_roundtrip() {
    let vfs = Vfs::new();
    let path = PathBuf::from("/test/path.g");
    let content: Arc<str> = Arc::from("fn main() {}");

    let id = vfs.add_file_content(&path, content.clone());
    assert_eq!(id.index(), 0);
    assert_eq!(vfs.file_content(id), Some(content.clone()));
    let result = vfs.file_content_ref(id, |s: &str| s.to_owned());
    assert_eq!(result, Some((*content).to_string()));
}

#[test]
fn file_id_returns_correct_id() {
    let vfs = Vfs::new();
    let path1 = PathBuf::from("/a.g");
    let path2 = PathBuf::from("/b.g");
    let content: Arc<str> = Arc::from("content");

    let id1 = vfs.add_file_content(&path1, content.clone());
    let id2 = vfs.add_file_content(&path2, content);
    assert_eq!(vfs.file_id(&path1), Some(id1));
    assert_eq!(vfs.file_id(&path2), Some(id2));
    assert_eq!(vfs.file_id(&PathBuf::from("/missing.g")), None);
}

#[test]
fn set_file_content_updates_content() {
    let vfs = Vfs::new();
    let path = PathBuf::from("/test.g");
    let id = vfs.add_file_content(&path, Arc::from("old"));
    assert_eq!(vfs.file_content(id), Some(Arc::from("old")));

    vfs.set_file_content(id, Arc::from("new"))
        .expect("the write lands on a registered id");
    assert_eq!(vfs.file_content(id), Some(Arc::from("new")));
}

#[test]
fn add_file_from_disk_reads_file() {
    let vfs = Vfs::new();
    let (path, _dir) = temp_file("hello world");
    let id = vfs.add_file_from_disk(&path).unwrap();
    assert_eq!(vfs.file_content(id), Some(Arc::from("hello world")));
    assert_eq!(vfs.file_path(id), Some(path));
}

#[test]
fn add_file_content_updates_existing_path() {
    let vfs = Vfs::new();
    let path = PathBuf::from("/dup.g");
    let id1 = vfs.add_file_content(&path, Arc::from("first"));
    let id2 = vfs.add_file_content(&path, Arc::from("second"));
    // Same FileId should be returned
    assert_eq!(id1, id2);
    assert_eq!(vfs.file_content(id1), Some(Arc::from("second")));
}

#[test]
fn set_file_content_rejects_unknown_file_id() {
    let vfs = Vfs::new();
    let bogus_id = FileId::from_usize(999);
    let err = vfs
        .set_file_content(bogus_id, Arc::from("refused"))
        .expect_err("an unknown id never silently loses the write");
    assert_eq!(err, crate::VfsError::UnknownFile(bogus_id));
    assert_eq!(vfs.len(), 0);
}
