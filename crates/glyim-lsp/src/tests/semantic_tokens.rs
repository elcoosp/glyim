//! Policy-level tests for the semantic-tokens engine (LSP-SEMANTIC-TOKENS).
//!
//! Token classification is decided through an in-memory [`MockSource`]
//! implementing [`TokenLexSource`], not the live `AnalysisDatabase` —
//! the point of the trait seam: the *policy* (which legend index a
//! (kind, parent, grandparent, next) quadruple maps to) is verified
//! decoupled from any rowan tree, lock, or source-map machinery. The
//! parse-tree-backed path (real token extents, trivia skipping, wire
//! delta encoding) is covered in `semantic_tokens_tests`.

use crate::semantic_tokens::{ClassifiedToken, LexedToken, TokenLexSource, classify_tokens};
use glyim_syntax::SyntaxKind;

/// In-memory token store: the policy-side test double. Tokens are
/// appended in source order; `parent`/`grandparent`/`next_kind` default
/// to the [`SyntaxKind::SourceFile`] sentinel.
#[derive(Default)]
struct MockSource {
    tokens: Vec<LexedToken>,
}

impl MockSource {
    fn add(&mut self, lo: usize, hi: usize, kind: SyntaxKind) {
        self.tokens.push(LexedToken {
            lo,
            hi,
            kind,
            parent: SyntaxKind::SourceFile,
            grandparent: SyntaxKind::SourceFile,
            next_kind: SyntaxKind::SourceFile,
        });
    }

    /// A token with a chosen enclosing node and following-token kind —
    /// the two facts the identifier rules read.
    fn add_in(
        &mut self,
        lo: usize,
        hi: usize,
        kind: SyntaxKind,
        parent: SyntaxKind,
        next_kind: SyntaxKind,
    ) {
        self.tokens.push(LexedToken {
            lo,
            hi,
            kind,
            parent,
            grandparent: SyntaxKind::SourceFile,
            next_kind,
        });
    }

    /// A path identifier: nested `UsePath` → `Ident` with the path's own
    /// enclosing node (`PathType`/`PathExpr`) as grandparent — the
    /// probe-verified tree shape the path rules read.
    fn add_path(&mut self, lo: usize, hi: usize, path_of: SyntaxKind, next_kind: SyntaxKind) {
        self.tokens.push(LexedToken {
            lo,
            hi,
            kind: SyntaxKind::Ident,
            parent: SyntaxKind::UsePath,
            grandparent: path_of,
            next_kind,
        });
    }
}

impl TokenLexSource for MockSource {
    fn lexical_tokens(&self) -> Vec<LexedToken> {
        self.tokens.clone()
    }
}

/// Legend indices the assertions below pin (see `token_legend` in the
/// engine; the order there is the contract).
const KEYWORD: usize = 0;
const COMMENT: usize = 1;
const STRING: usize = 2;
const NUMBER: usize = 3;
const TYPE: usize = 4;
const FUNCTION: usize = 5;
const METHOD: usize = 6;
const PROPERTY: usize = 7;
const VARIABLE: usize = 8;
const PARAMETER: usize = 9;
const ENUM_MEMBER: usize = 10;
const MACRO: usize = 11;

#[test]
fn keywords_and_literals_classify_lexically() {
    let mut src = MockSource::default();
    src.add(0, 3, SyntaxKind::KwFn);
    src.add(8, 10, SyntaxKind::IntLit);
    src.add(15, 21, SyntaxKind::StringLit);
    src.add(24, 29, SyntaxKind::FloatLit);
    src.add(32, 36, SyntaxKind::CharLit);
    src.add(40, 46, SyntaxKind::LineComment);
    src.add(50, 62, SyntaxKind::DocComment);

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(
        kinds,
        vec![KEYWORD, NUMBER, STRING, NUMBER, STRING, COMMENT, COMMENT],
        "lexical classes need no context"
    );
}

#[test]
fn declaration_names_take_their_class_from_the_declaring_node() {
    let mut src = MockSource::default();
    // `fn greet` — a function name is followed by its parameter list.
    src.add_in(
        0,
        5,
        SyntaxKind::Ident,
        SyntaxKind::FnDef,
        SyntaxKind::LParen,
    );
    // `struct Point {` / `trait Draw {` / `type Alias =` — type names.
    src.add_in(
        10,
        15,
        SyntaxKind::Ident,
        SyntaxKind::StructDef,
        SyntaxKind::LBrace,
    );
    src.add_in(
        20,
        24,
        SyntaxKind::Ident,
        SyntaxKind::TraitDef,
        SyntaxKind::LBrace,
    );
    src.add_in(
        28,
        33,
        SyntaxKind::Ident,
        SyntaxKind::TypeAlias,
        SyntaxKind::Eq,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(
        kinds,
        vec![FUNCTION, TYPE, TYPE, TYPE],
        "declaration names take their class from the declaring node"
    );
}

#[test]
fn struct_fields_are_properties_but_the_struct_name_is_a_type() {
    // The parser nests definition fields directly under `StructDef`;
    // only the following token separates the field (`w:`) from the
    // struct's own name (`Rect {`).
    let mut src = MockSource::default();
    src.add_in(
        0,
        4,
        SyntaxKind::Ident,
        SyntaxKind::StructDef,
        SyntaxKind::LBrace,
    );
    src.add_in(
        7,
        8,
        SyntaxKind::Ident,
        SyntaxKind::StructDef,
        SyntaxKind::Colon,
    );
    // Literal fields sit under StructField: also properties.
    src.add_in(
        20,
        21,
        SyntaxKind::Ident,
        SyntaxKind::StructField,
        SyntaxKind::Colon,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(kinds, vec![TYPE, PROPERTY, PROPERTY]);
}

#[test]
fn enum_variants_are_members_and_enum_names_are_types() {
    let mut src = MockSource::default();
    src.add_in(
        0,
        5,
        SyntaxKind::Ident,
        SyntaxKind::EnumDef,
        SyntaxKind::LBrace,
    );
    src.add_in(
        8,
        11,
        SyntaxKind::Ident,
        SyntaxKind::EnumVariant,
        SyntaxKind::Comma,
    );
    src.add_in(
        14,
        19,
        SyntaxKind::Ident,
        SyntaxKind::EnumVariant,
        SyntaxKind::RBrace,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(kinds, vec![TYPE, ENUM_MEMBER, ENUM_MEMBER]);
}

#[test]
fn bindings_and_params_classify_as_variable_or_parameter() {
    let mut src = MockSource::default();
    // `let answer = 1` — the binding lives under PatIdent.
    src.add_in(
        4,
        10,
        SyntaxKind::Ident,
        SyntaxKind::PatIdent,
        SyntaxKind::Eq,
    );
    // `fn f(width: i32)` — the parameter name lives under Param.
    src.add_in(
        20,
        25,
        SyntaxKind::Ident,
        SyntaxKind::Param,
        SyntaxKind::Colon,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(kinds, vec![VARIABLE, PARAMETER]);
}

#[test]
fn members_methods_fields_classify_by_parent() {
    let mut src = MockSource::default();
    // `p.draw()` — the method name sits directly under MethodCallExpr.
    src.add_in(
        2,
        6,
        SyntaxKind::Ident,
        SyntaxKind::MethodCallExpr,
        SyntaxKind::LParen,
    );
    // `rect.fill` — the field name sits directly under FieldExpr.
    src.add_in(
        12,
        16,
        SyntaxKind::Ident,
        SyntaxKind::FieldExpr,
        SyntaxKind::Semicolon,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(kinds, vec![METHOD, PROPERTY]);
}

#[test]
fn path_idents_separate_calls_literals_and_references() {
    let mut src = MockSource::default();
    // `paint(c)` — callee `paint` (next `(`) vs argument `c` (next `)`):
    // both nest identically (UsePath under PathExpr under CallExpr);
    // only the following token separates them.
    src.add_path(0, 5, SyntaxKind::PathExpr, SyntaxKind::LParen);
    src.add_path(6, 7, SyntaxKind::PathExpr, SyntaxKind::RParen);
    // `Rect { .. }` — the literal's constructor name (next `{`).
    src.add_path(12, 16, SyntaxKind::PathExpr, SyntaxKind::LBrace);
    // `: Shape` — a type position (UsePath under PathType).
    src.add_path(22, 27, SyntaxKind::PathType, SyntaxKind::Semicolon);
    // `score` — a bare reference.
    src.add_path(32, 37, SyntaxKind::PathExpr, SyntaxKind::Semicolon);

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(
        kinds,
        vec![FUNCTION, VARIABLE, TYPE, TYPE, VARIABLE],
        "call and literal position read the next token; a bare path is a variable"
    );
}

#[test]
fn macro_and_gsx_attr_names_classify() {
    let mut src = MockSource::default();
    src.add_in(
        0,
        8,
        SyntaxKind::Ident,
        SyntaxKind::MacroCall,
        SyntaxKind::Bang,
    );
    src.add_in(
        20,
        27,
        SyntaxKind::Ident,
        SyntaxKind::GsxAttr,
        SyntaxKind::Eq,
    );
    src.add_in(
        30,
        40,
        SyntaxKind::Ident,
        SyntaxKind::GsxDirectiveAttr,
        SyntaxKind::Eq,
    );

    let out = classify_tokens(&src);
    let kinds: Vec<usize> = out.iter().map(|t| t.token_type).collect();
    assert_eq!(kinds, vec![MACRO, PROPERTY, PROPERTY]);
}

#[test]
fn unclassifiable_idents_and_punctuation_are_skipped() {
    let mut src = MockSource::default();
    // Punctuation/operators are deliberately not classified — the
    // client's static grammar keeps rendering those. (Lifetimes are not
    // in this fixture: `SyntaxKind::is_keyword` includes `Lifetime` in
    // its explicit FE-20 list, so they classify as keywords.)
    src.add(0, 1, SyntaxKind::Plus);
    src.add(2, 3, SyntaxKind::Semicolon);
    src.add(6, 7, SyntaxKind::Underscore);
    // An ident the policy has no parent rule for (e.g. inside a use
    // tree): skipped rather than guessed.
    src.tokens.push(LexedToken {
        lo: 10,
        hi: 14,
        kind: SyntaxKind::Ident,
        parent: SyntaxKind::UsePath,
        grandparent: SyntaxKind::UseTree,
        next_kind: SyntaxKind::ColonColon,
    });

    let out = classify_tokens(&src);
    assert!(out.is_empty(), "no token in this fixture qualifies");
}

#[test]
fn zero_length_and_non_monotonic_tokens_are_dropped() {
    let mut src = MockSource::default();
    src.add(0, 0, SyntaxKind::KwLet); // empty span: nothing to highlight
    src.add_in(
        10,
        20,
        SyntaxKind::Ident,
        SyntaxKind::FnDef,
        SyntaxKind::LParen,
    );
    src.add_in(
        5,
        8,
        SyntaxKind::Ident,
        SyntaxKind::FnDef,
        SyntaxKind::LParen,
    ); // rewinds
    src.add_in(
        22,
        30,
        SyntaxKind::Ident,
        SyntaxKind::FnDef,
        SyntaxKind::LParen,
    );

    let out = classify_tokens(&src);
    let los: Vec<usize> = out.iter().map(|t| t.lo).collect();
    assert_eq!(los, vec![10, 22], "output stays strictly monotonic");
}

#[test]
fn classification_preserves_extents() {
    let mut src = MockSource::default();
    src.add(3, 6, SyntaxKind::KwLet);
    let out = classify_tokens(&src);
    assert_eq!(
        out,
        vec![ClassifiedToken {
            lo: 3,
            hi: 6,
            token_type: KEYWORD,
        }]
    );
}
