use glyim_syntax::SyntaxKind;

pub fn is_sync_point(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::KwFn | SyntaxKind::KwLet | SyntaxKind::KwStruct | SyntaxKind::KwEnum | SyntaxKind::Eof)
}
pub fn is_block_end(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::RBrace)
}
