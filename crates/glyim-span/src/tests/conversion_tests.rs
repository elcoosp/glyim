use crate::*;
use miette::SourceSpan;

#[test]
fn test_fileid_conversions() {
    let raw = 123u32;
    let id = FileId::from_usize(raw as usize);
    assert_eq!(id.to_raw(), raw);
    assert_eq!(id.index(), raw as usize);
    assert_eq!(FileId::BOGUS.to_raw(), u32::MAX);
}

#[test]
fn test_byteidx_conversions() {
    let raw = 456u32;
    let idx = ByteIdx::from_usize(raw as usize);
    assert_eq!(idx.to_raw(), raw);
    assert_eq!(idx.to_usize(), raw as usize);
    assert_eq!(ByteIdx::ZERO.to_raw(), 0);
}

#[test]
fn test_syntaxcontext_from_raw() {
    let raw = 7u32;
    let ctx = SyntaxContext::from_raw(raw);
    assert_eq!(ctx.to_raw(), raw);
    assert!(!ctx.is_root());
    let root = SyntaxContext::ROOT;
    assert!(root.is_root());
    assert_eq!(root.to_raw(), 0);
}

#[test]
fn test_expnid_from_raw() {
    let raw = 42u32;
    let id = ExpnId::from_raw(raw);
    assert_eq!(id.to_raw(), raw);
    assert!(!id.is_root());
    let root = ExpnId::ROOT;
    assert!(root.is_root());
    assert_eq!(root.to_raw(), 0);
}

#[test]
fn test_span_to_miette_sourcespan() {
    let file = FileId::from_usize(0);
    let lo = ByteIdx::from_usize(5);
    let hi = ByteIdx::from_usize(12);
    let span = Span::range(file, lo, hi, SyntaxContext::ROOT);
    let source: SourceSpan = span.into();
    assert_eq!(source.offset(), 5);
    assert_eq!(source.len(), 7); // 12-5 =7
}
