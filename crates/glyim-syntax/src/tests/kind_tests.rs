//! Tests for SyntaxKind try_from_raw mapping

use crate::SyntaxKind;

#[test]
fn try_from_raw_roundtrip_all_variants() {
    // Known maximum raw value: last variant is Error
    let max_raw = SyntaxKind::Error as u16;
    for raw in 0..=max_raw {
        let kind = SyntaxKind::try_from_raw(raw);
        assert!(
            kind.is_some(),
            "Raw value {} should map to a SyntaxKind",
            raw
        );
        let kind = kind.unwrap();
        assert_eq!(kind as u16, raw, "Roundtrip failed for raw {}", raw);
    }
}

#[test]
fn try_from_raw_invalid_returns_none() {
    let invalid_raw = u16::MAX;
    assert_eq!(SyntaxKind::try_from_raw(invalid_raw), None);
}

#[test]
fn is_trivia_works() {
    assert!(SyntaxKind::Whitespace.is_trivia());
    assert!(SyntaxKind::LineComment.is_trivia());
    assert!(SyntaxKind::BlockComment.is_trivia());
    assert!(SyntaxKind::DocComment.is_trivia());
    assert!(!SyntaxKind::KwFn.is_trivia());
}

#[test]
fn is_keyword_works() {
    let keyword_variants = [
        SyntaxKind::KwFn,
        SyntaxKind::KwLet,
        SyntaxKind::KwStruct,
        SyntaxKind::KwEnum,
        SyntaxKind::KwIf,
        SyntaxKind::KwElse,
        SyntaxKind::KwReturn,
        SyntaxKind::KwMatch,
        SyntaxKind::KwMod,
        SyntaxKind::KwComptime,
        SyntaxKind::KwSelf,
        SyntaxKind::KwSuper,
        SyntaxKind::KwCrate,
        SyntaxKind::KwTrue,
        SyntaxKind::KwFalse,
        SyntaxKind::KwMut,
        SyntaxKind::KwRef,
        SyntaxKind::KwAs,
        SyntaxKind::KwWhile,
        SyntaxKind::KwFor,
        SyntaxKind::KwLoop,
        SyntaxKind::KwIn,
        SyntaxKind::KwBreak,
        SyntaxKind::KwContinue,
        SyntaxKind::KwTrait,
        SyntaxKind::KwImpl,
        SyntaxKind::KwWhere,
        SyntaxKind::KwDyn,
        SyntaxKind::KwType,
        SyntaxKind::KwPub,
        SyntaxKind::KwPriv,
        SyntaxKind::KwExtern,
        SyntaxKind::KwUnsafe,
        SyntaxKind::KwUse,
        SyntaxKind::KwConst,
        SyntaxKind::KwStatic,
        SyntaxKind::KwMove,
        SyntaxKind::KwMacro,
        SyntaxKind::KwMacroRules,
        // FE-20: these are declared after `KwMacroRules`; the old numeric range
        // excluded them.
        SyntaxKind::KwAsync,
        SyntaxKind::KwAwait,
        SyntaxKind::Lifetime,
    ];
    for kw in keyword_variants {
        assert!(kw.is_keyword(), "{:?} should be a keyword", kw);
    }
    assert!(!SyntaxKind::Ident.is_keyword());
}

#[test]
fn is_literal_works() {
    let literal_variants = [
        SyntaxKind::IntLit,
        SyntaxKind::FloatLit,
        SyntaxKind::StringLit,
        SyntaxKind::CharLit,
        SyntaxKind::BoolLit,
        SyntaxKind::ByteLit,
    ];
    for lit in literal_variants {
        assert!(lit.is_literal(), "{:?} should be a literal", lit);
    }
    // FE-7: `true`/`false` lex as keywords and ARE literals.
    assert!(SyntaxKind::KwTrue.is_literal());
    assert!(SyntaxKind::KwFalse.is_literal());
}

#[test]
fn is_node_works() {
    let node_variants = [
        SyntaxKind::SourceFile,
        SyntaxKind::Module,
        SyntaxKind::FnDef,
        SyntaxKind::StructDef,
        SyntaxKind::EnumDef,
        SyntaxKind::TraitDef,
        SyntaxKind::ImplDef,
        SyntaxKind::TypeAlias,
        SyntaxKind::ConstDef,
        SyntaxKind::StaticDef,
        SyntaxKind::UseDecl,
        SyntaxKind::ExternBlock,
        SyntaxKind::ParamList,
        SyntaxKind::Param,
        SyntaxKind::TypeParamList,
        SyntaxKind::TypeParam,
        SyntaxKind::WhereClause,
        SyntaxKind::Block,
        SyntaxKind::LetStmt,
        SyntaxKind::ExprStmt,
        SyntaxKind::IfExpr,
        SyntaxKind::WhileExpr,
        SyntaxKind::LoopExpr,
        SyntaxKind::ForExpr,
        SyntaxKind::MatchExpr,
        SyntaxKind::MatchArmList,
        SyntaxKind::MatchArm,
        SyntaxKind::CallExpr,
        SyntaxKind::MethodCallExpr,
        SyntaxKind::FieldExpr,
        SyntaxKind::IndexExpr,
        SyntaxKind::UnaryExpr,
        SyntaxKind::BinaryExpr,
        SyntaxKind::CastExpr,
        SyntaxKind::RefExpr,
        SyntaxKind::ClosureExpr,
        SyntaxKind::PathExpr,
        SyntaxKind::LitExpr,
        SyntaxKind::ArrayExpr,
        SyntaxKind::TupleExpr,
        SyntaxKind::StructExpr,
        SyntaxKind::RangeExpr,
        SyntaxKind::BreakExpr,
        SyntaxKind::ContinueExpr,
        SyntaxKind::ReturnExpr,
        SyntaxKind::AssignExpr,
        SyntaxKind::PathType,
        SyntaxKind::FnType,
        SyntaxKind::DynType,
        SyntaxKind::RefType,
        SyntaxKind::SliceType,
        SyntaxKind::ArrayType,
        SyntaxKind::TupleType,
        SyntaxKind::NeverType,
        SyntaxKind::InferType,
        SyntaxKind::GenericArgList,
        SyntaxKind::PatIdent,
        SyntaxKind::PatStruct,
        SyntaxKind::PatTuple,
        SyntaxKind::PatOr,
        SyntaxKind::PatLit,
        SyntaxKind::PatRange,
        SyntaxKind::PatWild,
        SyntaxKind::UsePath,
        SyntaxKind::UseTree,
        SyntaxKind::MacroCall,
        SyntaxKind::TokenTree,
        SyntaxKind::MacroDef,
        SyntaxKind::MacroArm,
        SyntaxKind::MacroPattern,
        SyntaxKind::StructField,
        SyntaxKind::EnumVariant,
        SyntaxKind::FieldList,
        SyntaxKind::VariantList,
        // FE-20: these are declared after `Error`; the old numeric range
        // excluded them.
        SyntaxKind::Visibility,
        SyntaxKind::VisCrate,
        SyntaxKind::VisSuper,
        SyntaxKind::VisSelf,
        SyntaxKind::VisPath,
        SyntaxKind::WherePredicate,
        SyntaxKind::Bound,
        SyntaxKind::MetaVar,
        SyntaxKind::MetaVarCrate,
    ];
    for node in node_variants {
        assert!(node.is_node(), "{:?} should be a node", node);
    }
    assert!(!SyntaxKind::Ident.is_node());
    assert!(!SyntaxKind::Error.is_node());
}
