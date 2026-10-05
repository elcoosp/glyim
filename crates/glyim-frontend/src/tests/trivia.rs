use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_raw(0)).tokens
}

fn lex_result(source: &str) -> crate::lexer::LexResult {
    lex(source, FileId::from_raw(0))
}

#[test]
fn space_between_all_tokens() {
    let tokens = lex_tokens("fn foo ( x : i32 ) { }");
    assert_eq!(tokens.len(), 9);
    assert_eq!(tokens[0].kind, SyntaxKind::KwFn);
    assert_eq!(tokens[1].kind, SyntaxKind::Ident);
    assert_eq!(tokens[2].kind, SyntaxKind::LParen);
    assert_eq!(tokens[3].kind, SyntaxKind::Ident);
    assert_eq!(tokens[4].kind, SyntaxKind::Colon);
    assert_eq!(tokens[5].kind, SyntaxKind::Ident);
    assert_eq!(tokens[6].kind, SyntaxKind::RParen);
    assert_eq!(tokens[7].kind, SyntaxKind::LBrace);
    assert_eq!(tokens[8].kind, SyntaxKind::RBrace);
}

#[test]
fn tab_between_tokens() {
    let tokens = lex_tokens("fn\tfoo");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::KwFn);
    assert_eq!(tokens[1].kind, SyntaxKind::Ident);
}

#[test]
fn multiple_spaces_between_tokens() {
    let tokens = lex_tokens("fn    foo");
    assert_eq!(tokens.len(), 2);
}

#[test]
fn mixed_whitespace() {
    let tokens = lex_tokens("fn \t foo \n ( )");
    assert_eq!(tokens.len(), 4);
}

#[test]
fn leading_whitespace() {
    let tokens = lex_tokens("   fn");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::KwFn);
}

#[test]
fn trailing_whitespace() {
    let tokens = lex_tokens("fn   ");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::KwFn);
}

#[test]
fn only_whitespace() {
    let result = lex_result("   \t  \n  \r\n  ");
    assert!(result.tokens.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn line_comment_only() {
    let result = lex_result("// just a comment");
    assert!(result.tokens.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn line_comment_at_end() {
    let tokens = lex_tokens("42 // comment");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn line_comment_with_code_after_newline() {
    let tokens = lex_tokens("1 // comment\n2");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].text, "2");
}

#[test]
fn line_comment_with_special_chars() {
    let result = lex_result("// hello \"world\" 'a' 123 + - *\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_single_line() {
    let tokens = lex_tokens("/* comment */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_multiline() {
    let tokens = lex_tokens("/* line1\nline2\nline3 */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_before_and_after() {
    let tokens = lex_tokens("/* before */42/* after */");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn nested_block_comment_one_level() {
    let tokens = lex_tokens("/* outer /* inner */ still outer */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn nested_block_comment_two_levels() {
    let tokens = lex_tokens("/* a /* b /* c */ b2 */ a2 */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn multiple_nested_block_comments_separated() {
    let tokens = lex_tokens("/* a /* b */ c */ 1 /* d /* e */ f */ 2");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].text, "2");
}

#[test]
fn empty_block_comment() {
    let tokens = lex_tokens("/**/42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_with_stars() {
    let tokens = lex_tokens("/* *** * ** */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_with_slashes() {
    let tokens = lex_tokens("/* /// // */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn unterminated_block_comment() {
    let result = lex_result("42 /* never closed");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    // FE-18: an unterminated block comment is now diagnosed (Rust errors on
    // it too); previously it was silently swallowed as trivia.
    assert!(
        !result.diagnostics.is_empty(),
        "unterminated block comment must be diagnosed"
    );
}

#[test]
fn comment_not_inside_string() {
    let tokens = lex_tokens("\"/* not comment */\"");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn line_comment_not_inside_string() {
    let tokens = lex_tokens("\"// not comment\"");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn doc_comment_is_line_comment() {
    let result = lex_result("/// doc comment\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn inner_doc_comment_is_line_comment() {
    let result = lex_result("//! inner doc\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_doc_comment() {
    let result = lex_result("/** doc */42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn inner_block_doc_comment() {
    let result = lex_result("/*! inner doc */42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn comment_between_every_token() {
    let tokens = lex_tokens("fn/*a*/foo/*b*/(/*c*/x:i32/*d*/)/*e*/{}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwFn,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::RParen,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn comment_with_unicode_content() {
    let result = lex_result("// Unicode: \u{1F600}\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn windows_line_endings() {
    let tokens = lex_tokens("1\r\n2\r\n3");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].text, "2");
    assert_eq!(tokens[2].text, "3");
}

#[test]
fn mac_line_endings() {
    let tokens = lex_tokens("1\r2\r3");
    assert_eq!(tokens.len(), 3);
}

#[test]
fn mixed_line_endings() {
    let tokens = lex_tokens("1\n2\r\n3\r4");
    assert_eq!(tokens.len(), 4);
}

#[test]
fn vertical_tab_is_not_trivia() {
    // The lexer only recognizes space, tab, newline, carriage return as trivia.
    // Vertical tab (\u{000B}) is treated as an unexpected character.
    let result = lex_result("1\u{000B}2");
    assert_eq!(result.tokens.len(), 3, "should be: IntLit, Error, IntLit");
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::Error);
    assert_eq!(result.tokens[2].kind, SyntaxKind::IntLit);
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn form_feed_is_not_trivia() {
    // The lexer only recognizes space, tab, newline, carriage return as trivia.
    // Form feed (\u{000C}) is treated as an unexpected character.
    let result = lex_result("1\u{000C}2");
    assert_eq!(result.tokens.len(), 3, "should be: IntLit, Error, IntLit");
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::Error);
    assert_eq!(result.tokens[2].kind, SyntaxKind::IntLit);
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn comment_after_operator() {
    let tokens = lex_tokens("+/* comment */=");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::Plus);
    assert_eq!(tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn line_comment_before_block_comment() {
    let result = lex_result("// line\n/* block */42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn block_comment_before_line_comment() {
    let result = lex_result("/* block */// line\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}
