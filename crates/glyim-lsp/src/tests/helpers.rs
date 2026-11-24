use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

pub fn make_span(file_id: FileId, lo: usize, hi: usize) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_usize(lo),
        ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}
