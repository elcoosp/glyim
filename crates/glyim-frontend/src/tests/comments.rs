use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn line_comment_is_trivia() {
    let tokens = lex_tokens("// this is a comment\n42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn block_comment_is_trivia() {
    let tokens = lex_tokens("/* comment */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn nested_block_comment_depth_1() {
    let tokens = lex_tokens("/* outer /* inner */ still outer */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn nested_block_comment_depth_2() {
    let tokens = lex_tokens("/* a /* b /* c */ b2 */ a2 */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn multiple_nested_block_comments() {
    let source = "/* x /* y */ z */ 1 /* a /* b */ c */ 2";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].text, "2");
}

#[test]
fn line_comment_at_end_of_file() {
    let tokens = lex_tokens("42 // trailing comment");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_at_end_of_file() {
    let tokens = lex_tokens("42 /* trailing */");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn unterminated_block_comment_is_trivia() {
    let tokens = lex_tokens("42 /* never closed");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn comment_between_operators() {
    let tokens = lex_tokens("1/*comment*/+2");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].kind, SyntaxKind::Plus);
    assert_eq!(tokens[2].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[2].text, "2");
}
