//! M3.2 checked-offset gate tests (plan §6 M3.2 / §5.2 CT-3).
//!
//! The plan line these tests pin:
//!
//! > offsets > `u32::MAX` produce a diagnostic, not wraparound (FIXES §4
//! > class)
//!
//! Every byte offset a span can name is a `u32`, so a source longer than
//! [`glyim_span::MAX_SOURCE_LEN`] is refused at the VFS boundary — before
//! any cursor exists that could wrap — instead of silently narrowing
//! `usize` cursors inside the lexer.

use crate::{Vfs, VfsError};
use glyim_span::MAX_SOURCE_LEN;
use std::path::{Path, PathBuf};

#[test]
fn oversize_source_len_is_refused_with_the_real_length() {
    // The pure gate: a length past `u32::MAX` (tested without allocating
    // a 4 GiB source — the check sees the *length*, not the bytes) is
    // refused, naming the path and the refused size.
    let path = Path::new("/tmp/oversize.g");
    let oversize = MAX_SOURCE_LEN + 1;
    assert_eq!(
        Vfs::refuse_oversize_source(oversize, path),
        Err(VfsError::SourceTooLarge {
            path: path.to_path_buf(),
            len: oversize,
        })
    );
}

#[test]
fn boundary_length_is_accepted() {
    // `u32::MAX` itself is representable: every cursor in the file fits a
    // `ByteIdx`, so the gate accepts the exact boundary.
    let path = PathBuf::from("/tmp/boundary.g");
    assert_eq!(Vfs::refuse_oversize_source(MAX_SOURCE_LEN, &path), Ok(()));
    assert_eq!(Vfs::refuse_oversize_source(0, &path), Ok(()));
}

#[test]
fn oversize_display_reports_path_and_length() {
    let err = VfsError::SourceTooLarge {
        path: PathBuf::from("big.g"),
        len: MAX_SOURCE_LEN + 42,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("big.g") && msg.contains("4294967337"),
        "display must name the path and the real length, got: {msg}"
    );
}

#[test]
fn set_file_content_keeps_the_write_path_checked_but_total_for_small_sources() {
    // The LSP change path shares the gate: normal-sized writes still land
    // (through the new `Result` signature), and an oversized write would be
    // refused by the same pure predicate the setter calls before storing.
    let vfs = Vfs::new();
    let path = PathBuf::from("/test/small.g");
    let id = vfs
        .add_file_content(&path, std::sync::Arc::from("fn main() {}"))
        .expect("small sources always register");
    let updated: std::sync::Arc<str> = std::sync::Arc::from("fn updated() {}");
    vfs.set_file_content(id, updated.clone())
        .expect("small updates always land");
    assert_eq!(vfs.file_content(id), Some(updated));
}
