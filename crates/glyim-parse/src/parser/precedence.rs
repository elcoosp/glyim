use crate::ast::BinOp;
use glyim_syntax::SyntaxKind;

pub(crate) fn infix_bp(kind: SyntaxKind) -> Option<(u8, u8)> {
    match kind {
        SyntaxKind::PipePipe => Some((10, 11)),
        SyntaxKind::AmpAmp => Some((20, 21)),
        SyntaxKind::EqEq | SyntaxKind::BangEq => Some((30, 31)),
        SyntaxKind::Lt | SyntaxKind::Gt | SyntaxKind::LtEq | SyntaxKind::GtEq => Some((40, 41)),
        SyntaxKind::Plus | SyntaxKind::Minus => Some((50, 51)),
        SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent => Some((60, 61)),
        _ => None,
    }
}

pub(crate) fn to_binop(kind: SyntaxKind) -> BinOp {
    match kind {
        SyntaxKind::Plus => BinOp::Add,
        SyntaxKind::Minus => BinOp::Sub,
        SyntaxKind::Star => BinOp::Mul,
        SyntaxKind::Slash => BinOp::Div,
        SyntaxKind::Percent => BinOp::Mod,
        SyntaxKind::EqEq => BinOp::Eq,
        SyntaxKind::BangEq => BinOp::Neq,
        SyntaxKind::Lt => BinOp::Lt,
        SyntaxKind::Gt => BinOp::Gt,
        SyntaxKind::LtEq => BinOp::Lte,
        SyntaxKind::GtEq => BinOp::Gte,
        SyntaxKind::AmpAmp => BinOp::And,
        SyntaxKind::PipePipe => BinOp::Or,
        _ => unreachable!(),
    }
}
