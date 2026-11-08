use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_raw(0)).tokens
}

fn lex_kinds(source: &str) -> Vec<SyntaxKind> {
    lex_tokens(source).iter().map(|t| t.kind).collect()
}

#[test]
fn fn_def_no_params() {
    let kinds = lex_kinds("fn foo() {}");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwFn,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::RParen,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn fn_def_with_params() {
    let kinds = lex_kinds("fn add(a: i32, b: i32) -> i32 { a + b }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwFn,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::Comma,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::RParen,
            SyntaxKind::Arrow,
            SyntaxKind::Ident,
            SyntaxKind::LBrace,
            SyntaxKind::Ident,
            SyntaxKind::Plus,
            SyntaxKind::Ident,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn struct_def() {
    let kinds = lex_kinds("struct Point { x: f64, y: f64 }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwStruct,
            SyntaxKind::Ident,
            SyntaxKind::LBrace,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::Comma,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn enum_def() {
    let kinds = lex_kinds("enum Color { Red, Green, Blue }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwEnum,
            SyntaxKind::Ident,
            SyntaxKind::LBrace,
            SyntaxKind::Ident,
            SyntaxKind::Comma,
            SyntaxKind::Ident,
            SyntaxKind::Comma,
            SyntaxKind::Ident,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn let_statement() {
    let kinds = lex_kinds("let x: i32 = 42;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwLet,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::Eq,
            SyntaxKind::IntLit,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn let_mut_statement() {
    let kinds = lex_kinds("let mut y = 0;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwLet,
            SyntaxKind::KwMut,
            SyntaxKind::Ident,
            SyntaxKind::Eq,
            SyntaxKind::IntLit,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn if_else() {
    let kinds = lex_kinds("if x > 0 { true } else { false }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwIf,
            SyntaxKind::Ident,
            SyntaxKind::Gt,
            SyntaxKind::IntLit,
            SyntaxKind::LBrace,
            SyntaxKind::KwTrue,
            SyntaxKind::RBrace,
            SyntaxKind::KwElse,
            SyntaxKind::LBrace,
            SyntaxKind::KwFalse,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn match_expr() {
    let kinds = lex_kinds("match x { 1 => true, _ => false }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwMatch,
            SyntaxKind::Ident,
            SyntaxKind::LBrace,
            SyntaxKind::IntLit,
            SyntaxKind::FatArrow,
            SyntaxKind::KwTrue,
            SyntaxKind::Comma,
            SyntaxKind::Underscore,
            SyntaxKind::FatArrow,
            SyntaxKind::KwFalse,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn return_expr() {
    let kinds = lex_kinds("return 42;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwReturn,
            SyntaxKind::IntLit,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn impl_block() {
    let kinds = lex_kinds("impl Point { fn new() -> Self { Self { x: 0.0, y: 0.0 } } }");
    assert!(kinds.starts_with(&[SyntaxKind::KwImpl, SyntaxKind::Ident, SyntaxKind::LBrace]));
}

#[test]
fn trait_def() {
    let kinds = lex_kinds("trait Display { fn fmt(&self); }");
    assert!(kinds.starts_with(&[SyntaxKind::KwTrait, SyntaxKind::Ident, SyntaxKind::LBrace]));
}

#[test]
fn pub_fn() {
    let kinds = lex_kinds("pub fn main() {}");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwPub,
            SyntaxKind::KwFn,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::RParen,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn while_loop() {
    let kinds = lex_kinds("while x > 0 { x = x - 1; }");
    assert_eq!(kinds[0], SyntaxKind::KwWhile);
    assert_eq!(kinds[1], SyntaxKind::Ident);
}

#[test]
fn for_in_loop() {
    let kinds = lex_kinds("for i in 0..10 {}");
    assert_eq!(
        &kinds[0..4],
        &[
            SyntaxKind::KwFor,
            SyntaxKind::Ident,
            SyntaxKind::KwIn,
            SyntaxKind::IntLit
        ]
    );
    assert!(kinds.contains(&SyntaxKind::DotDot));
}

#[test]
fn range_dot_dot() {
    let kinds = lex_kinds("0..10");
    assert_eq!(
        kinds,
        vec![SyntaxKind::IntLit, SyntaxKind::DotDot, SyntaxKind::IntLit]
    );
}

#[test]
fn range_inclusive_dot_dot_eq() {
    let kinds = lex_kinds("0..=10");
    assert_eq!(
        kinds,
        vec![SyntaxKind::IntLit, SyntaxKind::DotDotEq, SyntaxKind::IntLit]
    );
}

#[test]
fn use_path() {
    let kinds = lex_kinds("std::collections::HashMap");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Ident,
            SyntaxKind::ColonColon,
            SyntaxKind::Ident,
            SyntaxKind::ColonColon,
            SyntaxKind::Ident,
        ]
    );
}

#[test]
fn boolean_literals() {
    let kinds = lex_kinds("true false");
    assert_eq!(kinds, vec![SyntaxKind::KwTrue, SyntaxKind::KwFalse]);
}

#[test]
fn self_keyword() {
    let kinds = lex_kinds("self");
    assert_eq!(kinds, vec![SyntaxKind::KwSelf]);
}

#[test]
fn super_keyword() {
    let kinds = lex_kinds("super::foo");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwSuper,
            SyntaxKind::ColonColon,
            SyntaxKind::Ident
        ]
    );
}

#[test]
fn crate_keyword() {
    let kinds = lex_kinds("crate::module");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwCrate,
            SyntaxKind::ColonColon,
            SyntaxKind::Ident
        ]
    );
}

#[test]
fn unsafe_block() {
    let kinds = lex_kinds("unsafe {}");
    assert_eq!(
        kinds,
        vec![SyntaxKind::KwUnsafe, SyntaxKind::LBrace, SyntaxKind::RBrace]
    );
}

#[test]
fn extern_block() {
    let kinds = lex_kinds("extern \"C\" {}");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwExtern,
            SyntaxKind::StringLit,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn const_item() {
    let kinds = lex_kinds("const MAX: i32 = 100;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwConst,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::Eq,
            SyntaxKind::IntLit,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn static_item() {
    let kinds = lex_kinds("static COUNT: i32 = 0;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwStatic,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident,
            SyntaxKind::Eq,
            SyntaxKind::IntLit,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn where_clause() {
    let kinds = lex_kinds("where T: Display");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwWhere,
            SyntaxKind::Ident,
            SyntaxKind::Colon,
            SyntaxKind::Ident
        ]
    );
}

#[test]
fn type_alias() {
    let kinds = lex_kinds("type Result = Option<i32>;");
    assert_eq!(kinds[0], SyntaxKind::KwType);
    assert_eq!(kinds[1], SyntaxKind::Ident);
}

#[test]
fn break_and_continue() {
    let kinds = lex_kinds("break; continue;");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwBreak,
            SyntaxKind::Semicolon,
            SyntaxKind::KwContinue,
            SyntaxKind::Semicolon,
        ]
    );
}

#[test]
fn as_cast() {
    let kinds = lex_kinds("x as i32");
    assert_eq!(
        kinds,
        vec![SyntaxKind::Ident, SyntaxKind::KwAs, SyntaxKind::Ident]
    );
}

#[test]
fn ref_pattern() {
    let kinds = lex_kinds("let ref x = y;");
    assert_eq!(
        &kinds[0..3],
        &[SyntaxKind::KwLet, SyntaxKind::KwRef, SyntaxKind::Ident]
    );
}

#[test]
fn comptime_keyword() {
    let kinds = lex_kinds("comptime { }");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwComptime,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace
        ]
    );
}

#[test]
fn priv_keyword() {
    let kinds = lex_kinds("priv fn internal() {}");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::KwPriv,
            SyntaxKind::KwFn,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::RParen,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn complex_expression() {
    let kinds = lex_kinds("(a + b) * (c - d) / 2");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::LParen,
            SyntaxKind::Ident,
            SyntaxKind::Plus,
            SyntaxKind::Ident,
            SyntaxKind::RParen,
            SyntaxKind::Star,
            SyntaxKind::LParen,
            SyntaxKind::Ident,
            SyntaxKind::Minus,
            SyntaxKind::Ident,
            SyntaxKind::RParen,
            SyntaxKind::Slash,
            SyntaxKind::IntLit,
        ]
    );
}

#[test]
fn comparison_operators_in_expr() {
    let kinds = lex_kinds("a == b && c != d || e < f");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Ident,
            SyntaxKind::EqEq,
            SyntaxKind::Ident,
            SyntaxKind::AndAnd,
            SyntaxKind::Ident,
            SyntaxKind::BangEq,
            SyntaxKind::Ident,
            SyntaxKind::OrOr,
            SyntaxKind::Ident,
            SyntaxKind::Lt,
            SyntaxKind::Ident,
        ]
    );
}

#[test]
fn bitwise_operators() {
    let kinds = lex_kinds("a & b | c ^ d");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Ident,
            SyntaxKind::And,
            SyntaxKind::Ident,
            SyntaxKind::Or,
            SyntaxKind::Ident,
            SyntaxKind::Caret,
            SyntaxKind::Ident,
        ]
    );
}

#[test]
fn shift_operators() {
    let kinds = lex_kinds("a << 2 >> 1");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Ident,
            SyntaxKind::Shl,
            SyntaxKind::IntLit,
            SyntaxKind::Shr,
            SyntaxKind::IntLit,
        ]
    );
}

#[test]
fn compound_assignment() {
    // x += 1; y -= 2; z *= 3; w /= 4;
    // 0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
    // x += 1 ;  y -= 2 ;  z *= 3 ;  w /= 4 ;
    let kinds = lex_kinds("x += 1; y -= 2; z *= 3; w /= 4;");
    assert_eq!(kinds[1], SyntaxKind::PlusEq);
    assert_eq!(kinds[5], SyntaxKind::MinusEq);
    assert_eq!(kinds[9], SyntaxKind::StarEq);
    assert_eq!(kinds[13], SyntaxKind::SlashEq);
}

#[test]
fn deref_and_not() {
    let kinds = lex_kinds("*x !y");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Star,
            SyntaxKind::Ident,
            SyntaxKind::Bang,
            SyntaxKind::Ident,
        ]
    );
}

#[test]
fn question_mark() {
    let kinds = lex_kinds("x?");
    assert_eq!(kinds, vec![SyntaxKind::Ident, SyntaxKind::Question]);
}

#[test]
fn at_sign() {
    let kinds = lex_kinds("@derive");
    assert_eq!(kinds, vec![SyntaxKind::At, SyntaxKind::Ident]);
}

#[test]
fn hash_sign() {
    let kinds = lex_kinds("#[test]");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Hash,
            SyntaxKind::LBracket,
            SyntaxKind::Ident,
            SyntaxKind::RBracket,
        ]
    );
}

#[test]
fn dollar_sign() {
    let kinds = lex_kinds("$var");
    assert_eq!(kinds, vec![SyntaxKind::Dollar, SyntaxKind::Ident]);
}

#[test]
fn tilde() {
    let tokens = lex_tokens("~");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Tilde);
}

#[test]
fn string_in_fn_call() {
    let kinds = lex_kinds("println(\"hello\")");
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::StringLit,
            SyntaxKind::RParen,
        ]
    );
}

#[test]
fn char_in_match() {
    let kinds = lex_kinds("'a' | 'b'");
    assert_eq!(
        kinds,
        vec![SyntaxKind::CharLit, SyntaxKind::Or, SyntaxKind::CharLit,]
    );
}

#[test]
fn full_function_with_comments() {
    let source = r"// This is a function
fn compute(x: i32) -> i32 {
    /* block comment */
    let y = x + 1; // add one
    y
}";
    let kinds = lex_kinds(source);
    assert_eq!(kinds[0], SyntaxKind::KwFn);
    assert!(kinds.contains(&SyntaxKind::IntLit));
    assert!(kinds.contains(&SyntaxKind::Plus));
}
