use crate::*;
use glyim_span::{ByteIdx, Span};

#[test]
fn source_info_new() {
    let si = SourceInfo::new(Span::DUMMY);
    assert!(si.span.is_dummy());
}

#[test]
fn source_info_with_real_span() {
    let span = Span::new(
        glyim_span::FileId::from_usize(1),
        ByteIdx::from_usize(10),
        ByteIdx::from_usize(20),
        glyim_span::SyntaxContext::ROOT,
    );
    let si = SourceInfo::new(span);
    assert_eq!(si.span.lo.to_usize(), 10);
    assert_eq!(si.span.hi.to_usize(), 20);
}
