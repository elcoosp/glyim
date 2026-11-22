use crate::lexer::lex;
use glyim_span::FileId;

#[test]
fn token_spans_cover_their_text() {
    let source = "fn main() {}";
    let result = lex(source, FileId::from_usize(0));
    for token in &result.tokens {
        let range = token.span.range();
        let extracted = &source[range];
        assert_eq!(
            extracted, token.text,
            "span text mismatch for token '{}'",
            token.text
        );
    }
}

#[test]
fn spans_are_monotonically_increasing() {
    let source = "fn main() { let x = 42; }";
    let result = lex(source, FileId::from_usize(0));
    let mut prev_end = 0usize;
    for token in &result.tokens {
        let range = token.span.range();
        assert!(
            range.start >= prev_end,
            "token '{}' at {}..{} overlaps with previous ending at {}",
            token.text,
            range.start,
            range.end,
            prev_end
        );
        prev_end = range.end;
    }
}

#[test]
fn span_file_id_preserved() {
    let fid = FileId::from_usize(42);
    let result = lex("fn", fid);
    assert_eq!(result.tokens[0].span.file, fid);
}

#[test]
fn empty_input_no_tokens_no_diagnostics() {
    let result = lex("", FileId::from_usize(0));
    assert!(
        result.tokens.is_empty(),
        "empty input should produce no tokens"
    );
    assert!(
        result.diagnostics.is_empty(),
        "empty input should produce no diagnostics"
    );
}

#[test]
fn whitespace_only_no_tokens_no_diagnostics() {
    let result = lex("   \n\t\r  ", FileId::from_usize(0));
    assert!(
        result.tokens.is_empty(),
        "whitespace-only input should produce no tokens"
    );
    assert!(
        result.diagnostics.is_empty(),
        "whitespace-only input should produce no diagnostics"
    );
}

#[test]
fn comment_only_no_tokens_no_diagnostics() {
    let result = lex("// just a comment\n/* block */", FileId::from_usize(0));
    assert!(
        result.tokens.is_empty(),
        "comment-only input should produce no tokens"
    );
    assert!(
        result.diagnostics.is_empty(),
        "comment-only input should produce no diagnostics"
    );
}

#[test]
fn spans_for_source_without_whitespace() {
    let source = "1+2";
    let result = lex(source, FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 3);

    assert_eq!(result.tokens[0].text, "1");
    assert_eq!(result.tokens[0].span.range().start, 0);
    assert_eq!(result.tokens[0].span.range().end, 1);

    assert_eq!(result.tokens[1].text, "+");
    assert_eq!(result.tokens[1].span.range().start, 1);
    assert_eq!(result.tokens[1].span.range().end, 2);

    assert_eq!(result.tokens[2].text, "2");
    assert_eq!(result.tokens[2].span.range().start, 2);
    assert_eq!(result.tokens[2].span.range().end, 3);
}

#[test]
fn contiguous_tokens_cover_entire_source() {
    let source = "1+2*3";
    let result = lex(source, FileId::from_usize(0));
    if let Some(last) = result.tokens.last() {
        let range = last.span.range();
        assert_eq!(
            range.end,
            source.len(),
            "last token span end should equal source length for contiguous source"
        );
    }
}
