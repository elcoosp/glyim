use crate::*;

#[test]
fn test_span_new_valid() {
    let file = FileId::from_raw(1);
    let lo = ByteIdx::from_raw(5);
    let hi = ByteIdx::from_raw(10);
    let span = Span::range(file, lo, hi, SyntaxContext::ROOT);
    assert_eq!(span.file(), file);
    assert_eq!(span.lo(), lo);
    assert_eq!(span.hi(), hi);
    assert_eq!(span.ctx(), SyntaxContext::ROOT);
}

#[test]
#[should_panic(expected = "Span lo > hi")]
fn test_span_new_invalid_lo_gt_hi() {
    let file = FileId::from_raw(1);
    let lo = ByteIdx::from_raw(10);
    let hi = ByteIdx::from_raw(5);
    let _ = Span::range(file, lo, hi, SyntaxContext::ROOT);
}

#[test]
fn test_is_dummy() {
    assert!(Span::DUMMY.is_dummy());
    let real = Span::range(
        FileId::from_raw(0),
        ByteIdx::ZERO,
        ByteIdx::from_raw(1),
        SyntaxContext::ROOT,
    );
    assert!(!real.is_dummy());
}

#[test]
fn test_range() {
    let lo = ByteIdx::from_raw(3);
    let hi = ByteIdx::from_raw(7);
    let span = Span::range(FileId::from_raw(0), lo, hi, SyntaxContext::ROOT);
    let r = span.byte_range();
    assert_eq!(r.start, 3);
    assert_eq!(r.end, 7);
}

#[test]
fn test_sans_ctx() {
    let ctx = SyntaxContext::from_raw(42); // non-root
    let span = Span::range(
        FileId::from_raw(0),
        ByteIdx::ZERO,
        ByteIdx::from_raw(1),
        ctx,
    );
    let sans = span.sans_ctx();
    assert_eq!(sans.ctx(), SyntaxContext::ROOT);
    assert_eq!(sans.file(), span.file());
    assert_eq!(sans.lo(), span.lo());
    assert_eq!(sans.hi(), span.hi());
}

#[test]
fn test_len_and_is_empty() {
    let empty = Span::range(
        FileId::from_raw(0),
        ByteIdx::from_raw(5),
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());
    let nonempty = Span::range(
        FileId::from_raw(0),
        ByteIdx::from_raw(5),
        ByteIdx::from_raw(10),
        SyntaxContext::ROOT,
    );
    assert_eq!(nonempty.len(), 5);
    assert!(!nonempty.is_empty());
}

#[test]
fn test_to_merges_spans_same_file() {
    let file = FileId::from_raw(1);
    let a = Span::range(
        file,
        ByteIdx::from_raw(2),
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let b = Span::range(
        file,
        ByteIdx::from_raw(4),
        ByteIdx::from_raw(7),
        SyntaxContext::ROOT,
    );
    let merged = a.to(b);
    assert_eq!(merged.file(), file);
    assert_eq!(merged.lo(), ByteIdx::from_raw(2));
    assert_eq!(merged.hi(), ByteIdx::from_raw(7));
    assert_eq!(merged.ctx(), SyntaxContext::ROOT);
}

#[test]
#[should_panic(expected = "Cannot merge spans from different files")]
fn test_to_panics_different_files() {
    let a = Span::range(
        FileId::from_raw(1),
        ByteIdx::ZERO,
        ByteIdx::from_raw(1),
        SyntaxContext::ROOT,
    );
    let b = Span::range(
        FileId::from_raw(2),
        ByteIdx::ZERO,
        ByteIdx::from_raw(1),
        SyntaxContext::ROOT,
    );
    let _ = a.to(b);
}
