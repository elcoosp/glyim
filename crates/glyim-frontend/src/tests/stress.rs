use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;
use std::fmt::Write as _;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_raw(0)).tokens
}

fn lex_result(source: &str) -> crate::lexer::LexResult {
    lex(source, FileId::from_raw(0))
}

#[test]
fn long_identifier() {
    let long_ident = "a".repeat(1000);
    let tokens = lex_tokens(&long_ident);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text.len(), 1000);
}

#[test]
fn long_integer() {
    let long_int = "1".repeat(100);
    let tokens = lex_tokens(&long_int);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text.len(), 100);
}

#[test]
fn long_hex_integer() {
    let long_hex = format!("0x{}", "F".repeat(64));
    let tokens = lex_tokens(&long_hex);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn long_string() {
    let inner = "a".repeat(1000);
    let source = format!("\"{inner}\"");
    let tokens = lex_tokens(&source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn many_tokens() {
    let mut source = String::with_capacity(500 * 6);
    for i in 0..500 {
        let _ = write!(source, "x{i} ");
    }
    let result = lex_result(source.trim());
    assert_eq!(result.tokens.len(), 500);
    for token in &result.tokens {
        assert_eq!(token.kind, SyntaxKind::Ident);
    }
}

#[test]
fn many_integers() {
    let mut source = String::with_capacity(200 * 7);
    for i in 0..200 {
        let _ = write!(source, "{i} ");
    }
    let result = lex_result(source.trim());
    assert_eq!(result.tokens.len(), 200);
    for token in &result.tokens {
        assert_eq!(token.kind, SyntaxKind::IntLit);
    }
}

#[test]
fn alternating_tokens() {
    let source: String = (0..100)
        .map(|i| if i % 2 == 0 { "x + " } else { "1 " })
        .collect();
    let result = lex_result(source.trim());
    assert!(
        result.tokens.len() >= 100,
        "should have many tokens, got {}",
        result.tokens.len()
    );
}

#[test]
fn many_nested_comments() {
    let mut source = String::new();
    for _ in 0..50 {
        source.push_str("/* ");
    }
    for _ in 0..50 {
        source.push_str("*/ ");
    }
    source.push_str("42");
    let tokens = lex_tokens(&source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn many_line_comments() {
    let mut source = String::new();
    for i in 0..100 {
        let _ = writeln!(source, "// comment {i}");
    }
    source.push_str("42");
    let tokens = lex_tokens(&source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn large_source_no_diagnostics() {
    let source = r"
        fn main() -> i32 {
            let a = 1;
            let b = 2;
            let c = a + b;
            if c > 0 {
                return c;
            } else {
                return 0;
            }
        }
    "
    .repeat(50);
    let result = lex_result(&source);
    assert!(
        result.diagnostics.is_empty(),
        "should have no diagnostics for valid source"
    );
    assert!(!result.tokens.is_empty());
}

#[test]
fn many_operators_in_sequence() {
    let tokens = lex_tokens("+ - * / % = == != < > <= >= && || << >>");
    assert_eq!(tokens.len(), 16);
    let expected = vec![
        SyntaxKind::Plus,
        SyntaxKind::Minus,
        SyntaxKind::Star,
        SyntaxKind::Slash,
        SyntaxKind::Percent,
        SyntaxKind::Eq,
        SyntaxKind::EqEq,
        SyntaxKind::BangEq,
        SyntaxKind::Lt,
        SyntaxKind::Gt,
        SyntaxKind::LtEq,
        SyntaxKind::GtEq,
        SyntaxKind::AndAnd,
        SyntaxKind::OrOr,
        SyntaxKind::Shl,
        SyntaxKind::Shr,
    ];
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, expected);
}

#[test]
fn all_keywords_in_sequence() {
    let source = "fn let struct enum if else return match mod comptime self super crate true false mut ref as while for in break continue trait impl where type pub priv extern unsafe const static";
    let tokens = lex_tokens(source);
    assert!(
        tokens.len() >= 33,
        "should have at least 33 keyword tokens, got {}",
        tokens.len()
    );
    for token in &tokens {
        assert!(
            matches!(
                token.kind,
                SyntaxKind::KwFn
                    | SyntaxKind::KwLet
                    | SyntaxKind::KwStruct
                    | SyntaxKind::KwEnum
                    | SyntaxKind::KwIf
                    | SyntaxKind::KwElse
                    | SyntaxKind::KwReturn
                    | SyntaxKind::KwMatch
                    | SyntaxKind::KwMod
                    | SyntaxKind::KwComptime
                    | SyntaxKind::KwSelf
                    | SyntaxKind::KwSuper
                    | SyntaxKind::KwCrate
                    | SyntaxKind::KwTrue
                    | SyntaxKind::KwFalse
                    | SyntaxKind::KwMut
                    | SyntaxKind::KwRef
                    | SyntaxKind::KwAs
                    | SyntaxKind::KwWhile
                    | SyntaxKind::KwFor
                    | SyntaxKind::KwIn
                    | SyntaxKind::KwBreak
                    | SyntaxKind::KwContinue
                    | SyntaxKind::KwTrait
                    | SyntaxKind::KwImpl
                    | SyntaxKind::KwWhere
                    | SyntaxKind::KwType
                    | SyntaxKind::KwPub
                    | SyntaxKind::KwPriv
                    | SyntaxKind::KwExtern
                    | SyntaxKind::KwUnsafe
                    | SyntaxKind::KwConst
                    | SyntaxKind::KwStatic
            ),
            "token should be a keyword, got {:?} for '{}'",
            token.kind,
            token.text
        );
    }
}

#[test]
fn whitespace_variations() {
    let sources = vec![
        "fn main(){}",
        "fn main( ) { }",
        "fn  main  (  )  {  }",
        "fn\tmain\t(\t)\t{\t}",
        "fn\nmain\n(\n)\n{\n}",
    ];
    for source in sources {
        let tokens = lex_tokens(source);
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::KwFn,
                SyntaxKind::Ident,
                SyntaxKind::LParen,
                SyntaxKind::RParen,
                SyntaxKind::LBrace,
                SyntaxKind::RBrace,
            ],
            "whitespace variation failed for: {source:?}"
        );
    }
}

#[test]
fn integer_boundary_values() {
    let values = vec![
        "0",
        "1",
        "9",
        "10",
        "99",
        "100",
        "255",
        "256",
        "65535",
        "65536",
        "2147483647",
        "4294967295",
        "18446744073709551615",
    ];
    for v in values {
        let tokens = lex_tokens(v);
        assert_eq!(tokens.len(), 1, "should lex {v} as single token");
        assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    }
}

#[test]
fn hex_various_cases() {
    let cases: Vec<(&str, &str)> = vec![
        ("0x0", "0x0"),
        ("0xFF", "0xFF"),
        ("0xff", "0xff"),
        ("0xABcd", "0xABcd"),
        ("0x1234_5678", "0x1234_5678"),
    ];
    for (source, expected_text) in &cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex {source} as single token");
        assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
        assert_eq!(tokens[0].text, *expected_text);
    }
}

#[test]
fn float_various_formats() {
    let cases: Vec<(&str, SyntaxKind)> = vec![
        ("0.0", SyntaxKind::FloatLit),
        ("1.0", SyntaxKind::FloatLit),
        ("3.14", SyntaxKind::FloatLit),
        ("0.5", SyntaxKind::FloatLit),
        ("1e10", SyntaxKind::FloatLit),
        ("1E10", SyntaxKind::FloatLit),
        ("1.5e-3", SyntaxKind::FloatLit),
        ("2e+5", SyntaxKind::FloatLit),
        ("1.0e0", SyntaxKind::FloatLit),
        ("0.0e0", SyntaxKind::FloatLit),
        ("1.5e10f64", SyntaxKind::FloatLit),
        ("3.14f32", SyntaxKind::FloatLit),
    ];
    for (source, expected_kind) in &cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex {source} as single token");
        assert_eq!(tokens[0].kind, *expected_kind, "wrong kind for {source}");
    }
}

#[test]
fn string_various_contents() {
    let cases: Vec<(&str, &str)> = vec![
        ("\"\"", "\"\""),
        ("\"a\"", "\"a\""),
        ("\"hello world\"", "\"hello world\""),
    ];
    for (source, expected_text) in &cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex {source} as single token");
        assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
        assert_eq!(tokens[0].text, *expected_text, "text mismatch for {source}");
    }

    let escaped_cases: Vec<(&str, SyntaxKind)> = vec![
        (r#""\n""#, SyntaxKind::StringLit),
        (r#""\t\n\r""#, SyntaxKind::StringLit),
        (r#""\\""#, SyntaxKind::StringLit),
        (r#""\"""#, SyntaxKind::StringLit),
    ];
    for (source, expected_kind) in &escaped_cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex escaped string as single token");
        assert_eq!(tokens[0].kind, *expected_kind);
    }
}

#[test]
fn char_various_contents() {
    let cases: Vec<(&str, &str)> = vec![("'a'", "'a'"), ("'Z'", "'Z'"), ("'0'", "'0'")];
    for (source, expected_text) in &cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex {source} as single token");
        assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
        assert_eq!(tokens[0].text, *expected_text, "text mismatch for {source}");
    }

    let escaped_cases: Vec<(&str, SyntaxKind)> = vec![
        (r"'\n'", SyntaxKind::CharLit),
        (r"'\t'", SyntaxKind::CharLit),
        (r"'\\'", SyntaxKind::CharLit),
        (r"'\''", SyntaxKind::CharLit),
        (r"'\0'", SyntaxKind::CharLit),
    ];
    for (source, expected_kind) in &escaped_cases {
        let tokens = lex_tokens(source);
        assert_eq!(tokens.len(), 1, "should lex escaped char as single token");
        assert_eq!(tokens[0].kind, *expected_kind);
    }
}

#[test]
fn real_glyim_program() {
    let source = r"
        fn fibonacci(n: i32) -> i32 {
            if n <= 1 {
                return n;
            }
            return fibonacci(n - 1) + fibonacci(n - 2);
        }

        fn main() -> i32 {
            let result = fibonacci(10);
            return result;
        }
    ";
    let result = lex_result(source);
    assert!(result.diagnostics.is_empty(), "should have no diagnostics");
    assert!(!result.tokens.is_empty());
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwFn));
    assert!(kinds.contains(&SyntaxKind::KwIf));
    assert!(kinds.contains(&SyntaxKind::KwReturn));
    assert!(kinds.contains(&SyntaxKind::KwLet));
    assert!(kinds.contains(&SyntaxKind::Arrow));
}

#[test]
fn real_glyim_struct_and_impl() {
    let source = r"
        struct Point {
            x: f64,
            y: f64,
        }

        impl Point {
            fn new(x: f64, y: f64) -> Self {
                Self { x: x, y: y }
            }

            fn distance(&self) -> f64 {
                return (self.x * self.x + self.y * self.y);
            }
        }
    ";
    let result = lex_result(source);
    assert!(result.diagnostics.is_empty(), "should have no diagnostics");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwStruct));
    assert!(kinds.contains(&SyntaxKind::KwImpl));
    assert!(kinds.contains(&SyntaxKind::KwSelf));
}

#[test]
fn real_glyim_enum_with_match() {
    let source = r"
        enum Option {
            Some,
            None,
        }

        fn unwrap(opt: Option) -> i32 {
            match opt {
                Option::Some => 42,
                Option::None => 0,
            }
        }
    ";
    let result = lex_result(source);
    assert!(result.diagnostics.is_empty(), "should have no diagnostics");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwEnum));
    assert!(kinds.contains(&SyntaxKind::KwMatch));
    assert!(kinds.contains(&SyntaxKind::FatArrow));
}

#[test]
fn mixed_comments_and_code() {
    let source = r"
        // This is a function
        fn foo() -> i32 {
            /* multi
               line
               comment */
            let x = 42; // inline comment
            /* nested /* inner */ outer */ return x;
        }
    ";
    let result = lex_result(source);
    assert!(result.diagnostics.is_empty(), "should have no diagnostics");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwFn));
    assert!(kinds.contains(&SyntaxKind::KwLet));
    assert!(kinds.contains(&SyntaxKind::KwReturn));
}

#[test]
fn all_punctuation_roundtrip() {
    let source = "( ) { } [ ] , ; . : :: + - * / % = == != < > <= >= & | ^ @ # $ ~ ? ! += -= *= /= -> => .. ..= << >> && ||";
    let tokens = lex_tokens(source);
    assert!(
        tokens.len() >= 30,
        "should have many punctuation tokens, got {}",
        tokens.len()
    );
    for token in &tokens {
        assert!(
            !matches!(
                token.kind,
                SyntaxKind::Ident | SyntaxKind::IntLit | SyntaxKind::Error
            ),
            "punctuation should not be Ident/IntLit/Error, got {:?}",
            token.kind
        );
    }
}
