//! Rowan language definition and type aliases for the Glyim CST.
use crate::SyntaxKind;
use rowan::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyimLang;

impl Language for GlyimLang {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        debug_assert!(raw.0 < crate::COUNT, "invalid SyntaxKind: {}", raw.0);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<GlyimLang>;
pub type SyntaxToken = rowan::SyntaxToken<GlyimLang>;
pub type SyntaxElement = rowan::SyntaxElement<GlyimLang>;
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<GlyimLang>;
pub type GreenNode = rowan::GreenNode;
