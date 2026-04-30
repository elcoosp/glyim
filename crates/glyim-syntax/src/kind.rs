//! Syntax kind enumeration for the Glyim lossless CST.
use std::fmt;

pub const COUNT: u16 = 98;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u16)]
pub enum SyntaxKind {
    Error = 0,
    Eof,
    Whitespace,
    LineComment,
    BlockComment,
    IntLit,
    FloatLit,
    Ident,
    StringLit,
    KwFn,
    KwSelf,
    KwStruct,
    KwEnum,
    KwLet,
    KwIf,
    KwElse,
    KwReturn,
    KwUse,
    KwTrue,
    KwFalse,
    KwMatch,
    KwExtern,
    KwAs,
    KwPub,
    KwImpl,
    KwWhile,
    KwMut,
    Amp,
    Eq,
    FatArrow,
    Arrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,
    At,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    BangEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AmpAmp,
    PipePipe,
    Question,
    Hash,
    OpenBracket,
    CloseBracket,
    Bang,
    Pipe,
    SourceFile,
    FnDef,
    ParamList,
    Param,
    BlockExpr,
    LambdaExpr,
    CallExpr,
    IfExpr,
    BinaryExpr,
    PrefixExpr,
    LitExpr,
    PathExpr,
    MacroCallExpr,
    StructDef,
    EnumDef,
    MatchExpr,
    MatchArm,
    Pattern,
    FieldList,
    Field,
    VariantList,
    Variant,
    AsExpr,
    TryExpr,
    ExternBlock,
    ExternFn,
    FloatLitExpr,
    TypeAnnotation,
    PtrType,
    LetStmt,
    AssignStmt,
    ExprStmt,
    StructLitExpr,
    EnumVariantExpr,
    FieldAccessExpr,
    TupleLitExpr,
    ReturnExpr,
    WhileExpr,
    MatchArmPat,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::KwFn
                | Self::KwStruct
                | Self::KwEnum
                | Self::KwLet
                | Self::KwIf
                | Self::KwElse
                | Self::KwReturn
                | Self::KwUse
                | Self::KwTrue
                | Self::KwFalse
                | Self::KwMatch
                | Self::KwExtern
                | Self::KwAs
                | Self::KwPub
                | Self::KwImpl
        )
    }
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Eof => "end of file",
            Self::Whitespace | Self::LineComment | Self::BlockComment => "trivia",
            Self::IntLit => "integer literal",
            Self::FloatLit => "float literal",
            Self::Ident => "identifier",
            Self::StringLit => "string literal",
            Self::KwFn => "fn",
            Self::KwSelf => "self",
            Self::KwStruct => "struct",
            Self::KwEnum => "enum",
            Self::KwLet => "let",
            Self::KwIf => "if",
            Self::KwElse => "else",
            Self::KwWhile => "while",
            Self::Amp => "&",
            Self::KwMut => "mut",
            Self::KwReturn => "return",
            Self::KwUse => "use",
            Self::KwTrue => "true",
            Self::KwFalse => "false",
            Self::KwMatch => "match",
            Self::KwExtern => "extern",
            Self::KwAs => "as",
            Self::KwPub => "pub",
            Self::KwImpl => "impl",
            Self::Eq => "=",
            Self::FatArrow => "=>",
            Self::Arrow => "->",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::Comma => ",",
            Self::Colon => ":",
            Self::Semicolon => ";",
            Self::At => "@",
            Self::Dot => ".",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::EqEq => "==",
            Self::BangEq => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::LtEq => "<=",
            Self::GtEq => ">=",
            Self::AmpAmp => "&&",
            Self::PipePipe => "||",
            Self::Bang => "!",
            Self::Pipe => "|",
            Self::Question => "?",
            Self::Hash => "#",
            Self::OpenBracket => "[",
            Self::CloseBracket => "]",
            Self::SourceFile => "source file",
            Self::FnDef => "function definition",
            Self::ParamList => "parameter list",
            Self::Param => "parameter",
            Self::BlockExpr => "block expression",
            Self::LambdaExpr => "lambda expression",
            Self::IfExpr => "if expression",
            Self::CallExpr => "call expression",
            Self::BinaryExpr => "binary expression",
            Self::PrefixExpr => "prefix expression",
            Self::LitExpr => "literal expression",
            Self::PathExpr => "path expression",
            Self::MacroCallExpr => "macro call expression",
            Self::StructDef => "struct definition",
            Self::EnumDef => "enum definition",
            Self::MatchExpr => "match expression",
            Self::MatchArm => "match arm",
            Self::Pattern => "pattern",
            Self::FieldList => "field list",
            Self::Field => "field",
            Self::VariantList => "variant list",
            Self::Variant => "variant",
            Self::AsExpr => "as expression",
            Self::TryExpr => "try expression",
            Self::ExternBlock => "extern block",
            Self::ExternFn => "extern function",
            Self::FloatLitExpr => "float literal expression",
            Self::TypeAnnotation => "type annotation",
            Self::PtrType => "pointer type",
            Self::LetStmt => "let statement",
            Self::AssignStmt => "assignment statement",
            Self::ExprStmt => "expression statement",
            Self::StructLitExpr => "struct literal",
            Self::EnumVariantExpr => "enum variant",
            Self::FieldAccessExpr => "field access",
            Self::TupleLitExpr => "tuple literal",
            Self::ReturnExpr => "return expression",
            Self::WhileExpr => "while expression",
            Self::MatchArmPat => "match arm pattern",
        }
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trivia_kinds_are_trivia() {
        assert!(SyntaxKind::Whitespace.is_trivia());
        assert!(SyntaxKind::LineComment.is_trivia());
        assert!(SyntaxKind::BlockComment.is_trivia());
    }
    #[test]
    fn non_trivia_kinds_are_not_trivia() {
        assert!(!SyntaxKind::Ident.is_trivia());
        assert!(!SyntaxKind::IntLit.is_trivia());
        assert!(!SyntaxKind::Plus.is_trivia());
        assert!(!SyntaxKind::SourceFile.is_trivia());
    }
    #[test]
    fn keywords_are_keywords() {
        assert!(SyntaxKind::KwFn.is_keyword());
        assert!(SyntaxKind::KwLet.is_keyword());
        assert!(SyntaxKind::KwUse.is_keyword());
        assert!(SyntaxKind::KwPub.is_keyword());
        assert!(SyntaxKind::KwImpl.is_keyword());
    }
    #[test]
    fn non_keywords_are_not_keywords() {
        assert!(!SyntaxKind::Ident.is_keyword());
        assert!(!SyntaxKind::Eq.is_keyword());
    }
    #[test]
    fn display_name_matches_expected() {
        assert_eq!(SyntaxKind::FatArrow.display_name(), "=>");
        assert_eq!(SyntaxKind::IntLit.display_name(), "integer literal");
        assert_eq!(SyntaxKind::Error.display_name(), "error");
    }
    #[test]
    fn count_matches_actual_variants() {
        let _ = SyntaxKind::Error;
        let _ = SyntaxKind::MatchArmPat;
        assert_eq!(COUNT, 94);
    }
}
