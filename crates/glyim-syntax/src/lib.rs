//! Crate root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, num_enum::TryFromPrimitive, PartialOrd)]
#[repr(u16)]
/// SyntaxKind.
pub enum SyntaxKind {
    // Keywords
    /// Variant.
    KwFn,
    /// Variant.
    KwLet,
    /// Variant.
    KwStruct,
    /// Variant.
    KwEnum,
    /// Variant.
    KwIf,
    /// Variant.
    KwElse,
    /// Variant.
    KwReturn,
    /// Variant.
    KwMatch,
    /// Variant.
    KwMod,
    /// Variant.
    KwComptime,
    /// Variant.
    KwSelf,
    /// Variant.
    KwSuper,
    /// Variant.
    KwCrate,
    /// Variant.
    KwTrue,
    /// Variant.
    KwFalse,
    /// Variant.
    KwMut,
    /// Variant.
    KwRef,
    /// Variant.
    KwAs,
    /// Variant.
    KwWhile,
    /// Variant.
    KwFor,
    /// Variant.
    KwLoop,
    /// Variant.
    KwIn,
    /// Variant.
    KwBreak,
    /// Variant.
    KwContinue,
    /// Variant.
    KwTrait,
    /// Variant.
    KwImpl,
    /// Variant.
    KwWhere,
    /// Variant.
    KwDyn,
    /// Variant.
    KwType,
    /// Variant.
    KwPub,
    /// Variant.
    KwPriv,
    /// Variant.
    KwExtern,
    /// Variant.
    KwUnsafe,
    /// Variant.
    KwUse,
    /// Variant.
    KwConst,
    /// Variant.
    KwStatic,
    /// Variant.
    KwMove,
    /// Variant.
    KwMacro,
    /// Variant.
    KwMacroRules,
    /// Variant.
    KwAsync,
    /// Variant.
    KwAwait,
    /// Variant.
    Lifetime,
    // Literals
    /// Variant.
    IntLit,
    /// Variant.
    FloatLit,
    /// Variant.
    StringLit,
    /// Variant.
    CharLit,
    /// Variant.
    ByteLit,
    /// Variant.
    BoolLit,
    /// Variant.
    Ident,
    // Operators
    /// Variant.
    Plus,
    /// Variant.
    Minus,
    /// Variant.
    Star,
    /// Variant.
    Slash,
    /// Variant.
    Percent,
    /// Variant.
    Eq,
    /// Variant.
    EqEq,
    /// Variant.
    Bang,
    /// Variant.
    BangEq,
    /// Variant.
    Lt,
    /// Variant.
    Gt,
    /// Variant.
    LtEq,
    /// Variant.
    GtEq,
    /// Variant.
    And,
    /// Variant.
    Or,
    /// Variant.
    AndAnd,
    /// Variant.
    OrOr,
    /// Variant.
    Caret,
    /// Variant.
    Shl,
    /// Variant.
    Shr,
    /// Variant.
    PlusEq,
    /// Variant.
    MinusEq,
    /// Variant.
    StarEq,
    /// Variant.
    SlashEq,
    /// Variant.
    OrEq,
    /// Variant.
    AndEq,
    /// Variant.
    CaretEq,
    /// Variant.
    ShlEq,
    /// Variant.
    ShrEq,
    // Punctuation
    /// Variant.
    Arrow,
    /// Variant.
    FatArrow,
    /// Variant.
    Dot,
    /// Variant.
    DotDot,
    /// Variant.
    DotDotEq,
    /// Variant.
    Comma,
    /// Variant.
    Semicolon,
    /// Variant.
    Colon,
    /// Variant.
    ColonColon,
    /// Variant.
    At,
    /// Variant.
    Hash,
    /// Variant.
    Dollar,
    /// Variant.
    Tilde,
    /// Variant.
    Underscore,
    /// Variant.
    Question,
    // Delimiters
    /// Variant.
    LParen,
    /// Variant.
    RParen,
    /// Variant.
    LBrace,
    /// Variant.
    RBrace,
    /// Variant.
    LBracket,
    /// Variant.
    RBracket,
    // Trivia
    /// Variant.
    Whitespace,
    /// Variant.
    LineComment,
    /// Variant.
    BlockComment,
    /// Variant.
    DocComment,
    // Nodes
    /// Variant.
    SourceFile,
    /// Variant.
    Module,
    /// Variant.
    FnDef,
    /// Variant.
    StructDef,
    /// Variant.
    EnumDef,
    /// Variant.
    TraitDef,
    /// Variant.
    ImplDef,
    /// Variant.
    TypeAlias,
    /// Variant.
    ConstDef,
    /// Variant.
    StaticDef,
    /// Variant.
    UseDecl,
    /// Variant.
    ExternBlock,
    /// Variant.
    ParamList,
    /// Variant.
    Param,
    /// Variant.
    TypeParamList,
    /// Variant.
    TypeParam,
    /// Variant.
    WhereClause,
    /// Variant.
    Block,
    /// Variant.
    LetStmt,
    /// Variant.
    ExprStmt,
    /// Variant.
    IfExpr,
    /// Variant.
    WhileExpr,
    /// Variant.
    LoopExpr,
    /// Variant.
    ForExpr,
    /// Variant.
    MatchExpr,
    /// Variant.
    MatchArmList,
    /// Variant.
    MatchArm,
    /// Variant.
    CallExpr,
    /// Variant.
    MethodCallExpr,
    /// Variant.
    FieldExpr,
    /// Variant.
    IndexExpr,
    /// Variant.
    UnaryExpr,
    /// Variant.
    BinaryExpr,
    /// Variant.
    CastExpr,
    /// Variant.
    RefExpr,
    /// Variant.
    ClosureExpr,
    /// Variant.
    PathExpr,
    /// Variant.
    TryExpr,
    /// Variant.
    AwaitExpr,
    /// Variant.
    ImplTraitType,
    /// Variant.
    LitExpr,
    /// Variant.
    ArrayExpr,
    /// Variant.
    TupleExpr,
    /// Variant.
    StructExpr,
    /// Variant.
    RangeExpr,
    /// Variant.
    BreakExpr,
    /// Variant.
    ContinueExpr,
    /// Variant.
    ReturnExpr,
    /// Variant.
    AssignExpr,
    /// Variant.
    RawPtrType,
    /// Variant.
    PathType,
    /// Variant.
    FnType,
    /// Variant.
    DynType,
    /// Variant.
    RefType,
    /// Variant.
    SliceType,
    /// Variant.
    ArrayType,
    /// Variant.
    TupleType,
    /// Variant.
    NeverType,
    /// Variant.
    InferType,
    /// Variant.
    GenericArgList,
    /// Variant.
    PatIdent,
    /// Variant.
    PatStruct,
    /// Variant.
    PatTuple,
    /// Variant.
    PatRef,
    /// Variant.
    PatOr,
    /// Variant.
    PatLit,
    /// Variant.
    PatRange,
    /// Variant.
    PatWild,
    /// Variant.
    PatSlice,
    /// Variant.
    UsePath,
    /// Variant.
    UseTree,
    /// Variant.
    MacroCall,
    /// Variant.
    TokenTree,
    /// Variant.
    MacroDef,
    /// Variant.
    MacroArm,
    /// Variant.
    MacroPattern,
    /// Variant.
    StructField,
    /// Variant.
    EnumVariant,
    /// Variant.
    FieldList,
    /// Variant.
    VariantList,
    // Error
    /// Variant.
    Error,
    // Visibility qualifiers
    /// Variant.
    Visibility,
    /// Variant.
    VisCrate,
    /// Variant.
    VisSuper,
    /// Variant.
    VisSelf,
    /// Variant.
    VisPath,
    // Where clause components
    /// Variant.
    WherePredicate,
    /// Variant.
    Bound,
    // Macro metavariables
    /// Variant.
    MetaVar,
    /// Variant.
    MetaVarCrate,
}
impl SyntaxKind {
    /// Returns true for whitespace and comment tokens.
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            SyntaxKind::Whitespace
                | SyntaxKind::LineComment
                | SyntaxKind::BlockComment
                | SyntaxKind::DocComment
        )
    }

    /// Returns true for literal tokens (integers, floats, strings, chars, booleans).
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            SyntaxKind::IntLit
                | SyntaxKind::FloatLit
                | SyntaxKind::StringLit
                | SyntaxKind::CharLit
                | SyntaxKind::BoolLit
        )
    }

    /// Returns true for keyword tokens.
    pub fn is_keyword(&self) -> bool {
        let raw = *self as u16;
        raw >= SyntaxKind::KwFn as u16 && raw <= SyntaxKind::KwMacroRules as u16
    }

    /// Returns true for node kinds (non-terminal syntax constructs).
    pub fn is_node(&self) -> bool {
        let raw = *self as u16;
        raw >= SyntaxKind::SourceFile as u16 && raw < SyntaxKind::Error as u16
    }

    /// Convert from raw u16 using the TryFromPrimitive derive.
    pub fn try_from_raw(raw: u16) -> Option<Self> {
        Self::try_from(raw).ok()
    }
}

/// Language type for rowan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyimLang;

impl rowan::Language for GlyimLang {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::try_from(raw.0).unwrap_or(SyntaxKind::Error)
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

/// Alias for rowan::SyntaxNode with GlyimLang.
pub type SyntaxNode = rowan::SyntaxNode<GlyimLang>;
/// Alias for rowan::SyntaxToken with GlyimLang.
pub type SyntaxToken = rowan::SyntaxToken<GlyimLang>;
/// Alias for rowan::SyntaxElement with GlyimLang.
pub type SyntaxElement = rowan::SyntaxElement<GlyimLang>;

pub use rowan::GreenNode;

/// AstNode.
pub trait AstNode {
    /// can_cast.
    fn can_cast(kind: SyntaxKind) -> bool;
    /// cast.
    fn cast(node: SyntaxNode) -> Option<Self>
    where
        Self: Sized;
    /// syntax.
    fn syntax(&self) -> &SyntaxNode;
}

/// child_of_kind.
pub fn child_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

macro_rules! ast_node {
    ($name:ident, $kind:expr) => {
        /// Struct.
        pub struct $name(SyntaxNode);
        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == $kind
            }
            fn cast(node: SyntaxNode) -> Option<Self> {
                if node.kind() == $kind {
                    Some(Self(node))
                } else {
                    None
                }
            }
            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(SourceFile, SyntaxKind::SourceFile);
ast_node!(FnDef, SyntaxKind::FnDef);
ast_node!(StructDef, SyntaxKind::StructDef);
ast_node!(EnumDef, SyntaxKind::EnumDef);
ast_node!(TraitDef, SyntaxKind::TraitDef);
ast_node!(ImplDef, SyntaxKind::ImplDef);
ast_node!(Block, SyntaxKind::Block);
ast_node!(CallExpr, SyntaxKind::CallExpr);
ast_node!(BinaryExpr, SyntaxKind::BinaryExpr);
ast_node!(PathExpr, SyntaxKind::PathExpr);
ast_node!(TryExpr, SyntaxKind::TryExpr);
ast_node!(AwaitExpr, SyntaxKind::AwaitExpr);
ast_node!(ImplTraitType, SyntaxKind::ImplTraitType);
ast_node!(LitExpr, SyntaxKind::LitExpr);
ast_node!(Module, SyntaxKind::Module);
ast_node!(TypeAlias, SyntaxKind::TypeAlias);
ast_node!(ConstDef, SyntaxKind::ConstDef);
ast_node!(StaticDef, SyntaxKind::StaticDef);
ast_node!(UseDecl, SyntaxKind::UseDecl);
ast_node!(ExternBlock, SyntaxKind::ExternBlock);
ast_node!(ParamList, SyntaxKind::ParamList);
ast_node!(Param, SyntaxKind::Param);
ast_node!(TypeParamList, SyntaxKind::TypeParamList);
ast_node!(TypeParam, SyntaxKind::TypeParam);
ast_node!(WhereClause, SyntaxKind::WhereClause);
ast_node!(LetStmt, SyntaxKind::LetStmt);
ast_node!(ExprStmt, SyntaxKind::ExprStmt);
ast_node!(IfExpr, SyntaxKind::IfExpr);
ast_node!(WhileExpr, SyntaxKind::WhileExpr);
ast_node!(LoopExpr, SyntaxKind::LoopExpr);
ast_node!(ForExpr, SyntaxKind::ForExpr);
ast_node!(MatchExpr, SyntaxKind::MatchExpr);
ast_node!(MatchArmList, SyntaxKind::MatchArmList);
ast_node!(MatchArm, SyntaxKind::MatchArm);
ast_node!(MethodCallExpr, SyntaxKind::MethodCallExpr);
ast_node!(FieldExpr, SyntaxKind::FieldExpr);
ast_node!(IndexExpr, SyntaxKind::IndexExpr);
ast_node!(UnaryExpr, SyntaxKind::UnaryExpr);
ast_node!(CastExpr, SyntaxKind::CastExpr);
ast_node!(RefExpr, SyntaxKind::RefExpr);
ast_node!(ClosureExpr, SyntaxKind::ClosureExpr);
ast_node!(ArrayExpr, SyntaxKind::ArrayExpr);
ast_node!(TupleExpr, SyntaxKind::TupleExpr);
ast_node!(StructExpr, SyntaxKind::StructExpr);
ast_node!(RangeExpr, SyntaxKind::RangeExpr);
ast_node!(BreakExpr, SyntaxKind::BreakExpr);
ast_node!(ContinueExpr, SyntaxKind::ContinueExpr);
ast_node!(ReturnExpr, SyntaxKind::ReturnExpr);
ast_node!(AssignExpr, SyntaxKind::AssignExpr);
ast_node!(PathType, SyntaxKind::PathType);
ast_node!(FnType, SyntaxKind::FnType);
ast_node!(DynType, SyntaxKind::DynType);
ast_node!(RefType, SyntaxKind::RefType);
ast_node!(SliceType, SyntaxKind::SliceType);
ast_node!(ArrayType, SyntaxKind::ArrayType);
ast_node!(TupleType, SyntaxKind::TupleType);
ast_node!(NeverType, SyntaxKind::NeverType);
ast_node!(InferType, SyntaxKind::InferType);
ast_node!(GenericArgList, SyntaxKind::GenericArgList);
ast_node!(PatIdent, SyntaxKind::PatIdent);
ast_node!(PatStruct, SyntaxKind::PatStruct);
ast_node!(PatTuple, SyntaxKind::PatTuple);
ast_node!(PatRef, SyntaxKind::PatRef);
ast_node!(PatOr, SyntaxKind::PatOr);
ast_node!(PatLit, SyntaxKind::PatLit);
ast_node!(PatRange, SyntaxKind::PatRange);
ast_node!(PatWild, SyntaxKind::PatWild);
ast_node!(PatSlice, SyntaxKind::PatSlice);
ast_node!(UsePath, SyntaxKind::UsePath);
ast_node!(UseTree, SyntaxKind::UseTree);
ast_node!(MacroCall, SyntaxKind::MacroCall);
ast_node!(TokenTree, SyntaxKind::TokenTree);
ast_node!(MacroDef, SyntaxKind::MacroDef);
ast_node!(MacroArm, SyntaxKind::MacroArm);
ast_node!(MacroPattern, SyntaxKind::MacroPattern);
ast_node!(StructField, SyntaxKind::StructField);
ast_node!(EnumVariant, SyntaxKind::EnumVariant);
ast_node!(FieldList, SyntaxKind::FieldList);
ast_node!(VariantList, SyntaxKind::VariantList);
pub use rowan::GreenToken;
